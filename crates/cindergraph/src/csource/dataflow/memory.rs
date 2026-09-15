//! Flow-insensitive local points-to sets and weak indirect writes.
//!
//! These sets never justify killing a possible target. Pointer reassignment
//! unions targets; subsequent direct assignments still kill projected writes.
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::{
    Binding, CallMemoryArgument, DataFlow, DefKind, Definition, MemoryAccess, MemoryAccessKind,
    MemoryAccessPrecision, MemoryDefinition, MemoryDefinitionKind, MemoryOverlapKind, MemoryRegion,
    MemoryRegionId, MemoryRegionKind, MemoryRegionOverlap, MemoryUse, SemanticIssueKind, Use,
};
use crate::csource::cfg::FunctionCfg;
use crate::csource::eval::{
    EvaluationPlan, PlaceId, PointerValueSource, ProjectedBase, ProjectedPlaceId,
    ProjectedPlaceKind, SemanticPlace, TypeValueOp, ValueId,
};
use crate::csource::lex::kind::TokenKind;
use crate::csource::parse::{tag::NodeTag, Tree};
use crate::syntax::ids::Span;

use super::interproc::{MemoryEffectPath, ParameterMemoryEffectKind, Summaries};

fn contains(outer: Span, inner: Span) -> bool {
    outer.lo <= inner.lo && inner.hi <= outer.hi
}

fn contains_bit(words: &[u64], index: usize) -> bool {
    words
        .get(index / u64::BITS as usize)
        .is_some_and(|word| word & (1 << (index % u64::BITS as usize)) != 0)
}

fn set_bit(words: &mut [u64], index: usize, value: bool) {
    let Some(word) = words.get_mut(index / u64::BITS as usize) else {
        return;
    };
    let mask = 1 << (index % u64::BITS as usize);
    if value {
        *word |= mask;
    } else {
        *word &= !mask;
    }
}

fn first_use_within(span: Span, uses_by_start: &[usize], uses: &[Use]) -> Option<usize> {
    let first = uses_by_start.partition_point(|&index| uses[index].span.lo < span.lo);
    let after = uses_by_start.partition_point(|&index| uses[index].span.lo <= span.hi);
    uses_by_start[first..after]
        .iter()
        .copied()
        .filter(|&index| contains(span, uses[index].span))
        // Preserve `uses.iter().find(...)` semantics even where several reads
        // share one source position or a recovered expression is malformed.
        .min()
}

fn exact_use_at(
    span: Span,
    binding: Binding,
    uses_by_start: &[usize],
    uses: &[Use],
) -> Option<usize> {
    let first = uses_by_start.partition_point(|&index| uses[index].span.lo < span.lo);
    let after = uses_by_start.partition_point(|&index| uses[index].span.lo <= span.lo);
    uses_by_start[first..after]
        .iter()
        .copied()
        .find(|&index| uses[index].binding == binding && uses[index].span == span)
}

fn operation_pointer_targets(
    sources: &[PointerValueSource],
    flow: &DataFlow,
    binding_by_place: &BTreeMap<PlaceId, Binding>,
    targets: &[BTreeSet<Binding>],
    unknown: &BTreeSet<Binding>,
    definitely_initialized: &[bool],
    uses_by_start: &[usize],
) -> (BTreeSet<Binding>, bool) {
    let pointer = |binding: Binding| {
        flow.types
            .get(binding.0 as usize)
            .is_some_and(|ty| ty.pointer_depth > 0)
    };
    let mut result = BTreeSet::new();
    let mut incomplete = false;
    for source in sources {
        match *source {
            PointerValueSource::Address(place) => {
                if let Some(&binding) = binding_by_place.get(&place) {
                    result.insert(binding);
                } else {
                    incomplete = true;
                }
            }
            PointerValueSource::Copy { place, occurrence } => {
                let Some(&binding) = binding_by_place.get(&place).filter(|&&b| pointer(b)) else {
                    incomplete = true;
                    continue;
                };
                let initialized = exact_use_at(occurrence, binding, uses_by_start, &flow.uses)
                    .is_some_and(|index| definitely_initialized[index]);
                if !initialized {
                    incomplete = true;
                }
                let possible = &targets[binding.0 as usize];
                incomplete |= possible.is_empty() || unknown.contains(&binding);
                result.extend(possible.iter().copied());
            }
            PointerValueSource::Load {
                pointer: place,
                occurrence,
            } => {
                let Some(&outer) = binding_by_place.get(&place).filter(|&&b| pointer(b)) else {
                    incomplete = true;
                    continue;
                };
                if exact_use_at(occurrence, outer, uses_by_start, &flow.uses)
                    .is_none_or(|index| !definitely_initialized[index])
                {
                    incomplete = true;
                }
                let pointee_pointers = &targets[outer.0 as usize];
                incomplete |= pointee_pointers.is_empty() || unknown.contains(&outer);
                for &inner in pointee_pointers {
                    if !pointer(inner) {
                        incomplete = true;
                        continue;
                    }
                    let possible = &targets[inner.0 as usize];
                    incomplete |= possible.is_empty() || unknown.contains(&inner);
                    result.extend(possible.iter().copied());
                }
            }
            PointerValueSource::Unknown => incomplete = true,
        }
    }
    if result.is_empty() {
        incomplete = true;
    }
    (result, incomplete)
}

