//! Flow-insensitive local points-to sets and weak indirect writes.
//!
//! These sets never justify killing a possible target. Pointer reassignment
//! unions targets; subsequent direct assignments still kill projected writes.
use std::collections::BTreeSet;

use super::{Binding, DataFlow, DefKind, Definition, Use};
use crate::csource::cfg::FunctionCfg;
use crate::csource::parse::{tag::NodeTag, Tree};
use crate::syntax::ids::Span;

fn contains(outer: Span, inner: Span) -> bool {
    outer.lo <= inner.lo && inner.hi <= outer.hi
}

pub(super) fn project_writes(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    function: &FunctionCfg,
    flow: &mut DataFlow,
) {
    let mut targets = vec![BTreeSet::<Binding>::new(); flow.names.len()];
    let pointer = |binding: Binding| {
        flow.types
            .get(binding.0 as usize)
            .is_some_and(|ty| ty.pointer_depth > 0)
    };
    let mut copies = Vec::new();
    let mut unknown = BTreeSet::new();
    for write in &flow.definitions {
        if !pointer(write.binding) || write.kind == DefKind::AddressTaken {
            continue;
        }
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
        {
            unknown.insert(write.binding);
        }
        for address in &flow.definitions {
            if address.kind == DefKind::AddressTaken && contains(expression, address.span) {
                targets[write.binding.0 as usize].insert(address.binding);
            }
        }
        for read in &flow.uses {
            if contains(expression, read.span) && pointer(read.binding) {
                copies.push((read.binding, write.binding));
            }
            if contains(expression, read.span)
                && flow.unresolved_bindings.contains(&read.binding)
                && !flow
                    .definitions
                    .iter()
                    .any(|d| d.kind == DefKind::AddressTaken && d.span == read.span)
            {
                unknown.insert(write.binding);
            }
        }
    }
    loop {
        let mut changed = false;
        for &(source, dest) in &copies {
            if unknown.contains(&source) {
                changed |= unknown.insert(dest);
            }
            let inherited = targets[source.0 as usize].clone();
            for target in inherited {
                changed |= targets[dest.0 as usize].insert(target);
            }
        }
        if !changed {
            break;
        }
    }
    let arena = tree.arena();
    for node in arena.preorder(function.node) {
        if arena.tag(node) != Some(NodeTag::AssignExpr.as_u16()) {
            continue;
        }
        let Some(place) = arena.children_iter(node).next() else {
            continue;
        };
        if super::events::is_direct_name(tree, place) {
            continue;
        }
        let Some(place_span) = arena.span(place, spans) else {
            continue;
        };
        let Some(whole) = arena.span(node, spans) else {
            continue;
        };
        // This increment resolves plain local pointer dereferences. Other
        // memory places remain explicitly incomplete, including fields/arrays.
        if arena.tag(place) != Some(NodeTag::UnaryExpr.as_u16())
            || !arena
                .children_iter(place)
                .next()
                .is_some_and(|child| super::events::is_direct_name(tree, child))
            || !text
                .get(place_span.lo as usize..place_span.hi as usize)
                .is_some_and(|s| s.trim_start().starts_with('*'))
        {
            flow.memory_complete = false;
            continue;
        }
        let Some(base) = flow.uses.iter().find(|u| contains(place_span, u.span)) else {
            flow.memory_complete = false;
            continue;
        };
        let Some(possible) = targets
            .get(base.binding.0 as usize)
            .filter(|set| !set.is_empty())
        else {
            flow.memory_complete = false;
            continue;
        };
        if unknown.contains(&base.binding) {
            flow.memory_complete = false;
        }
        for &binding in possible {
            flow.definitions.push(Definition {
                binding,
                name: flow.names[binding.0 as usize].clone(),
                node: super::events::node_for_span(&function.cfg, place_span),
                span: place_span,
                effect_at: whole.hi,
                kind: DefKind::MemoryWrite,
                declared: None,
            });
        }
    }
    // Project reads of a known local pointee as uses of that object. The
    // original pointer use remains: both address and stored value matter.
    let mut loads = Vec::new();
    for node in arena.preorder(function.node) {
        let Some(span) = arena.span(node, spans) else {
            continue;
        };
        if arena.tag(node) == Some(NodeTag::PostfixExpr.as_u16())
            && arena.children_iter(node).any(|child| {
                matches!(
                    arena.tag(child).and_then(NodeTag::from_u16),
                    Some(NodeTag::IndexSuffix | NodeTag::MemberSuffix)
                )
            })
        {
            flow.memory_complete = false;
        }
        if arena.tag(node) != Some(NodeTag::UnaryExpr.as_u16())
            || !text
                .get(span.lo as usize..span.hi as usize)
                .is_some_and(|s| s.trim_start().starts_with('*'))
        {
            continue;
        }
        if !arena
            .children_iter(node)
            .next()
            .is_some_and(|child| super::events::is_direct_name(tree, child))
        {
            flow.memory_complete = false;
            continue;
        }
        let Some(base) = flow.uses.iter().find(|u| contains(span, u.span)) else {
            flow.memory_complete = false;
            continue;
        };
        let Some(possible) = targets
            .get(base.binding.0 as usize)
            .filter(|set| !set.is_empty())
        else {
            flow.memory_complete = false;
            continue;
        };
        if unknown.contains(&base.binding) {
            flow.memory_complete = false;
        }
        for &binding in possible {
            loads.push(Use {
                binding,
                name: flow.names[binding.0 as usize].clone(),
                node: super::events::node_for_span(&function.cfg, span),
                span,
            });
        }
    }
    flow.uses.extend(loads);
}