/// Whether each pointer read has a definition on every path to that point.
///
/// Points-to targets are deliberately flow-insensitive, but completeness must
/// not be: a later `p = &x` cannot make an earlier read of uninitialized `p`
/// safe. This small must-analysis tracks pointer initialization separately
/// from the may-target closure.
fn definitely_initialized_uses(flow: &DataFlow, cfg: &crate::syntax::cfg::Cfg) -> Vec<bool> {
    let mut initialized_at_entry_or_declaration = vec![false; flow.names.len()];
    for definition in &flow.definitions {
        if matches!(definition.kind, DefKind::Parameter | DefKind::Declaration) {
            if let Some(initialized) =
                initialized_at_entry_or_declaration.get_mut(definition.binding.0 as usize)
            {
                *initialized = true;
            }
        }
    }
    let mut pointer_position = vec![usize::MAX; flow.names.len()];
    let mut pointer_count = 0usize;
    for (index, ty) in flow.types.iter().enumerate() {
        if ty.pointer_depth > 0 && !initialized_at_entry_or_declaration[index] {
            pointer_position[index] = pointer_count;
            pointer_count += 1;
        }
    }
    if pointer_count == 0 || cfg.node_count() == 0 {
        return vec![true; flow.uses.len()];
    }

    let words = pointer_count.div_ceil(u64::BITS as usize);
    let mut generated = vec![vec![0u64; words]; cfg.node_count()];
    let mut local_effects: BTreeMap<(u32, Binding), Vec<u32>> = BTreeMap::new();
    for definition in &flow.definitions {
        let position = pointer_position
            .get(definition.binding.0 as usize)
            .copied()
            .unwrap_or(usize::MAX);
        if position == usize::MAX
            || matches!(
                definition.kind,
                DefKind::AddressTaken | DefKind::MemoryWrite
            )
        {
            continue;
        }
        set_bit(&mut generated[definition.node as usize], position, true);
        local_effects
            .entry((definition.node, definition.binding))
            .or_default()
            .push(definition.effect_at);
    }
    for effects in local_effects.values_mut() {
        effects.sort_unstable();
    }

    let mut reachable = vec![false; cfg.node_count()];
    let mut pending = VecDeque::from([cfg.entry()]);
    while let Some(node) = pending.pop_front() {
        if node.index() >= reachable.len() || reachable[node.index()] {
            continue;
        }
        reachable[node.index()] = true;
        pending.extend(cfg.successors(node));
    }

    let mut top = vec![u64::MAX; words];
    if let Some(last) = top.last_mut() {
        let remainder = pointer_count % u64::BITS as usize;
        if remainder != 0 {
            *last = (1u64 << remainder) - 1;
        }
    }
    let mut in_ = vec![vec![0u64; words]; cfg.node_count()];
    let mut out = vec![vec![0u64; words]; cfg.node_count()];
    for node in 0..cfg.node_count() {
        if reachable[node] && node != cfg.entry().index() {
            in_[node].clone_from(&top);
            out[node].clone_from(&top);
        }
    }
    let bound = cfg
        .node_count()
        .saturating_mul(pointer_count)
        .saturating_add(8);
    for _ in 0..bound {
        let mut changed = false;
        for node in 0..cfg.node_count() {
            if !reachable[node] {
                continue;
            }
            let id = crate::syntax::ids::NodeId::new(node as u32);
            let mut incoming = if node == cfg.entry().index() {
                vec![0u64; words]
            } else {
                top.clone()
            };
            let mut has_predecessor = false;
            for predecessor in cfg.predecessors(id) {
                if !reachable[predecessor.index()] {
                    continue;
                }
                has_predecessor = true;
                for (slot, value) in incoming.iter_mut().zip(&out[predecessor.index()]) {
                    *slot &= *value;
                }
            }
            if node != cfg.entry().index() && !has_predecessor {
                incoming.fill(0);
            }
            let mut next = incoming.clone();
            for (slot, value) in next.iter_mut().zip(&generated[node]) {
                *slot |= *value;
            }
            if incoming != in_[node] {
                in_[node] = incoming;
                changed = true;
            }
            if next != out[node] {
                out[node] = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    flow.uses
        .iter()
        .map(|use_| {
            let position = pointer_position
                .get(use_.binding.0 as usize)
                .copied()
                .unwrap_or(usize::MAX);
            if position == usize::MAX {
                return true;
            }
            contains_bit(&in_[use_.node as usize], position)
                || local_effects
                    .get(&(use_.node, use_.binding))
                    .is_some_and(|effects| effects.partition_point(|&at| at <= use_.span.lo) > 0)
        })
        .collect()
}

fn strip_parentheses(
    tree: &Tree,
    mut node: crate::syntax::ids::NodeId,
) -> crate::syntax::ids::NodeId {
    let arena = tree.arena();
    while arena.tag(node) == Some(NodeTag::ParenExpr.as_u16()) {
        let Some(child) = arena.children_iter(node).next() else {
            break;
        };
        node = child;
    }
    node
}

/// Remove parentheses and transparent `*&place` pairs from a memory place.
fn strip_transparent_place(
    tree: &Tree,
    mut node: crate::syntax::ids::NodeId,
) -> crate::syntax::ids::NodeId {
    loop {
        node = strip_parentheses(tree, node);
        match super::events::dereference_of_address(tree, node) {
            Some((_, target)) => node = target,
            None => return node,
        }
    }
}

fn inc_dec_place(
    tree: &Tree,
    node: crate::syntax::ids::NodeId,
) -> Option<crate::syntax::ids::NodeId> {
    let arena = tree.arena();
    match arena.tag(node).and_then(NodeTag::from_u16) {
        Some(NodeTag::UnaryExpr)
            if arena.main_token(node).is_some_and(|token| {
                matches!(
                    TokenKind::from_u16(tree.tokens().kind(token)),
                    Some(TokenKind::PlusPlus | TokenKind::MinusMinus)
                )
            }) =>
        {
            arena.children_iter(node).next()
        }
        Some(NodeTag::PostfixExpr)
            if arena
                .children_iter(node)
                .any(|child| arena.tag(child) == Some(NodeTag::IncDecSuffix.as_u16())) =>
        {
            arena.children_iter(node).next()
        }
        _ => None,
    }
}

/// Dereferences canceled by an immediately enclosing address-of operation.
///
/// C defines `&*p` in terms of the pointer value: it neither loads the pointee
/// nor takes the address of `p`. Parentheses between the operators do not
/// change that rule. Deeper dereferences still execute (`&**q` reads `*q`), so
/// only the address operator's direct, parenthesis-stripped operand is kept.
fn addressed_dereferences(
    tree: &Tree,
    root: crate::syntax::ids::NodeId,
) -> BTreeSet<crate::syntax::ids::NodeId> {
    let arena = tree.arena();
    let mut canceled = BTreeSet::new();
    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::UnaryExpr.as_u16())
            || !arena.main_token(node).is_some_and(|token| {
                TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Amp)
            })
        {
            continue;
        }
        let Some(operand) = arena.children_iter(node).next() else {
            continue;
        };
        let dereference = strip_parentheses(tree, operand);
        if arena.tag(dereference) == Some(NodeTag::UnaryExpr.as_u16())
            && arena.main_token(dereference).is_some_and(|token| {
                TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Star)
            })
        {
            canceled.insert(dereference);
        }
    }
    canceled
}

/// The outer `*` in a computed goto selects a control destination; it is not a
/// load from the destination address. Nested dereferences remain ordinary
/// memory operations. The CFG is authoritative about which statement spans
/// are indirect dispatches, and the widest `*` expression inside that span is
/// the grammar's dispatch operator.
fn control_dereferences(
    tree: &Tree,
    spans: &[Span],
    function: &FunctionCfg,
) -> BTreeSet<crate::syntax::ids::NodeId> {
    let arena = tree.arena();
    function
        .cfg
        .indirect_dispatches()
        .iter()
        .filter_map(|dispatch| {
            let statement = function.cfg.node(dispatch.node)?.span();
            arena
                .preorder(function.node)
                .filter(|node| {
                    arena.tag(*node) == Some(NodeTag::UnaryExpr.as_u16())
                        && arena.main_token(*node).is_some_and(|token| {
                            TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Star)
                        })
                        && arena
                            .span(*node, spans)
                            .is_some_and(|span| contains(statement, span))
                })
                .max_by_key(|node| arena.span(*node, spans).map_or(0, |span| span.hi - span.lo))
        })
        .collect()
}

fn has_pointer_arithmetic(
    tree: &Tree,
    expression: Span,
    discarded_values: &super::regions::Regions,
) -> bool {
    let tokens = tree.tokens();
    // Token starts are monotone. Restrict the scan to this initializer or
    // assignment so wide pointer-copy functions stay near-linear.
    let first = tokens
        .starts()
        .partition_point(|&start| start < expression.lo);
    let after = tokens
        .starts()
        .partition_point(|&start| start < expression.hi);
    tokens.kinds()[first..after]
        .iter()
        .zip(&tokens.starts()[first..after])
        .any(|(&kind, &start)| {
            !discarded_values.contains(Span::empty_at(start))
                && matches!(
                    TokenKind::from_u16(kind),
                    Some(
                        TokenKind::Plus
                            | TokenKind::Minus
                            | TokenKind::PlusPlus
                            | TokenKind::MinusMinus
                    )
                )
        })
}

fn cast_operand_is_known_pointer(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    operand: crate::syntax::ids::NodeId,
    uses_by_start: &[usize],
    uses: &[Use],
    pointer: &impl Fn(Binding) -> bool,
) -> bool {
    let arena = tree.arena();
    let mut pending = vec![operand];
    while let Some(node) = pending.pop() {
        match arena.tag(node).and_then(NodeTag::from_u16) {
            Some(NodeTag::ParenExpr) => {
                let Some(child) = arena.children_iter(node).next() else {
                    return false;
                };
                pending.push(child);
            }
            Some(NodeTag::CastExpr) => {
                let mut children = arena.children_iter(node);
                let Some(type_name) = children.next() else {
                    return false;
                };
                let Some(operand) = children.next() else {
                    return false;
                };
                let Some(type_span) = arena.span(type_name, spans) else {
                    return false;
                };
                // Only an explicitly pointer-typed intermediate cast
                // preserves a local points-to identity. A pointer converted
                // through an integer type may round-trip on a particular ABI,
                // but this portable source model cannot certify that.
                if !text
                    .get(type_span.lo as usize..type_span.hi as usize)
                    .is_some_and(|source| source.contains('*'))
                {
                    return false;
                }
                pending.push(operand);
            }
            Some(NodeTag::CommaExpr) => {
                let Some(last) = arena.children_iter(node).last() else {
                    return false;
                };
                pending.push(last);
            }
            Some(NodeTag::CondExpr) => {
                let branches: Vec<_> = arena.children_iter(node).skip(1).collect();
                if branches.len() != 2 {
                    return false;
                }
                pending.extend(branches);
            }
            Some(NodeTag::UnaryExpr) => {
                let Some(span) = arena.span(node, spans) else {
                    return false;
                };
                let Some(source) = text.get(span.lo as usize..span.hi as usize) else {
                    return false;
                };
                if source.trim_start().starts_with('&') {
                    continue;
                }
                if !source.trim_start().starts_with('*') {
                    return false;
                }
                let Some(child) = arena.children_iter(node).next() else {
                    return false;
                };
                pending.push(child);
            }
            Some(NodeTag::NameRef) => {
                let Some(span) = arena.span(node, spans) else {
                    return false;
                };
                let first = uses_by_start.partition_point(|&index| uses[index].span.lo < span.lo);
                let after = uses_by_start.partition_point(|&index| uses[index].span.lo <= span.lo);
                if !uses_by_start[first..after]
                    .iter()
                    .any(|&index| uses[index].span == span && pointer(uses[index].binding))
                {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}

fn propagate_targets(
    targets: &mut [BTreeSet<Binding>],
    unknown: &mut BTreeSet<Binding>,
    copies: &[(Binding, Binding)],
    load_copies: &[(Binding, Binding)],
    pointer: &impl Fn(Binding) -> bool,
) {
    loop {
        // Direct copies are ordinary subset constraints. Drive only their
        // dependants when a source grows; rescanning the complete constraint
        // list makes a reverse-ordered chain quadratic.
        let mut successors = vec![Vec::new(); targets.len()];
        for &(source, dest) in copies {
            successors[source.0 as usize].push(dest);
        }
        let mut queued = vec![true; targets.len()];
        let mut pending: VecDeque<Binding> = (0..targets.len() as u32).map(Binding).collect();
        while let Some(source) = pending.pop_front() {
            queued[source.0 as usize] = false;
            let source_unknown = unknown.contains(&source);
            let inherited = targets[source.0 as usize].clone();
            for &dest in &successors[source.0 as usize] {
                let mut dest_changed = source_unknown && unknown.insert(dest);
                for &target in &inherited {
                    dest_changed |= targets[dest.0 as usize].insert(target);
                }
                if dest_changed && !queued[dest.0 as usize] {
                    queued[dest.0 as usize] = true;
                    pending.push_back(dest);
                }
            }
        }

        // A dereference load depends on both its source's targets and the
        // targets of every pointer it may name. Re-evaluate these less common
        // constraints after direct copies settle, then close direct copies
        // again only when a load grew its destination.
        let mut load_changed = false;
        for &(source, dest) in load_copies {
            if unknown.contains(&source) {
                load_changed |= unknown.insert(dest);
            }
            let intermediate = targets[source.0 as usize].clone();
            if intermediate.is_empty() {
                load_changed |= unknown.insert(dest);
            }
            for pointer_target in intermediate {
                if !pointer(pointer_target) || unknown.contains(&pointer_target) {
                    load_changed |= unknown.insert(dest);
                }
                if pointer(pointer_target) {
                    let inherited = targets[pointer_target.0 as usize].clone();
                    for target in inherited {
                        load_changed |= targets[dest.0 as usize].insert(target);
                    }
                }
            }
        }
        if !load_changed {
            break;
        }
    }
}

pub(super) struct ProjectionContext<'a> {
    pub(super) tree: &'a Tree,
    pub(super) text: &'a str,
    pub(super) spans: &'a [Span],
    pub(super) function: &'a FunctionCfg,
    pub(super) evaluation: &'a EvaluationPlan,
    pub(super) binding_by_place: &'a BTreeMap<PlaceId, Binding>,
    pub(super) unevaluated: &'a super::regions::Regions,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum RegionKey {
    Binding(Binding),
    ParameterPointee(u32, Binding),
    Field(Box<RegionKey>, String, bool),
    Elements(Box<RegionKey>),
}

fn intern_region(
    key: &RegionKey,
    by_key: &mut BTreeMap<RegionKey, MemoryRegionId>,
    regions: &mut Vec<MemoryRegion>,
) -> MemoryRegionId {
    if let Some(id) = by_key.get(key) {
        return *id;
    }
    let kind = match key {
        RegionKey::Binding(binding) => MemoryRegionKind::Binding { binding: *binding },
        RegionKey::ParameterPointee(parameter, binding) => MemoryRegionKind::ParameterPointee {
            parameter: *parameter,
            binding: *binding,
        },
        RegionKey::Field(base, member, overlapping_members) => MemoryRegionKind::Field {
            base: intern_region(base, by_key, regions),
            member: member.clone(),
            overlapping_members: *overlapping_members,
        },
        RegionKey::Elements(base) => MemoryRegionKind::Elements {
            base: intern_region(base, by_key, regions),
        },
    };
    let id = MemoryRegionId(regions.len() as u32);
    regions.push(MemoryRegion { id, kind });
    by_key.insert(key.clone(), id);
    id
}

struct RegionResolutionContext<'a> {
    evaluation: &'a EvaluationPlan,
    flow: &'a DataFlow,
    binding_by_place: &'a BTreeMap<PlaceId, Binding>,
    targets: &'a [BTreeSet<Binding>],
    unknown: &'a BTreeSet<Binding>,
    definitely_initialized: &'a [bool],
    uses_by_start: &'a [usize],
    parameter_origins: &'a [BTreeSet<u32>],
    parameter_bindings: &'a [Binding],
}

struct RegionInputs<'a> {
    binding_by_place: &'a BTreeMap<PlaceId, Binding>,
    targets: &'a [BTreeSet<Binding>],
    unknown: &'a BTreeSet<Binding>,
    definitely_initialized: &'a [bool],
    uses_by_start: &'a [usize],
    parameter_origins: &'a [BTreeSet<u32>],
    parameter_bindings: &'a [Binding],
}

fn pointer_region_roots(
    value: ValueId,
    context: &RegionResolutionContext<'_>,
    cache: &mut BTreeMap<ValueId, (Vec<(RegionKey, bool)>, bool)>,
) -> (Vec<(RegionKey, bool)>, bool) {
    if let Some(result) = cache.get(&value) {
        return result.clone();
    }
    let sources = context.evaluation.pointer_sources(value);
    let (targets, incomplete) = operation_pointer_targets(
        &sources,
        context.flow,
        context.binding_by_place,
        context.targets,
        context.unknown,
        context.definitely_initialized,
        context.uses_by_start,
    );
    let mut roots = targets
        .into_iter()
        // Flow-insensitive target sets are may facts even when only one
        // target currently happens to be present.
        .map(|binding| (RegionKey::Binding(binding), false))
        .collect::<Vec<_>>();
    let mut abstractly_represented = !sources.is_empty();
    for source in &sources {
        match *source {
            PointerValueSource::Copy { place, .. } => {
                let Some(&binding) = context.binding_by_place.get(&place) else {
                    abstractly_represented = false;
                    continue;
                };
                let Some(origins) = context.parameter_origins.get(binding.0 as usize) else {
                    abstractly_represented = false;
                    continue;
                };
                if origins.is_empty() {
                    abstractly_represented = false;
                }
                roots.extend(origins.iter().filter_map(|&parameter| {
                    context
                        .parameter_bindings
                        .get(parameter as usize)
                        .copied()
                        .map(|formal| (RegionKey::ParameterPointee(parameter, formal), false))
                }));
            }
            PointerValueSource::Address(_) => {}
            PointerValueSource::Load { .. } | PointerValueSource::Unknown => {
                abstractly_represented = false;
            }
        }
    }
    roots.sort();
    roots.dedup();
    let result = (roots, incomplete && !abstractly_represented);
    cache.insert(value, result.clone());
    result
}

fn projected_base_regions(
    base: ProjectedBase,
    context: &RegionResolutionContext<'_>,
    visiting: &mut BTreeSet<ProjectedPlaceId>,
    pointer_cache: &mut BTreeMap<ValueId, (Vec<(RegionKey, bool)>, bool)>,
    projected_cache: &mut BTreeMap<ProjectedPlaceId, (Vec<(RegionKey, bool)>, bool)>,
) -> (Vec<(RegionKey, bool)>, bool) {
    match base {
        ProjectedBase::Place(place) => match context.binding_by_place.get(&place) {
            Some(binding) => (vec![(RegionKey::Binding(*binding), true)], false),
            None => (Vec::new(), true),
        },
        ProjectedBase::Value(value) => pointer_region_roots(value, context, pointer_cache),
        ProjectedBase::Projected(place) => {
            projected_regions(place, context, visiting, pointer_cache, projected_cache)
        }
    }
}

fn projected_regions(
    place: ProjectedPlaceId,
    context: &RegionResolutionContext<'_>,
    visiting: &mut BTreeSet<ProjectedPlaceId>,
    pointer_cache: &mut BTreeMap<ValueId, (Vec<(RegionKey, bool)>, bool)>,
    projected_cache: &mut BTreeMap<ProjectedPlaceId, (Vec<(RegionKey, bool)>, bool)>,
) -> (Vec<(RegionKey, bool)>, bool) {
    if let Some(result) = projected_cache.get(&place) {
        return result.clone();
    }
    if !visiting.insert(place) {
        return (Vec::new(), true);
    }
    let result = match context
        .evaluation
        .projected_places()
        .get(place.0 as usize)
        .map(|place| &place.kind)
    {
        Some(ProjectedPlaceKind::Dereference { address }) => {
            pointer_region_roots(*address, context, pointer_cache)
        }
        Some(ProjectedPlaceKind::Field {
            base,
            member,
            overlapping_members,
        }) => {
            let (bases, incomplete) =
                projected_base_regions(*base, context, visiting, pointer_cache, projected_cache);
            (
                bases
                    .into_iter()
                    .map(|(base, exact)| {
                        (
                            RegionKey::Field(Box::new(base), member.clone(), *overlapping_members),
                            exact,
                        )
                    })
                    .collect(),
                incomplete,
            )
        }
        Some(ProjectedPlaceKind::Element { base, .. }) => {
            let (bases, incomplete) =
                projected_base_regions(*base, context, visiting, pointer_cache, projected_cache);
            (
                bases
                    .into_iter()
                    .map(|(base, _)| (RegionKey::Elements(Box::new(base)), false))
                    .collect(),
                incomplete,
            )
        }
        None => (Vec::new(), true),
    };
    visiting.remove(&place);
    projected_cache.insert(place, result.clone());
    result
}

fn region_parent(regions: &[MemoryRegion], id: MemoryRegionId) -> Option<MemoryRegionId> {
    match regions.get(id.0 as usize)?.kind {
        MemoryRegionKind::Field { base, .. } | MemoryRegionKind::Elements { base } => Some(base),
        MemoryRegionKind::Binding { .. } | MemoryRegionKind::ParameterPointee { .. } => None,
    }
}

fn region_is_ancestor(
    regions: &[MemoryRegion],
    ancestor: MemoryRegionId,
    mut descendant: MemoryRegionId,
) -> bool {
    let mut remaining = regions.len();
    while remaining > 0 {
        remaining -= 1;
        let Some(base) = region_parent(regions, descendant) else {
            return false;
        };
        if base == ancestor {
            return true;
        }
        descendant = base;
    }
    false
}

fn union_branches(
    regions: &[MemoryRegion],
    mut region: MemoryRegionId,
) -> Vec<(MemoryRegionId, MemoryRegionId)> {
    let mut branches = Vec::new();
    let mut remaining = regions.len();
    while remaining > 0 {
        remaining -= 1;
        let Some(current) = regions.get(region.0 as usize) else {
            break;
        };
        if let MemoryRegionKind::Field {
            base,
            overlapping_members: true,
            ..
        } = current.kind
        {
            branches.push((base, region));
        }
        let Some(base) = region_parent(regions, region) else {
            break;
        };
        region = base;
    }
    branches
}

fn region_root_binding(regions: &[MemoryRegion], mut id: MemoryRegionId) -> Option<Binding> {
    let mut remaining = regions.len();
    while remaining > 0 {
        remaining -= 1;
        match regions.get(id.0 as usize)?.kind {
            MemoryRegionKind::Binding { binding } => return Some(binding),
            MemoryRegionKind::ParameterPointee { .. } => return None,
            MemoryRegionKind::Field { base, .. } | MemoryRegionKind::Elements { base } => id = base,
        }
    }
    None
}

fn seed_incoming_parameter_regions(flow: &mut DataFlow) {
    let regions = flow
        .memory_uses
        .iter()
        .map(|use_| use_.region)
        .collect::<BTreeSet<_>>();
    for region in regions {
        let abstract_parameter = match flow.memory_regions.get(region.0 as usize).map(|r| &r.kind) {
            Some(MemoryRegionKind::ParameterPointee { binding, .. }) => Some(*binding),
            Some(MemoryRegionKind::Field { .. }) | Some(MemoryRegionKind::Elements { .. }) => {
                let mut root = region;
                loop {
                    match flow.memory_regions.get(root.0 as usize).map(|r| &r.kind) {
                        Some(MemoryRegionKind::ParameterPointee { binding, .. }) => {
                            break Some(*binding)
                        }
                        Some(MemoryRegionKind::Field { base, .. })
                        | Some(MemoryRegionKind::Elements { base }) => root = *base,
                        _ => break None,
                    }
                }
            }
            _ => None,
        };
        let binding =
            abstract_parameter.or_else(|| region_root_binding(&flow.memory_regions, region));
        let Some(binding) = binding else { continue };
        let Some(parameter) = flow.definitions.iter().find(|definition| {
            definition.binding == binding && definition.kind == DefKind::Parameter
        }) else {
            continue;
        };
        if flow.memory_definitions.iter().any(|definition| {
            definition.region == region
                && definition.kind == MemoryDefinitionKind::IncomingParameter
        }) {
            continue;
        }
        flow.memory_definitions.push(MemoryDefinition {
            region,
            kind: MemoryDefinitionKind::IncomingParameter,
            precision: MemoryAccessPrecision::Exact,
            node: parameter.node,
            span: parameter.span,
            effect_at: 0,
        });
    }
}

/// Rebuild every structural may-overlap relation from the interned regions.
///
/// Region construction is not confined to the local projection pass: complete
/// callee summaries can materialize a previously unseen projected region in a
/// caller. Keeping overlap derivation here gives both construction paths the
/// same containment, union, and formal-alias semantics.
fn rebuild_memory_overlaps(flow: &mut DataFlow) {
    flow.memory_overlaps.clear();
    for left in 0..flow.memory_regions.len() {
        for right in left + 1..flow.memory_regions.len() {
            let left_id = MemoryRegionId(left as u32);
            let right_id = MemoryRegionId(right as u32);
            let relationship = if region_is_ancestor(&flow.memory_regions, left_id, right_id) {
                Some(MemoryRegionOverlap {
                    left: left_id,
                    right: right_id,
                    kind: MemoryOverlapKind::Containment,
                })
            } else if region_is_ancestor(&flow.memory_regions, right_id, left_id) {
                Some(MemoryRegionOverlap {
                    left: right_id,
                    right: left_id,
                    kind: MemoryOverlapKind::Containment,
                })
            } else {
                let left_branches = union_branches(&flow.memory_regions, left_id);
                let right_branches = union_branches(&flow.memory_regions, right_id);
                left_branches
                    .iter()
                    .any(|(left_base, left_child)| {
                        right_branches.iter().any(|(right_base, right_child)| {
                            left_base == right_base && left_child != right_child
                        })
                    })
                    .then_some(MemoryRegionOverlap {
                        left: left_id,
                        right: right_id,
                        kind: MemoryOverlapKind::UnionMembers,
                    })
                    .or_else(|| {
                        let root = |mut id: MemoryRegionId| loop {
                            match flow.memory_regions.get(id.0 as usize).map(|r| &r.kind) {
                                Some(MemoryRegionKind::Field { base, .. })
                                | Some(MemoryRegionKind::Elements { base }) => id = *base,
                                kind => break kind,
                            }
                        };
                        matches!(
                            (root(left_id), root(right_id)),
                            (
                                Some(MemoryRegionKind::ParameterPointee {
                                    parameter: left,
                                    ..
                                }),
                                Some(MemoryRegionKind::ParameterPointee {
                                    parameter: right,
                                    ..
                                })
                            ) if left != right
                        )
                        .then_some(MemoryRegionOverlap {
                            left: left_id,
                            right: right_id,
                            kind: MemoryOverlapKind::ParameterAlias,
                        })
                    })
            };
            if let Some(relationship) = relationship {
                flow.memory_overlaps.push(relationship);
            }
        }
    }
}

fn collect_memory_regions(
    evaluation: &EvaluationPlan,
    flow: &mut DataFlow,
    inputs: RegionInputs<'_>,
) -> Vec<Span> {
    let mut by_key = BTreeMap::new();
    let mut pointer_cache = BTreeMap::new();
    let mut projected_cache = BTreeMap::new();
    let mut unresolved = Vec::new();
    for operation in evaluation.operations() {
        let (kind, span, effect_at) = match operation.kind {
            TypeValueOp::LoadScalar { span, .. } => (MemoryAccessKind::Read, span, span.lo),
            TypeValueOp::StoreScalar { target, span, .. } => {
                (MemoryAccessKind::Write, target, span.hi)
            }
            _ => continue,
        };
        let Some(SemanticPlace::Projected(place)) = operation.place else {
            continue;
        };
        let context = RegionResolutionContext {
            evaluation,
            flow,
            binding_by_place: inputs.binding_by_place,
            targets: inputs.targets,
            unknown: inputs.unknown,
            definitely_initialized: inputs.definitely_initialized,
            uses_by_start: inputs.uses_by_start,
            parameter_origins: inputs.parameter_origins,
            parameter_bindings: inputs.parameter_bindings,
        };
        let (regions, incomplete) = projected_regions(
            place,
            &context,
            &mut BTreeSet::new(),
            &mut pointer_cache,
            &mut projected_cache,
        );
        if incomplete || regions.is_empty() {
            unresolved.push(span);
        }
        for (key, exact) in regions {
            let region = intern_region(&key, &mut by_key, &mut flow.memory_regions);
            flow.memory_accesses.push(MemoryAccess {
                region,
                kind,
                precision: if exact {
                    MemoryAccessPrecision::Exact
                } else {
                    MemoryAccessPrecision::MayAlias
                },
                node: operation.cfg_node,
                span,
                effect_at,
            });
            match kind {
                MemoryAccessKind::Read => flow.memory_uses.push(MemoryUse {
                    region,
                    precision: if exact {
                        MemoryAccessPrecision::Exact
                    } else {
                        MemoryAccessPrecision::MayAlias
                    },
                    node: operation.cfg_node,
                    span,
                }),
                MemoryAccessKind::Write => flow.memory_definitions.push(MemoryDefinition {
                    region,
                    kind: MemoryDefinitionKind::Store,
                    precision: if exact {
                        MemoryAccessPrecision::Exact
                    } else {
                        MemoryAccessPrecision::MayAlias
                    },
                    node: operation.cfg_node,
                    span,
                    effect_at,
                }),
            }
        }
    }
    rebuild_memory_overlaps(flow);
    seed_incoming_parameter_regions(flow);
    unresolved
}

fn project_call_clobbers(
    evaluation: &EvaluationPlan,
    flow: &mut DataFlow,
    inputs: RegionInputs<'_>,
) -> Vec<Span> {
    let accessed_regions = flow
        .memory_accesses
        .iter()
        .map(|access| access.region)
        .collect::<BTreeSet<_>>();
    let mut emitted_regions = BTreeSet::new();
    let mut emitted_scalars = BTreeSet::new();
    let mut incomplete = Vec::new();
    for call in evaluation.pointer_call_constraints() {
        let mut has_pointer_argument = false;
        for (argument, sources) in call.arguments {
            let pointer_like = sources.iter().any(|source| match *source {
                PointerValueSource::Address(_) => true,
                PointerValueSource::Copy { place, .. } => inputs
                    .binding_by_place
                    .get(&place)
                    .and_then(|binding| flow.types.get(binding.0 as usize))
                    .is_some_and(|ty| ty.pointer_depth > 0 || ty.array_rank > 0),
                PointerValueSource::Load { pointer, .. } => inputs
                    .binding_by_place
                    .get(&pointer)
                    .and_then(|binding| flow.types.get(binding.0 as usize))
                    .is_some_and(|ty| ty.pointer_depth > 1),
                PointerValueSource::Unknown => false,
            });
            if !pointer_like {
                continue;
            }
            has_pointer_argument = true;
            let (mut possible, _) = operation_pointer_targets(
                &sources,
                flow,
                inputs.binding_by_place,
                inputs.targets,
                inputs.unknown,
                inputs.definitely_initialized,
                inputs.uses_by_start,
            );
            let mut origins = BTreeSet::new();
            let mut sources_complete = true;
            for source in &sources {
                match *source {
                    PointerValueSource::Copy { place, .. } => {
                        let Some(&binding) = inputs.binding_by_place.get(&place) else {
                            sources_complete = false;
                            continue;
                        };
                        let inherited = inputs
                            .parameter_origins
                            .get(binding.0 as usize)
                            .cloned()
                            .unwrap_or_default();
                        if inherited.is_empty()
                            && inputs
                                .targets
                                .get(binding.0 as usize)
                                .is_none_or(BTreeSet::is_empty)
                        {
                            sources_complete = false;
                        }
                        origins.extend(inherited);
                    }
                    PointerValueSource::Address(_) => {}
                    PointerValueSource::Load { .. } | PointerValueSource::Unknown => {
                        sources_complete = false;
                    }
                }
            }
            for source in &sources {
                let PointerValueSource::Copy { place, .. } = *source else {
                    continue;
                };
                if let Some(&binding) = inputs.binding_by_place.get(&place).filter(|&&binding| {
                    flow.types
                        .get(binding.0 as usize)
                        .is_some_and(|ty| ty.array_rank > 0 && ty.pointer_depth == 0)
                }) {
                    possible.insert(binding);
                }
            }
            let has_origin = !origins.is_empty();
            flow.call_memory_arguments.push(CallMemoryArgument {
                call_span: call.span,
                node: call.node,
                argument,
                targets: possible.iter().copied().collect(),
                parameter_origins: origins.into_iter().collect(),
                complete: sources_complete && (!possible.is_empty() || has_origin),
            });
            for binding in possible {
                for &region in &accessed_regions {
                    if flow.memory_region_root_binding(region) == Some(binding)
                        && emitted_regions.insert((call.span, region))
                    {
                        flow.memory_definitions.push(MemoryDefinition {
                            region,
                            kind: MemoryDefinitionKind::CallClobber,
                            precision: MemoryAccessPrecision::MayAlias,
                            node: call.node,
                            span: call.span,
                            effect_at: call.span.hi,
                        });
                    }
                }
                let aggregate_object = flow.types.get(binding.0 as usize).is_some_and(|ty| {
                    ty.pointer_depth == 0
                        && (ty.specifiers.trim_start().starts_with("struct ")
                            || ty.specifiers.trim_start().starts_with("union "))
                });
                if !aggregate_object && emitted_scalars.insert((call.span, binding)) {
                    flow.definitions.push(Definition {
                        binding,
                        name: flow.names[binding.0 as usize].clone(),
                        node: call.node,
                        span: call.span,
                        effect_at: call.span.hi,
                        kind: DefKind::MemoryWrite,
                        declared: None,
                    });
                }
            }
        }
        if has_pointer_argument {
            // Without a callee effect summary, passing a pointer-like value
            // makes the absence of a write unknown even when its local target
            // is resolved exactly.
            incomplete.push(call.span);
        }
    }
    incomplete
}

pub(super) fn project_writes(context: ProjectionContext<'_>, flow: &mut DataFlow) {
    let ProjectionContext {
        tree,
        text,
        spans,
        function,
        evaluation,
        binding_by_place,
        unevaluated,
    } = context;
    let arena = tree.arena();
    let node_spans = super::events::NodeSpanIndex::new(&function.cfg);
    let mut targets = vec![BTreeSet::<Binding>::new(); flow.names.len()];
    let pointer = |binding: Binding| {
        flow.types
            .get(binding.0 as usize)
            .is_some_and(|ty| ty.pointer_depth > 0)
    };
    let mut copies = Vec::new();
    let mut load_copies = Vec::new();
    let mut unknown = BTreeSet::new();
    let mut issue_spans = Vec::new();
    // Initializer expressions usually cover one or two events. Index their
    // starts once instead of rescanning every definition and use for every
    // pointer declaration in a wide function.
    let mut definitions_by_start: Vec<usize> = (0..flow.definitions.len()).collect();
    definitions_by_start.sort_by_key(|&index| {
        let span = flow.definitions[index].span;
        (span.lo, span.hi, index)
    });
    let mut uses_by_start: Vec<usize> = (0..flow.uses.len()).collect();
    uses_by_start.sort_by_key(|&index| {
        let span = flow.uses[index].span;
        (span.lo, span.hi, index)
    });
    let definitely_initialized = definitely_initialized_uses(flow, &function.cfg);
    let addressed_dereferences = addressed_dereferences(tree, function.node);
    let control_dereferences = control_dereferences(tree, spans, function);
    let mut dereferenced_sources = Vec::new();
    let operation_loads = evaluation.pointer_load_constraints();
    let operation_load_spans = operation_loads
        .iter()
        .map(|load| load.span)
        .collect::<Vec<_>>();
    let modeled_load_spans = evaluation
        .operations()
        .iter()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::LoadScalar { span, .. } => Some(span),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut cast_nodes = Vec::new();
    let mut discarded_value_spans = Vec::new();
    for node in arena.preorder(function.node) {
        let Some(span) = arena.span(node, spans) else {
            continue;
        };
        let tag = arena.tag(node);
        if tag == Some(NodeTag::CastExpr.as_u16()) {
            cast_nodes.push(node);
        }
        if tag == Some(NodeTag::CondExpr.as_u16()) {
            if let Some(condition) = arena.children_iter(node).next() {
                if let Some(condition_span) = arena.span(condition, spans) {
                    discarded_value_spans.push(condition_span);
                }
            }
        } else if tag == Some(NodeTag::CommaExpr.as_u16()) {
            let mut children = arena.children_iter(node).peekable();
            while let Some(child) = children.next() {
                if children.peek().is_some() {
                    if let Some(child_span) = arena.span(child, spans) {
                        discarded_value_spans.push(child_span);
                    }
                }
            }
        }
        if super::events::is_direct_name(tree, node) {
            continue;
        }
        let access = strip_transparent_place(tree, node);
        let Some(access_span) = arena.span(access, spans) else {
            continue;
        };
        if operation_load_spans
            .iter()
            .any(|owned| contains(access_span, *owned) || contains(*owned, access_span))
        {
            continue;
        }
        if arena.tag(access) != Some(NodeTag::UnaryExpr.as_u16())
            || addressed_dereferences.contains(&node)
            || control_dereferences.contains(&access)
            || !text
                .get(access_span.lo as usize..access_span.hi as usize)
                .is_some_and(|source| source.trim_start().starts_with('*'))
        {
            continue;
        }
        let Some(child) = arena.children_iter(access).next() else {
            continue;
        };
        if !super::events::is_direct_name(tree, child) {
            continue;
        }
        let Some(child_span) = arena.span(child, spans) else {
            continue;
        };
        let first =
            uses_by_start.partition_point(|&index| flow.uses[index].span.lo < child_span.lo);
        let after =
            uses_by_start.partition_point(|&index| flow.uses[index].span.lo <= child_span.lo);
        if let Some((binding, use_index)) = uses_by_start[first..after].iter().find_map(|&index| {
            let read = &flow.uses[index];
            (read.span == child_span).then_some((read.binding, index))
        }) {
            dereferenced_sources.push((span, binding, use_index));
        }
    }
    dereferenced_sources
        .sort_by_key(|(span, binding, use_index)| (span.lo, span.hi, binding.0, *use_index));
    let discarded_values = super::regions::Regions::new(discarded_value_spans);
    let no_discarded_values = super::regions::Regions::new(Vec::new());
    let address_spans: BTreeSet<(u32, u32)> = flow
        .definitions
        .iter()
        .filter(|definition| definition.kind == DefKind::AddressTaken)
        .map(|definition| (definition.span.lo, definition.span.hi))
        .collect();
    let mut unsupported_pointer_casts = Vec::new();
    for node in cast_nodes {
        let mut children = arena.children_iter(node);
        let Some(type_name) = children.next() else {
            continue;
        };
        let Some(operand) = children.next() else {
            continue;
        };
        let (Some(type_span), Some(cast_span)) =
            (arena.span(type_name, spans), arena.span(node, spans))
        else {
            continue;
        };
        if !text
            .get(type_span.lo as usize..type_span.hi as usize)
            .is_some_and(|source| source.contains('*'))
        {
            continue;
        }
        // In a chain such as `(T *)(U *)(V *)p`, the innermost pointer cast
        // classifies the value source. Repeating the same range queries for
        // every enclosing cast would make a decompiler-style cast spine
        // quadratic without adding information.
        if arena.tag(operand) == Some(NodeTag::CastExpr.as_u16())
            && arena
                .children_iter(operand)
                .next()
                .and_then(|inner_type| arena.span(inner_type, spans))
                .and_then(|span| text.get(span.lo as usize..span.hi as usize))
                .is_some_and(|source| source.contains('*'))
        {
            continue;
        }

        if !cast_operand_is_known_pointer(
            tree,
            text,
            spans,
            operand,
            &uses_by_start,
            &flow.uses,
            &pointer,
        ) {
            unsupported_pointer_casts.push(cast_span);
        }
    }
    unsupported_pointer_casts.sort_by_key(|span| (span.lo, span.hi));
    let mut operation_pointer_writes = BTreeSet::new();
    for constraint in evaluation.pointer_value_constraints() {
        let Some(&destination) = binding_by_place.get(&constraint.destination) else {
            continue;
        };
        if !pointer(destination) {
            continue;
        }
        operation_pointer_writes.insert((destination, constraint.occurrence));
        for source in constraint.sources {
            match source {
                PointerValueSource::Address(place) => {
                    if let Some(&target) = binding_by_place.get(&place) {
                        targets[destination.0 as usize].insert(target);
                    } else {
                        unknown.insert(destination);
                    }
                }
                PointerValueSource::Copy { place, occurrence } => {
                    let Some(&source) = binding_by_place.get(&place).filter(|&&b| pointer(b))
                    else {
                        unknown.insert(destination);
                        continue;
                    };
                    copies.push((source, destination));
                    let initialized = exact_use_at(occurrence, source, &uses_by_start, &flow.uses)
                        .is_some_and(|index| definitely_initialized[index]);
                    if !initialized {
                        unknown.insert(destination);
                    }
                }
                PointerValueSource::Load {
                    pointer: place,
                    occurrence,
                } => {
                    let Some(&source) = binding_by_place.get(&place).filter(|&&b| pointer(b))
                    else {
                        unknown.insert(destination);
                        continue;
                    };
                    load_copies.push((source, destination));
                    let initialized = exact_use_at(occurrence, source, &uses_by_start, &flow.uses)
                        .is_some_and(|index| definitely_initialized[index]);
                    if !initialized {
                        unknown.insert(destination);
                    }
                }
                PointerValueSource::Unknown => {
                    unknown.insert(destination);
                }
            }
        }
    }
    for write in &flow.definitions {
        if !pointer(write.binding) || write.kind == DefKind::AddressTaken {
            continue;
        }
        if operation_pointer_writes.contains(&(write.binding, write.span)) {
            continue;
        }
        // A comma/condition operand discards its resulting value, not effects
        // performed while computing it. If this pointer write itself is in a
        // discarded region, its RHS still determines the value stored in the
        // pointer (for example `(p = &x, 0)`).
        let write_is_discarded = discarded_values.contains(write.span);
        let expression = Span {
            lo: write.span.lo,
            hi: write.effect_at,
        };
        if matches!(
            write.kind,
            DefKind::Parameter | DefKind::IncDec | DefKind::CompoundAssignment
        ) || flow
            .calls
            .iter()
            .any(|call| contains(expression, call.span))
            // Copying or casting a known pointer preserves its possible local
            // targets. Arithmetic does not: retaining the old target is a
            // useful may-alias edge, but cannot justify a complete result.
            || has_pointer_arithmetic(
                tree,
                expression,
                if write_is_discarded {
                    &no_discarded_values
                } else {
                    &discarded_values
                },
            )
            || unsupported_pointer_casts
                .iter()
                .any(|&cast| contains(expression, cast))
        {
            unknown.insert(write.binding);
        }
        if expression.lo <= expression.hi {
            let first_dereference =
                dereferenced_sources.partition_point(|(span, _, _)| span.lo < expression.lo);
            let after_dereferences =
                dereferenced_sources.partition_point(|(span, _, _)| span.lo <= expression.hi);
            let expression_dereferences =
                &dereferenced_sources[first_dereference..after_dereferences];
            for &(span, source, use_index) in expression_dereferences {
                if contains(expression, span)
                    && (write_is_discarded || !discarded_values.contains(span))
                {
                    load_copies.push((source, write.binding));
                    if !definitely_initialized[use_index] {
                        unknown.insert(write.binding);
                    }
                }
            }
            let first_definition = definitions_by_start
                .partition_point(|&index| flow.definitions[index].span.lo < expression.lo);
            let after_definitions = definitions_by_start
                .partition_point(|&index| flow.definitions[index].span.lo <= expression.hi);
            for &index in &definitions_by_start[first_definition..after_definitions] {
                let address = &flow.definitions[index];
                if address.kind == DefKind::AddressTaken
                    && contains(expression, address.span)
                    && (write_is_discarded || !discarded_values.contains(address.span))
                {
                    targets[write.binding.0 as usize].insert(address.binding);
                }
            }
            let first_use =
                uses_by_start.partition_point(|&index| flow.uses[index].span.lo < expression.lo);
            let after_uses =
                uses_by_start.partition_point(|&index| flow.uses[index].span.lo <= expression.hi);
            for &index in &uses_by_start[first_use..after_uses] {
                let read = &flow.uses[index];
                if !contains(expression, read.span) {
                    continue;
                }
                if pointer(read.binding)
                    && !address_spans.contains(&(read.span.lo, read.span.hi))
                    && (write_is_discarded || !discarded_values.contains(read.span))
                    && !expression_dereferences
                        .iter()
                        .any(|(span, _, _)| contains(*span, read.span))
                {
                    copies.push((read.binding, write.binding));
                    if !definitely_initialized[index] {
                        unknown.insert(write.binding);
                    }
                }
                if flow.unresolved_bindings.contains(&read.binding)
                    && !address_spans.contains(&(read.span.lo, read.span.hi))
                    && (write_is_discarded || !discarded_values.contains(read.span))
                {
                    unknown.insert(write.binding);
                }
            }
        }
    }
    propagate_targets(&mut targets, &mut unknown, &copies, &load_copies, &pointer);
    let mut parameter_origins = vec![BTreeSet::new(); flow.names.len()];
    let parameter_bindings = flow
        .definitions
        .iter()
        .filter(|definition| definition.kind == DefKind::Parameter)
        .map(|definition| definition.binding)
        .collect::<Vec<_>>();
    let mut parameter_index = 0u32;
    for definition in &flow.definitions {
        if definition.kind == DefKind::Parameter {
            if pointer(definition.binding) {
                parameter_origins[definition.binding.0 as usize].insert(parameter_index);
            }
            parameter_index += 1;
        }
    }
    loop {
        let mut changed = false;
        for &(source, destination) in &copies {
            let inherited = parameter_origins[source.0 as usize].clone();
            for origin in inherited {
                changed |= parameter_origins[destination.0 as usize].insert(origin);
            }
        }
        if !changed {
            break;
        }
    }
    let operation_stores = evaluation.pointer_store_constraints();
    let operation_store_spans = operation_stores
        .iter()
        .map(|store| store.span)
        .collect::<Vec<_>>();
    let operation_store_targets = operation_stores
        .iter()
        .map(|store| store.target)
        .collect::<Vec<_>>();
    let modeled_store_spans = evaluation
        .operations()
        .iter()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::StoreScalar { target, span, .. } => Some((target, span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    enum StoreAddress {
        Operation {
            address: Vec<PointerValueSource>,
            assigned: ValueId,
        },
        Legacy(usize),
    }
    let mut stores = Vec::new();
    for store in &operation_stores {
        stores.push((
            store.target,
            store.span,
            StoreAddress::Operation {
                address: store.address_sources.clone(),
                assigned: store.assigned,
            },
        ));
    }
    for node in arena.preorder(function.node) {
        if arena
            .span(node, spans)
            .is_some_and(|span| unevaluated.contains(span))
        {
            continue;
        }
        let place = if arena.tag(node) == Some(NodeTag::AssignExpr.as_u16()) {
            arena.children_iter(node).next()
        } else {
            inc_dec_place(tree, node)
        };
        let Some(place) = place else {
            continue;
        };
        if super::events::is_direct_name(tree, place) {
            continue;
        }
        let Some(place_span) = arena.span(place, spans) else {
            continue;
        };
        let dereference = strip_transparent_place(tree, place);
        let Some(dereference_span) = arena.span(dereference, spans) else {
            issue_spans.push(place_span);
            continue;
        };
        let Some(whole) = arena.span(node, spans) else {
            continue;
        };
        if operation_store_spans
            .iter()
            .any(|owned| contains(whole, *owned) || contains(*owned, whole))
            || modeled_store_spans.iter().any(|(target, span)| {
                contains(whole, *span)
                    || contains(*span, whole)
                    || contains(place_span, *target)
                    || contains(*target, place_span)
            })
        {
            continue;
        }
        // This increment resolves plain local pointer dereferences. Other
        // memory places remain explicitly incomplete, including fields/arrays.
        if arena.tag(dereference) != Some(NodeTag::UnaryExpr.as_u16())
            || !arena
                .children_iter(dereference)
                .next()
                .is_some_and(|child| super::events::is_direct_name(tree, child))
            || !text
                .get(dereference_span.lo as usize..dereference_span.hi as usize)
                .is_some_and(|s| s.trim_start().starts_with('*'))
        {
            issue_spans.push(place_span);
            continue;
        }
        let Some(base_index) = first_use_within(place_span, &uses_by_start, &flow.uses) else {
            issue_spans.push(place_span);
            continue;
        };
        stores.push((place_span, whole, StoreAddress::Legacy(base_index)));
    }
    for (place_span, whole, address) in stores {
        let (possible, assigned_sources) = match address {
            StoreAddress::Operation { address, assigned } => {
                let (possible, incomplete) = operation_pointer_targets(
                    &address,
                    flow,
                    binding_by_place,
                    &targets,
                    &unknown,
                    &definitely_initialized,
                    &uses_by_start,
                );
                if incomplete {
                    issue_spans.push(place_span);
                }
                (possible.into_iter().collect::<Vec<_>>(), Some(assigned))
            }
            StoreAddress::Legacy(base_index) => {
                let base = &flow.uses[base_index];
                if !definitely_initialized[base_index] {
                    issue_spans.push(place_span);
                }
                let Some(possible) = targets
                    .get(base.binding.0 as usize)
                    .filter(|set| !set.is_empty())
                    .map(|set| set.iter().copied().collect::<Vec<_>>())
                else {
                    issue_spans.push(place_span);
                    continue;
                };
                if unknown.contains(&base.binding) {
                    issue_spans.push(place_span);
                }
                (possible, None)
            }
        };
        if possible.is_empty() {
            continue;
        }
        let pointer_targets: Vec<Binding> = possible
            .iter()
            .copied()
            .filter(|&binding| pointer(binding))
            .collect();
        if !pointer_targets.is_empty() {
            if let Some(assigned) = assigned_sources {
                let assigned_sources = evaluation.pointer_sources(assigned);
                let (assigned, incomplete) = operation_pointer_targets(
                    &assigned_sources,
                    flow,
                    binding_by_place,
                    &targets,
                    &unknown,
                    &definitely_initialized,
                    &uses_by_start,
                );
                if incomplete {
                    issue_spans.push(place_span);
                }
                for target in pointer_targets {
                    targets[target.0 as usize].extend(assigned.iter().copied());
                    if incomplete {
                        unknown.insert(target);
                    }
                }
            } else {
                // Compatibility stores have no value identity. Preserve their
                // previous source-range may recovery and explicit uncertainty.
                issue_spans.push(place_span);
                let rhs = Span::new(place_span.hi, whole.hi);
                let first_definition = definitions_by_start
                    .partition_point(|&index| flow.definitions[index].span.lo < rhs.lo);
                let after_definitions = definitions_by_start
                    .partition_point(|&index| flow.definitions[index].span.lo < rhs.hi);
                let rhs_addresses: Vec<Binding> = definitions_by_start
                    [first_definition..after_definitions]
                    .iter()
                    .filter_map(|&index| {
                        let definition = &flow.definitions[index];
                        (definition.kind == DefKind::AddressTaken && contains(rhs, definition.span))
                            .then_some(definition.binding)
                    })
                    .collect();
                let first_use =
                    uses_by_start.partition_point(|&index| flow.uses[index].span.lo < rhs.lo);
                let after_uses =
                    uses_by_start.partition_point(|&index| flow.uses[index].span.lo < rhs.hi);
                let rhs_sources: Vec<Binding> = uses_by_start[first_use..after_uses]
                    .iter()
                    .filter_map(|&index| {
                        let read = &flow.uses[index];
                        (contains(rhs, read.span) && pointer(read.binding)).then_some(read.binding)
                    })
                    .collect();
                for target in pointer_targets {
                    unknown.insert(target);
                    targets[target.0 as usize].extend(rhs_addresses.iter().copied());
                    for source in &rhs_sources {
                        let inherited = targets[source.0 as usize].clone();
                        targets[target.0 as usize].extend(inherited);
                    }
                }
            }
            propagate_targets(&mut targets, &mut unknown, &copies, &load_copies, &pointer);
        }
        for binding in possible {
            flow.definitions.push(Definition {
                binding,
                name: flow.names[binding.0 as usize].clone(),
                node: node_spans.node_for(place_span),
                span: place_span,
                effect_at: whole.hi,
                kind: DefKind::MemoryWrite,
                declared: None,
            });
        }
    }
    let region_issues = collect_memory_regions(
        evaluation,
        flow,
        RegionInputs {
            binding_by_place,
            targets: &targets,
            unknown: &unknown,
            definitely_initialized: &definitely_initialized,
            uses_by_start: &uses_by_start,
            parameter_origins: &parameter_origins,
            parameter_bindings: &parameter_bindings,
        },
    );
    let resolved_projected_spans = flow
        .memory_accesses
        .iter()
        .map(|access| access.span)
        .filter(|span| !region_issues.contains(span))
        .collect::<BTreeSet<_>>();
    issue_spans.extend(region_issues);
    issue_spans.extend(project_call_clobbers(
        evaluation,
        flow,
        RegionInputs {
            binding_by_place,
            targets: &targets,
            unknown: &unknown,
            definitely_initialized: &definitely_initialized,
            uses_by_start: &uses_by_start,
            parameter_origins: &parameter_origins,
            parameter_bindings: &parameter_bindings,
        },
    ));
    // Project reads of a known local pointee as uses of that object. The
    // original pointer use remains: both address and stored value matter.
    let mut loads = Vec::new();
    for constraint in &operation_loads {
        let (possible, incomplete) = operation_pointer_targets(
            &constraint.sources,
            flow,
            binding_by_place,
            &targets,
            &unknown,
            &definitely_initialized,
            &uses_by_start,
        );
        if incomplete {
            issue_spans.push(constraint.span);
        }
        for binding in possible {
            loads.push(Use {
                binding,
                name: flow.names[binding.0 as usize].clone(),
                node: node_spans.node_for(constraint.span),
                span: constraint.span,
            });
        }
    }
    for node in arena.preorder(function.node) {
        if arena
            .span(node, spans)
            .is_some_and(|span| unevaluated.contains(span))
        {
            continue;
        }
        let Some(span) = arena.span(node, spans) else {
            continue;
        };
        if super::events::is_direct_name(tree, node) {
            continue;
        }
        let access = strip_transparent_place(tree, node);
        let Some(access_span) = arena.span(access, spans) else {
            issue_spans.push(span);
            continue;
        };
        if operation_load_spans
            .iter()
            .any(|owned| contains(access_span, *owned) || contains(*owned, access_span))
            || modeled_load_spans
                .iter()
                .any(|owned| contains(access_span, *owned) || contains(*owned, access_span))
            || operation_store_targets
                .iter()
                .any(|owned| contains(access_span, *owned) || contains(*owned, access_span))
        {
            continue;
        }
        if arena.tag(node) == Some(NodeTag::PostfixExpr.as_u16())
            && arena.children_iter(node).any(|child| {
                matches!(
                    arena.tag(child).and_then(NodeTag::from_u16),
                    Some(NodeTag::IndexSuffix | NodeTag::MemberSuffix)
                )
            })
        {
            issue_spans.push(span);
            continue;
        }
        if arena.tag(access) != Some(NodeTag::UnaryExpr.as_u16())
            || addressed_dereferences.contains(&node)
            || control_dereferences.contains(&access)
            || !text
                .get(access_span.lo as usize..access_span.hi as usize)
                .is_some_and(|s| s.trim_start().starts_with('*'))
        {
            continue;
        }
        if !arena
            .children_iter(access)
            .next()
            .is_some_and(|child| super::events::is_direct_name(tree, child))
        {
            issue_spans.push(access_span);
            continue;
        }
        let Some(base_index) = first_use_within(span, &uses_by_start, &flow.uses) else {
            issue_spans.push(access_span);
            continue;
        };
        let base = &flow.uses[base_index];
        if !definitely_initialized[base_index] {
            issue_spans.push(access_span);
        }
        let Some(possible) = targets
            .get(base.binding.0 as usize)
            .filter(|set| !set.is_empty())
        else {
            issue_spans.push(access_span);
            continue;
        };
        if unknown.contains(&base.binding) {
            issue_spans.push(access_span);
        }
        for &binding in possible {
            loads.push(Use {
                binding,
                name: flow.names[binding.0 as usize].clone(),
                node: node_spans.node_for(span),
                span,
            });
        }
    }
    flow.uses.extend(loads);
    for span in issue_spans {
        if resolved_projected_spans.contains(&span) {
            continue;
        }
        flow.record_issue(SemanticIssueKind::UnknownMemoryEffect, Some(span));
    }
}

fn parameter_binding(flow: &DataFlow, parameter: u32) -> Option<Binding> {
    flow.definitions
        .iter()
        .filter(|definition| definition.kind == DefKind::Parameter)
        .nth(parameter as usize)
        .map(|definition| definition.binding)
}

fn ensure_effect_region(
    flow: &mut DataFlow,
    root: RegionKey,
    path: &[MemoryEffectPath],
) -> MemoryRegionId {
    let mut key = root;
    for component in path {
        key = match component {
            MemoryEffectPath::Field(member) => {
                RegionKey::Field(Box::new(key), member.clone(), false)
            }
            MemoryEffectPath::Elements => RegionKey::Elements(Box::new(key)),
        };
    }
    let mut by_key = BTreeMap::new();
    for index in 0..flow.memory_regions.len() {
        fn key_of(regions: &[MemoryRegion], id: MemoryRegionId) -> Option<RegionKey> {
            Some(match &regions.get(id.0 as usize)?.kind {
                MemoryRegionKind::Binding { binding } => RegionKey::Binding(*binding),
                MemoryRegionKind::ParameterPointee { parameter, binding } => {
                    RegionKey::ParameterPointee(*parameter, *binding)
                }
                MemoryRegionKind::Field {
                    base,
                    member,
                    overlapping_members,
                } => RegionKey::Field(
                    Box::new(key_of(regions, *base)?),
                    member.clone(),
                    *overlapping_members,
                ),
                MemoryRegionKind::Elements { base } => {
                    RegionKey::Elements(Box::new(key_of(regions, *base)?))
                }
            })
        }
        let id = MemoryRegionId(index as u32);
        if let Some(existing) = key_of(&flow.memory_regions, id) {
            by_key.insert(existing, id);
        }
    }
    intern_region(&key, &mut by_key, &mut flow.memory_regions)
}

/// Replace generic pointer-call clobbers with complete known callee effects.
pub(super) fn refine_known_calls(
    flows: &mut [DataFlow],
    cfgs: &[FunctionCfg],
    summaries: &Summaries,
) {
    for (flow, function) in flows.iter_mut().zip(cfgs) {
        let mut refined = false;
        let calls = flow.calls.clone();
        for call in calls {
            let arguments = flow
                .call_memory_arguments
                .iter()
                .filter(|argument| argument.call_span == call.span)
                .cloned()
                .collect::<Vec<_>>();
            if arguments.is_empty() {
                continue;
            }
            if call
                .callee
                .as_deref()
                .is_some_and(|name| summaries.is_ambiguous(name))
            {
                continue;
            }
            let Some(summary) = call
                .callee
                .as_deref()
                .and_then(|name| summaries.lookup(name))
            else {
                continue;
            };
            if !summary.memory_effects_complete {
                continue;
            }
            refined = true;

            flow.memory_definitions.retain(|definition| {
                definition.span != call.span || definition.kind != MemoryDefinitionKind::CallClobber
            });
            let argument_targets = arguments
                .iter()
                .flat_map(|argument| argument.targets.iter().copied())
                .collect::<BTreeSet<_>>();
            flow.definitions.retain(|definition| {
                definition.span != call.span
                    || definition.kind != DefKind::MemoryWrite
                    || !argument_targets.contains(&definition.binding)
            });
            flow.semantic_issues.retain(|issue| {
                issue.kind != SemanticIssueKind::UnknownMemoryEffect
                    || issue.span != Some(call.span)
            });

            let mut emitted = BTreeSet::new();
            let mut emitted_scalar_writes = BTreeSet::new();
            for effect in &summary.memory_effects {
                for argument in arguments
                    .iter()
                    .filter(|argument| argument.argument == effect.parameter)
                {
                    let roots = argument
                        .targets
                        .iter()
                        .copied()
                        .map(RegionKey::Binding)
                        .chain(argument.parameter_origins.iter().filter_map(|&parameter| {
                            parameter_binding(flow, parameter)
                                .map(|binding| RegionKey::ParameterPointee(parameter, binding))
                        }))
                        .collect::<Vec<_>>();
                    for root in roots {
                        let region = ensure_effect_region(flow, root, &effect.path);
                        if !emitted.insert((region, effect.kind)) {
                            continue;
                        }
                        flow.memory_accesses.push(MemoryAccess {
                            region,
                            kind: match effect.kind {
                                ParameterMemoryEffectKind::Read => MemoryAccessKind::Read,
                                ParameterMemoryEffectKind::Write => MemoryAccessKind::Write,
                            },
                            precision: MemoryAccessPrecision::MayAlias,
                            node: argument.node,
                            span: call.span,
                            effect_at: call.span.hi,
                        });
                        match effect.kind {
                            ParameterMemoryEffectKind::Read => flow.memory_uses.push(MemoryUse {
                                region,
                                precision: MemoryAccessPrecision::MayAlias,
                                node: argument.node,
                                span: call.span,
                            }),
                            ParameterMemoryEffectKind::Write => {
                                flow.memory_definitions.push(MemoryDefinition {
                                    region,
                                    kind: MemoryDefinitionKind::CallEffect,
                                    precision: MemoryAccessPrecision::MayAlias,
                                    node: argument.node,
                                    span: call.span,
                                    effect_at: call.span.hi,
                                });
                                if effect.path.is_empty() {
                                    for &binding in &argument.targets {
                                        if flow.types.get(binding.0 as usize).is_some_and(|ty| {
                                            ty.pointer_depth == 0 && ty.array_rank == 0
                                        }) && emitted_scalar_writes.insert(binding)
                                        {
                                            flow.definitions.push(Definition {
                                                binding,
                                                name: flow.names[binding.0 as usize].clone(),
                                                node: argument.node,
                                                span: call.span,
                                                effect_at: call.span.hi,
                                                kind: DefKind::MemoryWrite,
                                                declared: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if !refined {
            continue;
        }
        rebuild_memory_overlaps(flow);
        seed_incoming_parameter_regions(flow);
        super::memory_solve::solve(flow, &function.cfg);
        super::solve::solve(flow, &function.cfg);
        flow.sync_compatibility_flags();
    }
}
