//! Reading definitions and uses out of the syntax tree.
//!
//! Everything here answers one question: which names in this function are
//! written, which are read, and which variable is each one about. The
//! fixpoint that consumes the answer is [`super::solve`]; the vocabulary it
//! is expressed in is [`super::model`].
//!
//! # Why so much of this is about *not* recording an event
//!
//! Most of the work below is deciding that a name is not a read. C spells
//! several things with a bare identifier that are not values --- a type in
//! `sizeof(T)`, a callee in `f(x)`, the base of `a[i] = v` --- and this parser
//! deliberately has no general typedef table (`parse/look.rs` says so and says
//! why). Each of those was a real over-report measured against the fixture
//! corpus. Most are fixed positionally; parameter recovery additionally keeps
//! the narrow typedef identity needed to distinguish types from value
//! bindings. File-scope names use one translation-unit index; block-scope
//! names share the ordinary identifier scope stack so shadowing stays lexical.

use std::collections::{BTreeMap, BTreeSet};

use crate::csource::cfg::FunctionCfg;
use crate::csource::eval::{
    EvaluationPlan, EvaluationPurpose, PlaceId, ScalarOperand, ScalarWriteKind, TypeValueOp,
};
use crate::csource::lex::kind::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::csource::semantic::declarations::{
    name_of, parameter_declarations, FunctionResolution, SymbolId, SymbolKind,
    TranslationUnitSymbols,
};
use crate::csource::semantic::types::FunctionTypes;
use crate::syntax::cfg::Cfg;
use crate::syntax::ids::{NodeId, Span};

use super::model::{
    Binding, CType, CallRecord, DefKind, Definition, SemanticIssue, SemanticIssueKind, Use,
};
use super::regions::Regions;
use super::types::declared_types;

/// The definitions and uses of one function, before the fixpoint.
pub(super) struct Events {
    pub(super) unresolved: Vec<Binding>,
    pub(super) definitions: Vec<Definition>,
    pub(super) uses: Vec<Use>,
    /// One entry per binding, in binding order.
    pub(super) types: Vec<CType>,
    /// One entry per binding, in binding order.
    pub(super) names: Vec<String>,
    /// Every call, in source order.
    pub(super) calls: Vec<CallRecord>,
    pub(super) unevaluated: Regions,
    /// Localized qualifications produced directly by event lowering.
    pub(super) semantic_issues: Vec<SemanticIssue>,
    pub(super) bound_captures: Vec<BoundCapture>,
    pub(super) binding_by_place: BTreeMap<PlaceId, Binding>,
}

/// One later use of a runtime bound captured at type-formation time.
pub(super) struct BoundCapture {
    pub(super) use_: Use,
    pub(super) source_expression: Span,
}

pub(super) struct SemanticContext<'a> {
    pub(super) resolution: &'a FunctionResolution,
    pub(super) types: &'a FunctionTypes,
    pub(super) evaluation: &'a EvaluationPlan,
    pub(super) symbols: &'a TranslationUnitSymbols,
}

/// Walk the function's syntax tree and record every write and read.
///
/// One pass in source order over the definition's subtree. The scope stack
/// opens on a `CompoundStmt` and on the constructs that introduce a scope of
/// their own (`for`, which may declare its own induction variable), and the
/// binding of a use is the innermost visible one at that point.
pub(super) fn collect_events(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    function: &FunctionCfg,
    semantic: SemanticContext<'_>,
) -> Events {
    let SemanticContext {
        resolution,
        types: structural_types,
        evaluation,
        symbols: typedefs,
    } = semantic;
    let arena = tree.arena();
    let mut definitions: Vec<Definition> = Vec::new();
    let mut uses: Vec<Use> = Vec::new();
    let mut bound_captures: Vec<BoundCapture> = Vec::new();
    let mut types: Vec<CType> = Vec::new();
    let mut binding_names: Vec<String> = Vec::new();
    let mut semantic_issues = structural_types
        .issues()
        .iter()
        .map(|issue| SemanticIssue {
            kind: SemanticIssueKind::UnknownType,
            span: Some(issue.span),
        })
        .collect::<Vec<_>>();
    semantic_issues.extend(evaluation.unsequenced_roots().iter().copied().map(|span| {
        SemanticIssue {
            kind: SemanticIssueKind::UnsequencedAccess,
            span: Some(span),
        }
    }));
    let mut variably_modified_bindings = BTreeSet::new();
    let mut binding_by_symbol: BTreeMap<SymbolId, Binding> = BTreeMap::new();

    macro_rules! incomplete_vla {
        ($span:expr) => {{
            let issue = SemanticIssue {
                kind: SemanticIssueKind::UnmodeledTypeValue,
                span: $span,
            };
            if !semantic_issues.contains(&issue) {
                semantic_issues.push(issue);
            }
        }};
    }

    // The CFG retains its definition's identity from the same syntax tree.
    // Re-enumerating every definition here cost a full-file scan per function.
    let root = function.node;
    // The declarator names the function itself. That is not a variable, and
    // binding it would make a recursive call look like a read of a local.
    let own_name_span = function.name_span;

    // Names that are not data reads, collected before the walk because both
    // tests need the *enclosing* node and the walk sees a node before it knows
    // what encloses it.
    //
    // A type name is the larger of the two: `int32_t x = 1;` puts `int32_t` in
    // the tree as a name, and counting it as a read of an undefined variable
    // was 516 of the 1,970 unresolved uses this analysis first reported over
    // the fixture corpus.
    let skip_spans = type_name_spans(tree, text, token_spans, root);
    let callee_spans = callee_name_spans(tree, text, token_spans, root);
    // Names in a position that is a type only when the name is not a variable.
    // Resolve simple sizeof operands before deciding whether they read a
    // value: known scalar/pointer sizes are independent of the stored value.
    let ambiguous_spans = evaluation.ambiguous_size_names();
    // `int x = 1;` writes. `int x;` does not: it binds a name and leaves it
    // holding whatever was on the stack. Counting the bare form as a store
    // made every uninitialized local a dead store --- 437 of the 897 this
    // analysis first reported --- and, worse, a read of one looked satisfied
    // rather than being the read-of-uninitialized it is.
    let initialized = initialized_declarator_spans(tree, text, token_spans, root);
    let function_offset = arena
        .span(root, token_spans)
        .map_or(u32::MAX, |span| span.lo);
    let parameter_declarations =
        parameter_declarations(tree, text, token_spans, root, typedefs, function_offset);
    let parameters = parameter_declarations.named();
    // The declared type of every name, keyed by the name's span. Parameters
    // and event bindings consume the same recovered declaration records.
    let declared = declared_types(tree, text, token_spans, root, parameters, structural_types);
    let type_at = |span: Span| -> CType {
        declared
            .iter()
            .find(|(candidate, _)| *candidate == span)
            .map(|(_, ty)| ty.clone())
            .unwrap_or_default()
    };

    let mut next_binding = 0u32;

    // Parameters first, because every one of them is a definition live at the
    // entry and a body that reads one must see it.
    //
    // The semantic declaration table owns parameter boundaries and binding
    // selection. Event lowering only consumes those records and adds the
    // entry definitions and evaluated array-bound effects.
    for &(group_start, group_end) in parameter_declarations.groups() {
        let Some(parameter) = parameters
            .iter()
            .find(|parameter| {
                parameter.group_start == group_start && parameter.group_end == group_end
            })
            .cloned()
        else {
            continue;
        };
        let (unevaluated, extent_complete) = unevaluated_extent_identifiers(
            tree,
            text,
            parameter.group_start,
            parameter.group_end,
            resolution,
        );
        if !extent_complete {
            incomplete_vla!(Some(parameter.span));
        }
        if extent_has_opaque_access(
            tree,
            text,
            parameter.group_start,
            parameter.group_end,
            resolution,
        ) {
            incomplete_vla!(Some(parameter.span));
        }
        // A parenthesis opened after the outer name is a nested function
        // signature, unless it is itself inside an active array bound.
        // Parentheses opened before the name merely group the outer
        // declarator (`int (a[n])`) and must not hide its suffix.
        let mut suppressed_parens = 0u32;
        let mut array_depth = 0u32;
        for index in parameter.group_start..parameter.group_end {
            let id = crate::syntax::ids::TokenId::new(index);
            let token = tree.tokens().text(id, text).trim();
            match token {
                "(" if index > parameter.name_index && array_depth == 0 => {
                    suppressed_parens = suppressed_parens.saturating_add(1);
                }
                ")" if suppressed_parens > 0 => suppressed_parens -= 1,
                "[" if array_depth > 0 || suppressed_parens == 0 => {
                    array_depth = array_depth.saturating_add(1)
                }
                "]" if array_depth > 0 => array_depth -= 1,
                _ if array_depth > 0
                    && tree.tokens().kind(id) == TokenKind::Identifier.as_u16() =>
                {
                    if unevaluated.contains(&index) {
                        continue;
                    }
                    let Some(extent_span) = token_spans.get(index as usize).copied() else {
                        continue;
                    };
                    let Some(extent_name) =
                        text.get(extent_span.lo as usize..extent_span.hi as usize)
                    else {
                        continue;
                    };
                    if extent_identifier_is_selector(tree, text, index, parameter.group_start) {
                        incomplete_vla!(Some(extent_span));
                        continue;
                    }
                    let resolved = resolution.resolve_at(extent_name, extent_span.lo);
                    if resolved.is_some_and(|symbol| symbol.kind != SymbolKind::Value)
                        || (resolved.is_none()
                            && (typedefs.typedef_is_visible(extent_name, function_offset)
                                || typedefs.constant_is_visible(extent_name, function_offset)))
                    {
                        continue;
                    }
                    let write_kind = extent_write_kind(tree, text, index, parameter.group_start);
                    match resolved.and_then(|symbol| binding_by_symbol.get(&symbol.id).copied()) {
                        Some(binding) => {
                            if let Some(kind) = write_kind {
                                definitions.push(Definition {
                                    binding,
                                    name: extent_name.to_string(),
                                    node: 0,
                                    span: extent_span,
                                    effect_at: token_spans
                                        .get(parameter.group_end.saturating_sub(1) as usize)
                                        .map_or(extent_span.hi, |span| span.hi),
                                    kind,
                                    declared: None,
                                });
                                // Comma sequencing and nested side effects are
                                // not fully reconstructed from this token run.
                                incomplete_vla!(Some(extent_span));
                            }
                            if write_kind != Some(DefKind::Assignment) {
                                uses.push(Use {
                                    binding,
                                    name: extent_name.to_string(),
                                    node: 0,
                                    span: extent_span,
                                });
                            }
                        }
                        None => incomplete_vla!(Some(extent_span)),
                    }
                }
                _ => {}
            }
        }
        let name = parameter.name;
        let span = parameter.span;
        let binding = Binding(next_binding);
        next_binding += 1;
        if let Some(declaration) = resolution.declaration_at(span) {
            binding_by_symbol.insert(declaration.id, binding);
        }
        let ty = type_at(span);
        types.push(ty.clone());
        binding_names.push(name.clone());
        definitions.push(Definition {
            binding,
            name,
            node: 0,
            span,
            // A parameter is written by the caller, so it is visible to every
            // read in the body including the first.
            effect_at: 0,
            kind: DefKind::Parameter,
            declared: (!ty.is_empty()).then_some(ty),
        });
    }

    // An explicit stack, never native recursion (`REQ-SYN-3`). Each entry is
    // a node to visit. Lexical identity is owned by `FunctionResolution`.
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        let tag = arena.tag(node).and_then(NodeTag::from_u16);

        match tag {
            // Grammar-owned enumerators enter the ordinary identifier
            // namespace at their declaration and denote constants, never
            // runtime storage.
            Some(NodeTag::DeclSpecifiers) => {
                if let Some((first, end)) = arena.token_extent(node) {
                    let belongs_to_typedef = (first..end).any(|index| {
                        tree.tokens().kind(crate::syntax::ids::TokenId::new(index))
                            == TokenKind::KwTypedef.as_u16()
                    });
                    for name in typeof_value_names(tree, text, first, end) {
                        let captured = resolution
                            .resolve_at(&name, token_spans[first as usize].lo)
                            .filter(|symbol| symbol.kind == SymbolKind::Value);
                        if captured
                            .and_then(|symbol| binding_by_symbol.get(&symbol.id))
                            .is_some_and(|binding| variably_modified_bindings.contains(binding))
                            && captured.is_none_or(|symbol| {
                                !structural_types.has_typeof_capture_of(symbol.span)
                            })
                        {
                            incomplete_vla!(arena.span(node, token_spans));
                        }
                    }
                    for (operand_start, operand_end) in typeof_vla_type_operands(
                        tree,
                        text,
                        first,
                        end,
                        resolution,
                        typedefs,
                        function_offset,
                    ) {
                        let (unevaluated, extent_complete) = unevaluated_extent_identifiers(
                            tree,
                            text,
                            operand_start,
                            operand_end,
                            resolution,
                        );
                        if !extent_complete {
                            incomplete_vla!(arena.span(node, token_spans));
                        }
                        if extent_has_opaque_access(
                            tree,
                            text,
                            operand_start,
                            operand_end,
                            resolution,
                        ) {
                            incomplete_vla!(arena.span(node, token_spans));
                        }
                        let mut array_depth = 0u32;
                        for index in operand_start..operand_end {
                            let id = crate::syntax::ids::TokenId::new(index);
                            match tree.tokens().text(id, text).trim() {
                                "[" => {
                                    array_depth += 1;
                                    continue;
                                }
                                "]" => {
                                    array_depth = array_depth.saturating_sub(1);
                                    continue;
                                }
                                _ => {}
                            }
                            if array_depth == 0
                                || tree.tokens().kind(id) != TokenKind::Identifier.as_u16()
                                || unevaluated.contains(&index)
                            {
                                continue;
                            }
                            let Some(span) = token_spans.get(index as usize).copied() else {
                                continue;
                            };
                            let Some(name) = text.get(span.lo as usize..span.hi as usize) else {
                                continue;
                            };
                            if extent_identifier_is_selector(tree, text, index, operand_start) {
                                incomplete_vla!(Some(span));
                                continue;
                            }
                            let resolved = resolution.resolve_at(name, span.lo);
                            if resolved.is_some_and(|symbol| symbol.kind != SymbolKind::Value)
                                || (resolved.is_none()
                                    && (typedefs.typedef_is_visible(name, function_offset)
                                        || typedefs.constant_is_visible(name, function_offset)))
                            {
                                continue;
                            }
                            let write_kind = extent_write_kind(tree, text, index, operand_start);
                            match resolved
                                .and_then(|symbol| binding_by_symbol.get(&symbol.id).copied())
                            {
                                Some(binding) => {
                                    if belongs_to_typedef {
                                        incomplete_vla!(Some(span));
                                    }
                                    if let Some(kind) = write_kind {
                                        definitions.push(Definition {
                                            binding,
                                            name: name.to_owned(),
                                            node: 0,
                                            span,
                                            effect_at: arena
                                                .span(node, token_spans)
                                                .map_or(span.hi, |whole| whole.hi),
                                            kind,
                                            declared: None,
                                        });
                                        incomplete_vla!(Some(span));
                                    }
                                    if write_kind != Some(DefKind::Assignment) {
                                        uses.push(Use {
                                            binding,
                                            name: name.to_owned(),
                                            node: 0,
                                            span,
                                        });
                                    }
                                }
                                None => incomplete_vla!(Some(span)),
                            }
                        }
                    }
                }
            }
            // `sizeof(type-name)` keeps the type name as an opaque token run.
            // A variably modified array bound is nevertheless evaluated (or,
            // when it cannot affect the result, may be evaluated), so recover
            // its value events just as for a declaration's `ArraySuffix`.
            Some(NodeTag::SizeofType) => {
                let Some(type_name) = arena
                    .children_iter(node)
                    .find(|child| arena.tag(*child) == Some(NodeTag::TypeName.as_u16()))
                else {
                    continue;
                };
                let Some((first, end)) = arena.token_extent(type_name) else {
                    continue;
                };
                let (unevaluated, extent_complete) =
                    unevaluated_extent_identifiers(tree, text, first, end, resolution);
                if !extent_complete {
                    incomplete_vla!(arena.span(node, token_spans));
                }
                if extent_has_opaque_access(tree, text, first, end, resolution) {
                    incomplete_vla!(arena.span(node, token_spans));
                }
                let mut array_depth = 0u32;
                for index in first..end {
                    let id = crate::syntax::ids::TokenId::new(index);
                    match tree.tokens().text(id, text).trim() {
                        "[" => {
                            array_depth = array_depth.saturating_add(1);
                            continue;
                        }
                        "]" => {
                            array_depth = array_depth.saturating_sub(1);
                            continue;
                        }
                        _ => {}
                    }
                    if array_depth == 0
                        || tree.tokens().kind(id) != TokenKind::Identifier.as_u16()
                        || unevaluated.contains(&index)
                    {
                        continue;
                    }
                    let Some(span) = token_spans.get(index as usize).copied() else {
                        continue;
                    };
                    let Some(name) = text.get(span.lo as usize..span.hi as usize) else {
                        continue;
                    };
                    if extent_identifier_is_selector(tree, text, index, first) {
                        incomplete_vla!(Some(span));
                        continue;
                    }
                    let resolved = resolution.resolve_at(name, span.lo);
                    if resolved.is_some_and(|symbol| symbol.kind != SymbolKind::Value)
                        || (resolved.is_none()
                            && (typedefs.typedef_is_visible(name, function_offset)
                                || typedefs.constant_is_visible(name, function_offset)))
                    {
                        continue;
                    }
                    let write_kind = extent_write_kind(tree, text, index, first);
                    match resolved.and_then(|symbol| binding_by_symbol.get(&symbol.id).copied()) {
                        Some(binding) => {
                            if let Some(kind) = write_kind {
                                definitions.push(Definition {
                                    binding,
                                    name: name.to_owned(),
                                    node: 0,
                                    span,
                                    effect_at: arena
                                        .span(node, token_spans)
                                        .map_or(span.hi, |whole| whole.hi),
                                    kind,
                                    declared: None,
                                });
                                // The standard leaves evaluation unspecified
                                // when changing this bound cannot affect the
                                // sizeof result, so a write cannot certify a
                                // complete negative result here.
                                incomplete_vla!(Some(span));
                            }
                            if write_kind != Some(DefKind::Assignment) {
                                uses.push(Use {
                                    binding,
                                    name: name.to_owned(),
                                    node: 0,
                                    span,
                                });
                            }
                        }
                        None => incomplete_vla!(Some(span)),
                    }
                }
            }
            // Array suffixes are opaque token runs, so VLA extents have no
            // `NameRef` child for the ordinary walk to see. Recover only
            // identifiers that already resolve to a lexical value. This
            // captures `int a[n]` without pretending an unknown macro or type
            // spelling is a local value.
            Some(NodeTag::ArraySuffix) => {
                if let Some((first, end)) = arena.token_extent(node) {
                    let owner = resolution.declaration_owning(
                        tree.tokens().start(crate::syntax::ids::TokenId::new(first)),
                    );
                    let array_binding = owner
                        .filter(|declaration| declaration.kind == SymbolKind::Value)
                        .and_then(|declaration| binding_by_symbol.get(&declaration.id).copied());
                    if owner.is_some_and(|declaration| {
                        !structural_types
                            .runtime_bound_slots_of_declaration(declaration.span)
                            .is_empty()
                    }) {
                        if let Some(binding) = array_binding {
                            variably_modified_bindings.insert(binding);
                        }
                    }
                    let suffix_span = arena.span(node, token_spans);
                    if suffix_span.is_some_and(|span| evaluation.models_formation_in(span)) {
                        let mut emitted_uses = BTreeSet::new();
                        let owners = suffix_span
                            .map(|span| evaluation.formation_evaluations_in(span))
                            .unwrap_or_default();
                        for operation in evaluation.operations().iter().filter(|operation| {
                            operation
                                .evaluation
                                .is_some_and(|owner| owners.contains(&owner))
                        }) {
                            let scalar_inputs = evaluation.direct_scalar_inputs_of(operation);
                            if !scalar_inputs.is_empty() {
                                for operand in scalar_inputs {
                                    let Some(declaration) = operand.declaration() else {
                                        continue;
                                    };
                                    let occurrence = operand.occurrence();
                                    let Some(binding) =
                                        resolution.declaration_at(declaration).and_then(|symbol| {
                                            binding_by_symbol.get(&symbol.id).copied()
                                        })
                                    else {
                                        incomplete_vla!(Some(occurrence));
                                        continue;
                                    };
                                    let Some(name) = binding_names.get(binding.0 as usize).cloned()
                                    else {
                                        incomplete_vla!(Some(occurrence));
                                        continue;
                                    };
                                    if emitted_uses.insert((
                                        binding,
                                        operation.cfg_node,
                                        occurrence.lo,
                                        occurrence.hi,
                                    )) {
                                        uses.push(Use {
                                            binding,
                                            name,
                                            node: operation.cfg_node,
                                            span: occurrence,
                                        });
                                    }
                                }
                                if !matches!(operation.kind, TypeValueOp::WriteScalar { .. }) {
                                    continue;
                                }
                            }
                            let (place, occurrence, write) = match &operation.kind {
                                TypeValueOp::ReadScalar {
                                    place, occurrence, ..
                                } => (*place, *occurrence, None),
                                TypeValueOp::WriteScalar {
                                    place,
                                    occurrence,
                                    kind,
                                    ..
                                } => (*place, *occurrence, Some(*kind)),
                                _ => continue,
                            };
                            if suffix_span.is_none_or(|span| {
                                occurrence.lo < span.lo || occurrence.hi > span.hi
                            }) {
                                continue;
                            }
                            let Some(binding) = binding_by_symbol.get(&SymbolId(place.0)).copied()
                            else {
                                incomplete_vla!(Some(occurrence));
                                continue;
                            };
                            let Some(name) = binding_names.get(binding.0 as usize).cloned() else {
                                incomplete_vla!(Some(occurrence));
                                continue;
                            };
                            if let Some(kind) = write {
                                definitions.push(Definition {
                                    binding,
                                    name,
                                    node: operation.cfg_node,
                                    span: occurrence,
                                    effect_at: suffix_span.map_or(occurrence.hi, |span| span.hi),
                                    kind: match kind {
                                        ScalarWriteKind::Assign => DefKind::Assignment,
                                        ScalarWriteKind::CompoundAssign => {
                                            DefKind::CompoundAssignment
                                        }
                                        ScalarWriteKind::Increment => DefKind::IncDec,
                                    },
                                    declared: None,
                                });
                            } else if emitted_uses.insert((
                                binding,
                                operation.cfg_node,
                                occurrence.lo,
                                occurrence.hi,
                            )) {
                                uses.push(Use {
                                    binding,
                                    name,
                                    node: operation.cfg_node,
                                    span: occurrence,
                                });
                            }
                        }
                        continue;
                    }
                    let (unevaluated, extent_complete) =
                        unevaluated_extent_identifiers(tree, text, first, end, resolution);
                    if !extent_complete {
                        incomplete_vla!(arena.span(node, token_spans));
                    }
                    if extent_has_opaque_access(tree, text, first, end, resolution) {
                        incomplete_vla!(arena.span(node, token_spans));
                        if let Some(binding) = array_binding {
                            variably_modified_bindings.insert(binding);
                        }
                    }
                    for index in first..end {
                        let id = crate::syntax::ids::TokenId::new(index);
                        if tree.tokens().kind(id) != TokenKind::Identifier.as_u16()
                            || unevaluated.contains(&index)
                        {
                            continue;
                        }
                        let Some(span) = token_spans.get(index as usize).copied() else {
                            continue;
                        };
                        let Some(name) = text.get(span.lo as usize..span.hi as usize) else {
                            continue;
                        };
                        if extent_identifier_is_selector(tree, text, index, first) {
                            incomplete_vla!(Some(span));
                            continue;
                        }
                        let resolved = resolution.resolve_at(name, span.lo);
                        if resolved.is_some_and(|symbol| symbol.kind != SymbolKind::Value)
                            || (resolved.is_none()
                                && (typedefs.typedef_is_visible(name, function_offset)
                                    || typedefs.constant_is_visible(name, function_offset)))
                        {
                            continue;
                        }
                        let write_kind = extent_write_kind(tree, text, index, first);
                        match resolved.and_then(|symbol| binding_by_symbol.get(&symbol.id).copied())
                        {
                            Some(binding) => {
                                if let Some(array_binding) = array_binding {
                                    variably_modified_bindings.insert(array_binding);
                                }
                                if let Some(kind) = write_kind {
                                    definitions.push(Definition {
                                        binding,
                                        name: name.to_string(),
                                        node: 0,
                                        span,
                                        effect_at: token_spans
                                            .get(end.saturating_sub(1) as usize)
                                            .map_or(span.hi, |suffix| suffix.hi),
                                        kind,
                                        declared: None,
                                    });
                                    incomplete_vla!(Some(span));
                                }
                                if write_kind != Some(DefKind::Assignment) {
                                    uses.push(Use {
                                        binding,
                                        name: name.to_string(),
                                        node: 0,
                                        span,
                                    });
                                }
                            }
                            None => {
                                // This may be a macro, enum constant, field,
                                // type name or callee. Without resolving that
                                // identity, the extent's value effects are not
                                // complete enough to certify a negative flow.
                                incomplete_vla!(Some(span));
                                if let Some(binding) = array_binding {
                                    variably_modified_bindings.insert(binding);
                                }
                            }
                        }
                    }
                }
            }
            // A declared name: bind it, and record the write.
            Some(NodeTag::DeclName) => {
                if let Some((name, span)) = name_of(tree, text, token_spans, node) {
                    if span == own_name_span {
                        // The function's own name; skip without binding it.
                        let children: Vec<NodeId> = arena.children_iter(node).collect();
                        for child in children.into_iter().rev() {
                            stack.push(child);
                        }
                        continue;
                    }
                    let declaration = resolution.declaration_at(span);
                    if declaration
                        .is_some_and(|declaration| declaration.kind == SymbolKind::Typedef)
                    {
                        continue;
                    }
                    let binding = Binding(next_binding);
                    next_binding += 1;
                    if let Some(declaration) = declaration {
                        binding_by_symbol.insert(declaration.id, binding);
                    }
                    if !structural_types
                        .runtime_bound_slots_of_declaration(span)
                        .is_empty()
                    {
                        variably_modified_bindings.insert(binding);
                    }
                    // The type is bound even when the declaration writes
                    // nothing: `int x;` has a type and no definition.
                    let ty = type_at(span);
                    debug_assert_eq!(types.len(), binding.0 as usize);
                    types.push(ty.clone());
                    binding_names.push(name.clone());
                    if initialized.contains(&span) {
                        definitions.push(Definition {
                            binding,
                            name,
                            node: 0, // assigned below, once spans are joined
                            span,
                            // Raised to the end of this declarator's initializer
                            // below. The binding is already in scope, but its
                            // initialized value is not available yet.
                            effect_at: span.hi,
                            kind: DefKind::Declaration,
                            declared: (!ty.is_empty()).then_some(ty.clone()),
                        });
                    }
                }
            }
            // A read of a name.
            Some(NodeTag::NameRef) => {
                if let Some((name, span)) = name_of(tree, text, token_spans, node) {
                    if skip_spans.contains(&span) {
                        let children: Vec<NodeId> = arena.children_iter(node).collect();
                        for child in children.into_iter().rev() {
                            stack.push(child);
                        }
                        continue;
                    }
                    let semantic = resolution.resolve_at(&name, span.lo);
                    if semantic.is_some_and(|declaration| declaration.kind == SymbolKind::Constant)
                        || (semantic.is_none()
                            && typedefs.constant_is_visible(&name, function_offset))
                    {
                        continue;
                    }
                    if ambiguous_spans.contains(&span)
                        && semantic
                            .is_some_and(|declaration| declaration.kind == SymbolKind::Typedef)
                    {
                        continue;
                    }
                    if ambiguous_spans.contains(&span)
                        && semantic.is_none()
                        && typedefs.typedef_is_visible(&name, function_offset)
                    {
                        continue;
                    }
                    let binding = semantic
                        .filter(|declaration| declaration.kind == SymbolKind::Value)
                        .and_then(|declaration| binding_by_symbol.get(&declaration.id).copied())
                        .unwrap_or(Binding::FREE);
                    let adjusted_pointer = semantic.is_some_and(|declaration| {
                        structural_types.adjusted_is_pointer(declaration.span)
                    });
                    let unevaluated_local = ambiguous_spans.contains(&span)
                        && (evaluation.reads_bound_at(span)
                            || types.get(binding.0 as usize).is_some_and(|ty| {
                                let fixed_written = ty.array_rank == 0
                                    && (ty.pointer_depth > 0
                                        || ty.specifiers.split_whitespace().all(|word| {
                                            matches!(
                                                word,
                                                "char"
                                                    | "short"
                                                    | "int"
                                                    | "long"
                                                    | "signed"
                                                    | "unsigned"
                                                    | "float"
                                                    | "double"
                                                    | "_Bool"
                                                    | "_Complex"
                                                    | "const"
                                                    | "volatile"
                                                    | "static"
                                                    | "extern"
                                                    | "register"
                                                    | "auto"
                                            )
                                        }));
                                !ty.is_empty() && (adjusted_pointer || fixed_written)
                            }));
                    // A name that resolves to nothing and is being *called* is
                    // a function, not a value: `memcpy(a, b, n)` reads a, b
                    // and n. A callee that does resolve is a function pointer,
                    // and reading it is a real dependence, so the test is on
                    // the binding rather than on the syntax alone.
                    if unevaluated_local
                        || (binding.is_free()
                            && (callee_spans.contains(&span) || ambiguous_spans.contains(&span)))
                    {
                        // fall through to the children walk below
                    } else {
                        uses.push(Use {
                            binding,
                            name,
                            node: 0,
                            span,
                        });
                    }
                }
            }
            _ => {}
        }

        // Children in source order: push reversed so the first pops first.
        let children: Vec<NodeId> = arena.children_iter(node).collect();
        for child in children.into_iter().rev() {
            stack.push(child);
        }
    }

    // Supported side-effect-free initializer and return roots now take their
    // reads from the common value graph. Remove only the exact source
    // occurrences that plan owns, then re-emit them with the operation's CFG
    // placement. Unsupported roots remain entirely on the syntax path.
    let ordinary_evaluations = evaluation
        .operations()
        .iter()
        .filter_map(|operation| {
            matches!(operation.kind, TypeValueOp::FinishExpression { .. })
                .then_some(operation.evaluation)
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    let mut modeled_use_nodes = BTreeMap::new();
    let mut modeled_write_nodes = BTreeMap::new();
    let mut modeled_occurrences = BTreeSet::new();
    let mut evaluated_uses = Vec::new();
    for operation in evaluation.operations().iter().filter(|operation| {
        operation
            .evaluation
            .is_some_and(|owner| ordinary_evaluations.contains(&owner))
    }) {
        modeled_occurrences.extend(
            evaluation
                .storage_identity_occurrences_of(operation)
                .into_iter()
                .map(|occurrence| (occurrence.lo, occurrence.hi)),
        );
        if let TypeValueOp::AddressOf {
            place,
            occurrence,
            span,
            ..
        } = &operation.kind
        {
            modeled_occurrences.insert((occurrence.lo, occurrence.hi));
            if let Some((binding, name)) = binding_by_symbol
                .get(&SymbolId(place.0))
                .copied()
                .and_then(|binding| {
                    binding_names
                        .get(binding.0 as usize)
                        .cloned()
                        .map(|name| (binding, name))
                })
            {
                definitions.push(Definition {
                    binding,
                    name,
                    node: operation.cfg_node,
                    span: *occurrence,
                    effect_at: span.hi,
                    kind: DefKind::AddressTaken,
                    declared: None,
                });
            }
        }
        if let TypeValueOp::WriteScalar {
            place,
            occurrence,
            span,
            kind,
            ..
        } = &operation.kind
        {
            modeled_occurrences.insert((occurrence.lo, occurrence.hi));
            modeled_write_nodes.insert(
                (occurrence.lo, occurrence.hi),
                (operation.cfg_node, span.hi),
            );
            if let Some((binding, name)) = binding_by_symbol
                .get(&SymbolId(place.0))
                .copied()
                .and_then(|binding| {
                    binding_names
                        .get(binding.0 as usize)
                        .cloned()
                        .map(|name| (binding, name))
                })
            {
                definitions.push(Definition {
                    binding,
                    name,
                    node: operation.cfg_node,
                    span: *occurrence,
                    effect_at: span.hi,
                    kind: match kind {
                        ScalarWriteKind::Assign => DefKind::Assignment,
                        ScalarWriteKind::CompoundAssign => DefKind::CompoundAssignment,
                        ScalarWriteKind::Increment => DefKind::IncDec,
                    },
                    declared: None,
                });
            }
        }
        for operand in evaluation.direct_scalar_inputs_of(operation) {
            let Some(place) = operand.place() else {
                continue;
            };
            let occurrence = operand.occurrence();
            let Some(binding) = binding_by_symbol.get(&SymbolId(place.0)).copied() else {
                continue;
            };
            let Some(name) = binding_names.get(binding.0 as usize).cloned() else {
                continue;
            };
            modeled_use_nodes.insert((occurrence.lo, occurrence.hi), operation.cfg_node);
            modeled_occurrences.insert((occurrence.lo, occurrence.hi));
            evaluated_uses.push(Use {
                binding,
                name,
                node: operation.cfg_node,
                span: occurrence,
            });
        }
    }
    uses.retain(|use_| !modeled_occurrences.contains(&(use_.span.lo, use_.span.hi)));
    uses.extend(evaluated_uses);
    uses.sort_by_key(|use_| (use_.span.lo, use_.span.hi));

    let initialized_by_evaluation = evaluation
        .operations()
        .iter()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::FinishExpression {
                purpose: EvaluationPurpose::Initialize { declaration, .. },
                expression,
                ..
            } => Some((declaration, (operation.cfg_node, expression.hi))),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

    for operation in evaluation.operations() {
        let TypeValueOp::ReadBound { slot, consumer } = &operation.kind else {
            continue;
        };
        let slot_id = *slot;
        let Some(slot) = structural_types.bound_slot(slot_id) else {
            incomplete_vla!(Some(*consumer));
            continue;
        };
        let Some(inputs) = evaluation.bound_inputs(slot_id) else {
            incomplete_vla!(Some(*consumer));
            continue;
        };
        for input in &inputs {
            let Some(binding) = resolution
                .declaration_at(*input)
                .and_then(|declaration| binding_by_symbol.get(&declaration.id).copied())
            else {
                incomplete_vla!(Some(*consumer));
                continue;
            };
            let Some(input_name) = binding_names.get(binding.0 as usize) else {
                incomplete_vla!(Some(*consumer));
                continue;
            };
            let capture = BoundCapture {
                use_: Use {
                    binding,
                    name: input_name.clone(),
                    node: operation.cfg_node,
                    span: *consumer,
                },
                source_expression: slot.expression,
            };
            if !bound_captures.iter().any(|existing| {
                existing.use_ == capture.use_
                    && existing.source_expression == capture.source_expression
            }) {
                bound_captures.push(capture);
            }
        }
    }

    // One token occurrence is one evaluation. Structured array bounds may be
    // visible both to the conservative bound adapter and to the ordinary
    // syntax walk when the scalar plan deliberately declines an ambiguous
    // construct. Collapse that overlap before write promotion, whose
    // span-index assumes this invariant.
    let mut seen_uses = BTreeSet::new();
    uses.retain(|use_| seen_uses.insert((use_.binding, use_.node, use_.span.lo, use_.span.hi)));

    // An assignment's left operand is a *write* as well as, for a compound
    // operator, a read. The walk above recorded it as a read, because
    // syntactically it is a `NameRef`; promote it here where the shape of the
    // enclosing expression is available.
    promote_writes(
        tree,
        text,
        token_spans,
        root,
        evaluation,
        &mut definitions,
        &mut uses,
    );
    // CFG construction deliberately omits statements after a terminator. Use
    // that executable-span evidence to keep syntactically present dead code
    // from contributing calls, memory uncertainty or value events.
    let node_spans = NodeSpanIndex::new(&function.cfg);
    let body_start = arena
        .children_iter(root)
        .find(|child| arena.tag(*child) == Some(NodeTag::CompoundStmt.as_u16()))
        .and_then(|body| arena.span(body, token_spans))
        .map_or(u32::MAX, |span| span.lo);
    let has_preprocessor_paths = arena
        .preorder(root)
        .any(|node| arena.tag(node) == Some(NodeTag::PpDirective.as_u16()));
    let mut unevaluated_spans =
        unevaluated_spans(tree, text, token_spans, root, &uses, &types, typedefs);
    if !has_preprocessor_paths {
        let initially_unevaluated = Regions::new(unevaluated_spans.clone());
        unevaluated_spans.extend(switch_dispatch_prefix_spans(
            tree,
            text,
            token_spans,
            root,
            &initially_unevaluated,
            &node_spans,
        ));
        let already_unevaluated = Regions::new(unevaluated_spans.clone());
        let structural_tails = non_fallthrough_tail_spans(
            tree,
            text,
            token_spans,
            root,
            &already_unevaluated,
            &node_spans,
            false,
        );
        unevaluated_spans.extend(structural_tails);
        let structurally_unevaluated = Regions::new(unevaluated_spans.clone());
        unevaluated_spans.extend(non_fallthrough_tail_spans(
            tree,
            text,
            token_spans,
            root,
            &structurally_unevaluated,
            &node_spans,
            true,
        ));
        unevaluated_spans.extend(arena.preorder(root).filter_map(|node| {
            let tag = arena.tag(node).and_then(NodeTag::from_u16)?;
            let event_statement = matches!(
                tag,
                NodeTag::ExprStmt
                    | NodeTag::ReturnStmt
                    | NodeTag::GotoStmt
                    | NodeTag::Asm
                    | NodeTag::IfStmt
                    | NodeTag::WhileStmt
                    | NodeTag::DoWhileStmt
                    | NodeTag::ForStmt
                    | NodeTag::SwitchStmt
            ) || (tag == NodeTag::Decl
                && arena
                    .children_iter(node)
                    .any(|child| arena.tag(child) == Some(NodeTag::Initializer.as_u16())));
            if !event_statement {
                return None;
            }
            let span = arena.span(node, token_spans)?;
            (span.lo >= body_start && !node_spans.has_span_within(span)).then_some(span)
        }));
    }
    let unevaluated = Regions::new(unevaluated_spans);
    // A modeled call still has unknown result/effects, but only when its CFG
    // region can execute. Literal short-circuit and conditional pruning has
    // already populated `unevaluated`; qualifying `1 || opaque(n)` before
    // this point would turn a language-proven dead call into false uncertainty.
    let bound_evaluations = evaluation
        .operations()
        .iter()
        .filter_map(|operation| {
            matches!(operation.kind, TypeValueOp::FormBound { .. })
                .then_some(operation.evaluation)
                .flatten()
        })
        .collect::<BTreeSet<_>>();
    for operation in evaluation.operations() {
        if let TypeValueOp::CallScalar { span, .. } = operation.kind {
            if operation
                .evaluation
                .is_none_or(|owner| !bound_evaluations.contains(&owner))
            {
                continue;
            }
            if unevaluated.contains(span) {
                continue;
            }
            for kind in [
                SemanticIssueKind::UnmodeledTypeValue,
                SemanticIssueKind::UnmodeledEffect,
            ] {
                let issue = SemanticIssue {
                    kind,
                    span: Some(span),
                };
                if !semantic_issues.contains(&issue) {
                    semantic_issues.push(issue);
                }
            }
        }
    }
    let effect_issues = arena
        .preorder(root)
        .filter_map(|node| {
            let opaque = match arena.tag(node).and_then(NodeTag::from_u16) {
                Some(NodeTag::Asm) => true,
                Some(NodeTag::Attribute) => attribute_has_cleanup(tree, text, node),
                Some(NodeTag::BuiltinExpr) => matches!(
                    arena
                        .main_token(node)
                        .and_then(|token| TokenKind::from_u16(tree.tokens().kind(token))),
                    Some(
                        TokenKind::KwGeneric
                            | TokenKind::KwBuiltinVaArg
                            | TokenKind::KwBuiltinChooseExpr
                    )
                ),
                _ => false,
            };
            let span = arena.span(node, token_spans);
            (opaque && span.is_none_or(|span| !unevaluated.contains(span))).then_some(
                SemanticIssue {
                    kind: SemanticIssueKind::UnmodeledEffect,
                    span,
                },
            )
        })
        .collect::<Vec<_>>();
    semantic_issues.extend(effect_issues);

    // Calls, collected after the scope walk so each argument can be resolved
    // to the binding it names.
    let mut calls = collect_calls(tree, text, token_spans, root, &uses, typedefs);
    let modeled_calls = evaluation.operations().iter().filter(|operation| {
        operation
            .evaluation
            .is_some_and(|owner| ordinary_evaluations.contains(&owner))
            && matches!(operation.kind, TypeValueOp::CallScalar { .. })
    });
    let modeled_call_spans = modeled_calls
        .clone()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::CallScalar { span, .. } => Some(span),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    calls.retain(|record| !modeled_call_spans.contains(&record.span));
    for operation in modeled_calls {
        let TypeValueOp::CallScalar {
            span,
            ref callee,
            ref arguments,
            ..
        } = operation.kind
        else {
            continue;
        };
        let returned = operation.evaluation.is_some_and(|owner| {
            evaluation.operations().iter().any(|finish| {
                finish.evaluation == Some(owner)
                    && matches!(
                        finish.kind,
                        TypeValueOp::FinishExpression {
                            purpose: EvaluationPurpose::Return { .. },
                            ..
                        }
                    )
                    && finish.inputs.as_slice() == [operation.output]
            })
        });
        let argument_spans = arguments
            .iter()
            .map(ScalarOperand::occurrence)
            .collect::<Vec<_>>();
        let arguments = argument_spans
            .iter()
            .map(|span| {
                uses.iter()
                    .find(|use_| use_.span == *span)
                    .map_or(Binding::FREE, |use_| use_.binding)
            })
            .collect();
        calls.push(CallRecord {
            callee: Some(callee.clone()),
            arguments,
            argument_spans,
            result_is_returned: returned,
            span,
        });
    }
    calls.sort_by_key(|record| (record.span.lo, record.span.hi));
    let evaluated = |span: Span| !unevaluated.contains(span);
    definitions.retain(|event| evaluated(event.span));
    uses.retain(|event| evaluated(event.span));
    calls.retain(|event| evaluated(event.span));

    // Each initialized value becomes available after its own initializer,
    // not after the whole comma-separated declaration.
    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::Decl.as_u16()) {
            continue;
        }
        let children: Vec<_> = arena.children_iter(node).collect();
        for (index, child) in children.iter().enumerate() {
            if arena.tag(*child) != Some(NodeTag::Declarator.as_u16()) {
                continue;
            }
            let Some(extent) = arena.span(*child, token_spans) else {
                continue;
            };
            let end = children
                .get(index + 1)
                .filter(|next| arena.tag(**next) == Some(NodeTag::Initializer.as_u16()))
                .and_then(|next| arena.span(*next, token_spans))
                .map_or(extent.hi, |init| init.hi);
            for definition in &mut definitions {
                if definition.kind == DefKind::Declaration
                    && extent.lo <= definition.span.lo
                    && definition.span.hi <= extent.hi
                {
                    definition.effect_at = end;
                }
            }
        }
    }

    // Join each event to the CFG node whose spans contain it. Build the span
    // index once: asking `node_for_span` separately made wide straight-line
    // functions scan every CFG node for every definition and use.
    for definition in &mut definitions {
        if let Some(&(node, effect_at)) =
            modeled_write_nodes.get(&(definition.span.lo, definition.span.hi))
        {
            definition.node = node;
            definition.effect_at = effect_at;
            continue;
        }
        if definition.kind == DefKind::Declaration {
            if let Some(&(node, effect_at)) = initialized_by_evaluation.get(&definition.span) {
                definition.node = node;
                definition.effect_at = effect_at;
                continue;
            }
        }
        definition.node = node_spans.node_for(definition.span);
    }
    for use_ in &mut uses {
        use_.node = modeled_use_nodes
            .get(&(use_.span.lo, use_.span.hi))
            .copied()
            .unwrap_or_else(|| node_spans.node_for(use_.span));
    }

    // Give each unresolved spelling a stable identity after lexical binding
    // and indirect-callee classification. Locals retain their original IDs.
    let unresolved_names: std::collections::BTreeSet<String> = definitions
        .iter()
        .filter(|d| d.binding.is_free())
        .map(|d| d.name.clone())
        .chain(
            uses.iter()
                .filter(|u| u.binding.is_free())
                .map(|u| u.name.clone()),
        )
        .collect();
    let mut unresolved = Vec::new();
    let mut unresolved_ids = std::collections::BTreeMap::new();
    for name in unresolved_names {
        let binding = Binding(binding_names.len() as u32);
        binding_names.push(name.clone());
        types.push(CType::default());
        unresolved.push(binding);
        unresolved_ids.insert(name, binding);
    }
    for definition in &mut definitions {
        if definition.binding.is_free() {
            definition.binding = unresolved_ids[&definition.name];
        }
    }
    for use_ in &mut uses {
        if use_.binding.is_free() {
            use_.binding = unresolved_ids[&use_.name];
        }
    }
    for call in &mut calls {
        for (argument, span) in call.arguments.iter_mut().zip(&call.argument_spans) {
            if let Some(use_) = uses.iter().find(|use_| use_.span == *span) {
                *argument = use_.binding;
            }
        }
    }
    let binding_by_place = binding_by_symbol
        .into_iter()
        .map(|(symbol, binding)| (PlaceId::from(symbol), binding))
        .collect();
    Events {
        unresolved,
        definitions,
        uses,
        types,
        names: binding_names,
        calls,
        unevaluated,
        semantic_issues,
        bound_captures,
        binding_by_place,
    }
}

/// Whether an opaque declaration attribute schedules an implicit cleanup.
///
/// Most attributes affect layout, diagnostics or linkage and introduce no
/// value event. GNU/Clang `cleanup(function)` is different: leaving the
/// variable's scope implicitly calls `function(&variable)`. Until cleanup
/// edges and scope-exit calls are modeled, their presence invalidates a clean
/// effect-completeness claim.
fn attribute_has_cleanup(tree: &Tree, text: &str, node: NodeId) -> bool {
    let Some((first, end)) = tree.arena().token_extent(node) else {
        return true;
    };
    (first..end).any(|index| {
        let token = crate::syntax::ids::TokenId::new(index);
        tree.tokens().kind(token) == TokenKind::Identifier.as_u16()
            && tree.tokens().text(token, text).trim().trim_matches('_') == "cleanup"
    })
}

/// Every call in the function, with each argument resolved to its binding.
///
/// Runs after the scope walk because an argument's binding is only known once
/// the walk has resolved the name at that offset. Matching by span is exact:
/// the walk recorded a use at the same offset the argument occupies.
fn collect_calls(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    uses: &[Use],
    file_symbols: &TranslationUnitSymbols,
) -> Vec<CallRecord> {
    let arena = tree.arena();
    let binding_at = |span: Span| -> Binding {
        uses.iter()
            .find(|use_| use_.span == span)
            .map(|use_| use_.binding)
            .unwrap_or(Binding::FREE)
    };

    let mut out: Vec<CallRecord> = Vec::new();
    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::PostfixExpr.as_u16()) {
            continue;
        }
        let children: Vec<NodeId> = arena.children_iter(node).collect();
        for (args_index, args) in children
            .iter()
            .enumerate()
            .filter(|(_, child)| arena.tag(**child) == Some(NodeTag::CallArgs.as_u16()))
        {
            // Only the first suffix in `name(...)` has a directly named
            // callee. A later call suffix in `factory()()` invokes the value
            // returned by the preceding chain and is therefore indirect.
            let has_prior_suffix = children[..args_index].iter().any(|child| {
                matches!(
                    arena.tag(*child).and_then(NodeTag::from_u16),
                    Some(
                        NodeTag::CallArgs
                            | NodeTag::IndexSuffix
                            | NodeTag::MemberSuffix
                            | NodeTag::IncDecSuffix
                    )
                )
            });
            let callee = (!has_prior_suffix)
                .then(|| children.first())
                .flatten()
                .and_then(|first| direct_callee_name(tree, text, token_spans, *first, file_symbols))
                // A visible local/parameter with this spelling is an indirect
                // call, even when a file-level function has the same name.
                .filter(|(_, span)| binding_at(*span).is_free())
                .map(|(name, _)| name);

            // One argument per child of the `CallArgs` node. A child that is a
            // bare name resolves to its binding; anything else is FREE.
            let arguments: Vec<Binding> = arena
                .children_iter(*args)
                .map(|child| {
                    if arena.tag(child) == Some(NodeTag::NameRef.as_u16()) {
                        name_of(tree, text, token_spans, child)
                            .map(|(_, span)| binding_at(span))
                            .unwrap_or(Binding::FREE)
                    } else {
                        Binding::FREE
                    }
                })
                .collect();

            let whole = arena.span(node, token_spans).unwrap_or_default();
            let span = Span {
                lo: whole.lo,
                hi: arena
                    .span(*args, token_spans)
                    .map_or(whole.hi, |span| span.hi),
            };
            out.push(CallRecord {
                callee,
                arguments,
                argument_spans: arena
                    .children_iter(*args)
                    .map(|child| arena.span(child, token_spans).unwrap_or_default())
                    .collect(),
                result_is_returned: returned_directly(tree, root, node, *args),
                span,
            });
        }
    }
    out
}

/// Resolve the syntactically direct callee of the first call suffix.
///
/// A plain name is unambiguous in call position. Wrapped names are not: without
/// a complete typedef table, `(T)(x)` may be either a cast or a call. Redundant
/// parentheses and C's transparent function-designator operations (`&f`,
/// `*f`) are therefore unwrapped only when the name matches a function
/// declaration in this translation unit. That recovers `(&helper)(x)` without
/// inventing an external call edge for an opaque typedef or pointer object.
fn direct_callee_name(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    mut node: NodeId,
    file_symbols: &TranslationUnitSymbols,
) -> Option<(String, Span)> {
    let arena = tree.arena();
    let mut wrapped = false;
    loop {
        let tag = arena.tag(node).and_then(NodeTag::from_u16);
        let transparent = match tag {
            Some(NodeTag::ParenExpr) => true,
            Some(NodeTag::UnaryExpr) => matches!(
                arena
                    .main_token(node)
                    .and_then(|token| TokenKind::from_u16(tree.tokens().kind(token))),
                Some(TokenKind::Amp | TokenKind::Star)
            ),
            _ => false,
        };
        if !transparent {
            break;
        }
        let mut expressions = arena.children_iter(node).filter(|child| {
            arena
                .tag(*child)
                .and_then(NodeTag::from_u16)
                .is_some_and(NodeTag::is_expression)
        });
        node = expressions.next()?;
        if expressions.next().is_some() {
            return None;
        }
        wrapped = true;
    }
    if arena.tag(node) != Some(NodeTag::NameRef.as_u16()) {
        return None;
    }
    let resolved = name_of(tree, text, token_spans, node)?;
    (!wrapped || file_symbols.declares_function(&resolved.0)).then_some(resolved)
}

/// Whether the expression at `span` is the operand of a `return`.
fn returned_directly(tree: &Tree, root: NodeId, call: NodeId, args: NodeId) -> bool {
    let arena = tree.arena();

    // A postfix chain is flat.  `g(x)[0]` and `g(x).field` therefore have the
    // same PostfixExpr owner as the call, but return a value derived from the
    // call rather than the call result itself.  The call arguments must be the
    // final suffix before the expression can be a direct return.
    let children: Vec<_> = arena.children_iter(call).collect();
    let Some(args_index) = children.iter().position(|child| *child == args) else {
        return false;
    };
    if children[args_index + 1..].iter().any(|child| {
        matches!(
            arena.tag(*child).and_then(NodeTag::from_u16),
            Some(
                NodeTag::CallArgs
                    | NodeTag::IndexSuffix
                    | NodeTag::MemberSuffix
                    | NodeTag::IncDecSuffix
            )
        )
    }) {
        return false;
    }

    arena.preorder(root).any(|node| {
        if arena.tag(node) != Some(NodeTag::ReturnStmt.as_u16()) {
            return false;
        }
        let Some(mut expression) = arena.children_iter(node).find(|child| {
            arena
                .tag(*child)
                .and_then(NodeTag::from_u16)
                .is_some_and(NodeTag::is_expression)
        }) else {
            return false;
        };

        // Parentheses do not transform a value.  No other expression wrapper
        // is transparent here: a cast, unary operator, binary expression,
        // conditional or comma expression consumes the call result.
        while arena.tag(expression) == Some(NodeTag::ParenExpr.as_u16()) {
            let mut nested = arena.children_iter(expression).filter(|child| {
                arena
                    .tag(*child)
                    .and_then(NodeTag::from_u16)
                    .is_some_and(NodeTag::is_expression)
            });
            let Some(inner) = nested.next() else {
                return false;
            };
            if nested.next().is_some() {
                return false;
            }
            expression = inner;
        }
        expression == call
    })
}

/// Turn the left operand of an assignment, and the operand of `++`/`--`, into
/// a definition.
///
/// Kept separate from the main walk because it needs the *enclosing*
/// expression's tag, and the walk visits a node before it knows what encloses
/// it. A compound assignment (`x += 1`) and an increment both read and write,
/// so the use stays and a definition is added; a plain `=` is a write only, so
/// the use is removed.
fn promote_writes(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    evaluation: &EvaluationPlan,
    definitions: &mut Vec<Definition>,
    uses: &mut Vec<Use>,
) {
    let arena = tree.arena();
    let mut promoted: Vec<(Span, DefKind, u32)> = Vec::new();
    let canceled_addresses: BTreeSet<NodeId> = arena
        .preorder(root)
        .filter_map(|node| dereference_of_address(tree, node).map(|(address, _)| address))
        .collect();
    let ordinary_place_effect_spans = evaluation.ordinary_place_effect_spans();

    for node in arena.preorder(root) {
        if arena.span(node, token_spans).is_some_and(|span| {
            evaluation.lowered_formation_contains(span)
                || ordinary_place_effect_spans
                    .iter()
                    .any(|write| span.lo <= write.lo && write.hi <= span.hi)
        }) {
            continue;
        }
        let tag = arena.tag(node).and_then(NodeTag::from_u16);
        match tag {
            Some(NodeTag::AssignExpr) => {
                // The first child is the target. A plain `=` writes only; every
                // compound operator reads it too.
                let Some(target) = arena.children_iter(node).next() else {
                    continue;
                };
                let compound = assign_is_compound(tree, text, node);
                // Only a *direct* name is a definition. `a[i] = v`, `s.f = v`
                // and `*p = v` all store into memory the base merely points
                // at, so the base is read, not written. Calling them
                // definitions would kill the real reaching definition of the
                // base --- unsound in the dangerous direction, and the exact
                // opposite of what this module claims to do.
                if !is_direct_name(tree, target) {
                    continue;
                }
                let effect_at = arena.span(node, token_spans).map_or(0, |whole| whole.hi);
                if let Some((_, span)) = leftmost_name(tree, text, token_spans, target) {
                    promoted.push((
                        span,
                        if compound {
                            DefKind::CompoundAssignment
                        } else {
                            DefKind::Assignment
                        },
                        effect_at,
                    ));
                }
            }
            // `i++` parses as a `PostfixExpr` holding a `NameRef` and an
            // `IncDecSuffix` side by side, so the operand is the suffix's
            // *sibling*. Reading the enclosing node is what finds it; reading
            // the suffix finds only the `++` token.
            // `&x` is retained as a definition-shaped event for the points-to
            // pass and for consumers interested in escaping storage. The
            // solver deliberately does not treat it as a value definition:
            // evaluating `&x` reads an address but does not store into `x`.
            Some(NodeTag::UnaryExpr)
                if !canceled_addresses.contains(&node) && takes_address(tree, text, node) =>
            {
                let effect_at = arena.span(node, token_spans).map_or(0, |whole| whole.hi);
                if let Some((_, span)) = leftmost_name(tree, text, token_spans, node) {
                    promoted.push((span, DefKind::AddressTaken, effect_at));
                }
            }
            Some(NodeTag::PostfixExpr) | Some(NodeTag::UnaryExpr) => {
                if !mentions_inc_dec(tree, text, node) {
                    continue;
                }
                let Some(place) = arena.children_iter(node).next() else {
                    continue;
                };
                // `++rank[ra]` and `(*p)++` update memory, not their base
                // names, for the same reason those places on the left of an
                // assignment do not define `rank` or `p`.
                if has_place_suffix(tree, node) || !is_direct_name(tree, place) {
                    continue;
                }
                let effect_at = arena.span(node, token_spans).map_or(0, |whole| whole.hi);
                if let Some((_, span)) = leftmost_name(tree, text, token_spans, node) {
                    promoted.push((span, DefKind::IncDec, effect_at));
                }
            }
            _ => {}
        }
    }

    // Promotion used to linearly search and remove from `uses` for every
    // write. Wide assignment sequences therefore shifted the remaining tail
    // repeatedly. Index the original spans and compact once after preserving
    // the promoted-definition order.
    let use_by_span: std::collections::BTreeMap<(u32, u32), usize> = uses
        .iter()
        .enumerate()
        .map(|(index, use_)| ((use_.span.lo, use_.span.hi), index))
        .collect();
    let mut remove = vec![false; uses.len()];
    for (span, kind, effect_at) in promoted {
        // The use recorded for this name is the one whose span matches, and it
        // already carries the binding the scope walk resolved.
        let Some(&index) = use_by_span.get(&(span.lo, span.hi)) else {
            continue;
        };
        let source = &uses[index];
        definitions.push(Definition {
            binding: source.binding,
            name: source.name.clone(),
            node: 0,
            span,
            effect_at,
            kind,
            // An assignment declares nothing; only the declaration site does.
            declared: None,
        });
        if matches!(kind, DefKind::Assignment | DefKind::AddressTaken) {
            remove[index] = true;
        }
    }
    let mut index = 0usize;
    uses.retain(|_| {
        let keep = !remove[index];
        index += 1;
        keep
    });
}

/// Whether an opaque bound hides a call or indirect memory access.
fn extent_has_opaque_access(
    tree: &Tree,
    text: &str,
    start: u32,
    end: u32,
    resolution: &FunctionResolution,
) -> bool {
    let tokens = tree.tokens();
    let spelling = |index| {
        tokens
            .text(crate::syntax::ids::TokenId::new(index), text)
            .trim()
    };
    let mut depth = 0u32;
    for index in start..end {
        let token = spelling(index);
        if token == "[" {
            if depth > 0 {
                let previous = (index > start).then(|| index - 1);
                if previous.is_some_and(|previous| {
                    let id = crate::syntax::ids::TokenId::new(previous);
                    if tokens.kind(id) == TokenKind::Identifier.as_u16() {
                        resolution
                            .resolve_at(spelling(previous), tokens.start(id))
                            .is_some_and(|symbol| symbol.kind == SymbolKind::Value)
                    } else {
                        matches!(spelling(previous), ")" | "]")
                    }
                }) {
                    return true;
                }
            }
            depth += 1;
            continue;
        }
        if token == "]" {
            depth = depth.saturating_sub(1);
            continue;
        }
        if depth == 0 {
            continue;
        }
        if matches!(token, "." | "->") {
            return true;
        }
        if tokens.kind(crate::syntax::ids::TokenId::new(index)) == TokenKind::Identifier.as_u16()
            && index + 1 < end
            && spelling(index + 1) == "("
        {
            return true;
        }
        if token == "*" {
            let previous = (index > start).then(|| spelling(index - 1));
            if previous.is_none_or(|previous| {
                matches!(
                    previous,
                    "[" | "(" | "," | "?" | ":" | "=" | "+" | "-" | "!" | "~"
                )
            }) {
                return true;
            }
        }
    }
    false
}

/// Whether an identifier in an opaque bound is a field selector, not a value.
fn extent_identifier_is_selector(tree: &Tree, text: &str, index: u32, start: u32) -> bool {
    index > start
        && matches!(
            tree.tokens()
                .text(crate::syntax::ids::TokenId::new(index - 1), text)
                .trim(),
            "." | "->"
        )
}

/// A direct write spelled around one identifier in an opaque array bound.
fn extent_write_kind(tree: &Tree, text: &str, index: u32, range_start: u32) -> Option<DefKind> {
    let tokens = tree.tokens();
    let spelling = |at| {
        tokens
            .text(crate::syntax::ids::TokenId::new(at), text)
            .trim()
    };
    let previous = (index > range_start).then(|| spelling(index - 1));
    let next = spelling(index + 1);
    if matches!(previous, Some("." | "->" | "*" | "&")) {
        return None;
    }
    if matches!(previous, Some("++" | "--")) || matches!(next, "++" | "--") {
        return Some(DefKind::IncDec);
    }
    match next {
        "=" => Some(DefKind::Assignment),
        "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "^=" | "|=" => {
            Some(DefKind::CompoundAssignment)
        }
        _ => None,
    }
}

/// Direct value operands of top-level `typeof` specifiers.
///
/// This is deliberately a narrow compatibility check for the pre-semantic
/// representation: declaration specifiers are still opaque token runs here.
/// It recognizes only an identifier wrapped in any number of redundant
/// parentheses. P3/P4 replace it with resolved expression/type identities and
/// captured bound slots.
fn typeof_value_names(tree: &Tree, text: &str, start: u32, end: u32) -> Vec<String> {
    let tokens = tree.tokens();
    let spelling = |index| {
        tokens
            .text(crate::syntax::ids::TokenId::new(index), text)
            .trim()
    };
    let mut names = Vec::new();
    let mut index = start;
    let mut depth = 0u32;
    while index < end {
        let kind = TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(index)));
        if depth == 0 && kind == Some(TokenKind::KwTypeof) && index + 1 < end {
            let open = index + 1;
            if spelling(open) == "(" {
                let mut nested = 1u32;
                let mut close = open + 1;
                while close < end && nested > 0 {
                    match spelling(close) {
                        "(" => nested += 1,
                        ")" => nested -= 1,
                        _ => {}
                    }
                    close += 1;
                }
                if nested == 0 {
                    let mut lo = open + 1;
                    let mut hi = close - 1;
                    loop {
                        if hi.saturating_sub(lo) < 3
                            || spelling(lo) != "("
                            || spelling(hi - 1) != ")"
                        {
                            break;
                        }
                        let mut inner_depth = 0u32;
                        let mut matching = None;
                        for candidate in lo..hi {
                            match spelling(candidate) {
                                "(" => inner_depth += 1,
                                ")" => {
                                    inner_depth = inner_depth.saturating_sub(1);
                                    if inner_depth == 0 {
                                        matching = Some(candidate);
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        if matching != Some(hi - 1) {
                            break;
                        }
                        lo += 1;
                        hi -= 1;
                    }
                    if hi == lo + 1
                        && TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(lo)))
                            == Some(TokenKind::Identifier)
                    {
                        names.push(spelling(lo).to_owned());
                    }
                    index = close;
                    continue;
                }
            }
        }
        match spelling(index) {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    names
}

/// Top-level `typeof(type-name)` operands whose type is variably modified.
///
/// GNU C leaves an ordinary expression operand unevaluated, but constructing a
/// variably modified type evaluates its array bounds. Declaration specifiers
/// are opaque token runs, so select only a top-level `typeof`, require a
/// type-looking first token, and hand its complete operand to the same bounded
/// array recovery used by ordinary declarators.
fn typeof_vla_type_operands(
    tree: &Tree,
    text: &str,
    start: u32,
    end: u32,
    resolution: &FunctionResolution,
    file_symbols: &TranslationUnitSymbols,
    function_offset: u32,
) -> Vec<(u32, u32)> {
    let tokens = tree.tokens();
    let spelling = |index| {
        tokens
            .text(crate::syntax::ids::TokenId::new(index), text)
            .trim()
    };
    let mut out = Vec::new();
    let mut depth = 0u32;
    let mut index = start;
    while index < end {
        let kind = TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(index)));
        if depth == 0 && kind == Some(TokenKind::KwTypeof) && index + 1 < end {
            let open = index + 1;
            if spelling(open) == "(" {
                let mut nested = 1u32;
                let mut close = open + 1;
                while close < end && nested > 0 {
                    match spelling(close) {
                        "(" => nested += 1,
                        ")" => nested -= 1,
                        _ => {}
                    }
                    close += 1;
                }
                if nested == 0 {
                    let operand_start = open + 1;
                    let operand_end = close - 1;
                    let mut first = operand_start;
                    while first < operand_end
                        && matches!(
                            TokenKind::from_u16(
                                tokens.kind(crate::syntax::ids::TokenId::new(first))
                            ),
                            Some(
                                TokenKind::KwConst
                                    | TokenKind::KwRestrict
                                    | TokenKind::KwVolatile
                                    | TokenKind::KwAtomic
                            )
                        )
                    {
                        first += 1;
                    }
                    let first_kind = (first < operand_end).then(|| {
                        TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(first)))
                    });
                    let first_name = (first < operand_end
                        && first_kind.flatten() == Some(TokenKind::Identifier))
                    .then(|| spelling(first));
                    let type_like = matches!(
                        first_kind.flatten(),
                        Some(
                            TokenKind::KwBool
                                | TokenKind::KwChar
                                | TokenKind::KwComplex
                                | TokenKind::KwDouble
                                | TokenKind::KwEnum
                                | TokenKind::KwFloat
                                | TokenKind::KwInt
                                | TokenKind::KwInt128
                                | TokenKind::KwLong
                                | TokenKind::KwShort
                                | TokenKind::KwSigned
                                | TokenKind::KwStruct
                                | TokenKind::KwUnion
                                | TokenKind::KwUnsigned
                                | TokenKind::KwVoid
                                | TokenKind::KwTypeof
                        )
                    ) || first_name.is_some_and(|name| {
                        resolution
                            .resolve_at(name, tokens.start(crate::syntax::ids::TokenId::new(first)))
                            .is_some_and(|symbol| symbol.kind == SymbolKind::Typedef)
                            || file_symbols.typedef_is_visible(name, function_offset)
                    });
                    let has_array =
                        (operand_start..operand_end).any(|candidate| spelling(candidate) == "[");
                    if type_like && has_array {
                        out.push((operand_start, operand_end));
                    } else {
                        // An expression operand can still contain a cast to a
                        // variably modified type. Determining that cast's type
                        // evaluates its bound even though the surrounding
                        // `typeof` does not evaluate the expression value.
                        let mut candidate = operand_start;
                        while candidate < operand_end {
                            if spelling(candidate) != "(" {
                                if matches!(spelling(candidate), "*" | "&" | "+" | "-" | "!" | "~")
                                {
                                    candidate += 1;
                                    continue;
                                }
                                break;
                            }
                            let previous_kind = (candidate > operand_start).then(|| {
                                TokenKind::from_u16(
                                    tokens.kind(crate::syntax::ids::TokenId::new(candidate - 1)),
                                )
                            });
                            let mut cast_depth = 1u32;
                            let mut cast_close = candidate + 1;
                            while cast_close < operand_end && cast_depth > 0 {
                                match spelling(cast_close) {
                                    "(" => cast_depth += 1,
                                    ")" => cast_depth -= 1,
                                    _ => {}
                                }
                                cast_close += 1;
                            }
                            if cast_depth != 0 {
                                break;
                            }
                            let cast_end = cast_close - 1;
                            if matches!(
                                previous_kind.flatten(),
                                Some(
                                    TokenKind::KwSizeof
                                        | TokenKind::KwAlignof
                                        | TokenKind::KwTypeof
                                        | TokenKind::KwGeneric
                                )
                            ) {
                                candidate = cast_close;
                                continue;
                            }
                            let mut cast_first = candidate + 1;
                            while cast_first < cast_end
                                && matches!(
                                    TokenKind::from_u16(
                                        tokens.kind(crate::syntax::ids::TokenId::new(cast_first))
                                    ),
                                    Some(
                                        TokenKind::KwConst
                                            | TokenKind::KwRestrict
                                            | TokenKind::KwVolatile
                                            | TokenKind::KwAtomic
                                    )
                                )
                            {
                                cast_first += 1;
                            }
                            let cast_kind = (cast_first < cast_end).then(|| {
                                TokenKind::from_u16(
                                    tokens.kind(crate::syntax::ids::TokenId::new(cast_first)),
                                )
                            });
                            let cast_name = (cast_first < cast_end
                                && cast_kind.flatten() == Some(TokenKind::Identifier))
                            .then(|| spelling(cast_first));
                            let cast_type_like = matches!(
                                cast_kind.flatten(),
                                Some(
                                    TokenKind::KwBool
                                        | TokenKind::KwChar
                                        | TokenKind::KwComplex
                                        | TokenKind::KwDouble
                                        | TokenKind::KwEnum
                                        | TokenKind::KwFloat
                                        | TokenKind::KwInt
                                        | TokenKind::KwInt128
                                        | TokenKind::KwLong
                                        | TokenKind::KwShort
                                        | TokenKind::KwSigned
                                        | TokenKind::KwStruct
                                        | TokenKind::KwUnion
                                        | TokenKind::KwUnsigned
                                        | TokenKind::KwVoid
                                        | TokenKind::KwTypeof
                                )
                            ) || cast_name.is_some_and(|name| {
                                resolution
                                    .resolve_at(
                                        name,
                                        tokens.start(crate::syntax::ids::TokenId::new(cast_first)),
                                    )
                                    .is_some_and(|symbol| symbol.kind == SymbolKind::Typedef)
                                    || file_symbols.typedef_is_visible(name, function_offset)
                            });
                            if cast_type_like
                                && ((candidate + 1)..cast_end).any(|at| spelling(at) == "[")
                            {
                                out.push((candidate + 1, cast_end));
                                candidate = cast_close;
                            } else {
                                candidate += 1;
                            }
                        }
                    }
                    index = close;
                    continue;
                }
            }
        }
        match spelling(index) {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            _ => {}
        }
        index += 1;
    }
    out
}

/// Identifier tokens inside array bounds that C does not evaluate.
///
/// Array suffixes are opaque token runs, so the syntax tree cannot supply its
/// normal `sizeof` regions here. Parenthesised expression operands are wholly
/// unevaluated. A type operand containing an array suffix is the exception:
/// `sizeof(int[n])` evaluates `n`, while `sizeof(n)` does not.
fn unevaluated_extent_identifiers(
    tree: &Tree,
    text: &str,
    start: u32,
    end: u32,
    resolution: &FunctionResolution,
) -> (BTreeSet<u32>, bool) {
    let tokens = tree.tokens();
    let mut out = BTreeSet::new();
    let mut complete = true;
    let mark_identifiers = |out: &mut BTreeSet<u32>, lo: u32, hi: u32| {
        for index in lo..hi {
            if tokens.kind(crate::syntax::ids::TokenId::new(index))
                == TokenKind::Identifier.as_u16()
            {
                out.insert(index);
            }
        }
    };

    for operator in start..end {
        let operator_kind =
            TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(operator)));
        if !matches!(
            operator_kind,
            Some(TokenKind::KwSizeof | TokenKind::KwAlignof | TokenKind::KwGeneric)
        ) {
            continue;
        }
        let next = operator + 1;
        if next >= end
            || tokens
                .text(crate::syntax::ids::TokenId::new(next), text)
                .trim()
                != "("
        {
            // Cover the unambiguous `sizeof name` case. More complex unary
            // operands stay conservative rather than skipping too much.
            if next < end
                && tokens.kind(crate::syntax::ids::TokenId::new(next))
                    == TokenKind::Identifier.as_u16()
            {
                out.insert(next);
            }
            if operator_kind == Some(TokenKind::KwGeneric) {
                complete = false;
            }
            continue;
        }

        let mut depth = 1u32;
        let mut close = next + 1;
        while close < end && depth > 0 {
            match tokens
                .text(crate::syntax::ids::TokenId::new(close), text)
                .trim()
            {
                "(" => depth = depth.saturating_add(1),
                ")" => depth -= 1,
                _ => {}
            }
            close += 1;
        }
        if depth != 0 {
            if operator_kind == Some(TokenKind::KwGeneric) {
                complete = false;
            }
            continue;
        }
        let operand_end = close - 1;
        if operator_kind == Some(TokenKind::KwGeneric) {
            // The controlling expression is unevaluated, while only one
            // association expression runs. Selecting it requires C type
            // compatibility that this token layer cannot establish. Skip all
            // candidate reads and fail the completeness signal instead of
            // reporting every arm as a definite dependency.
            mark_identifiers(&mut out, next + 1, operand_end);
            complete = false;
            continue;
        }
        if operator_kind == Some(TokenKind::KwAlignof) {
            mark_identifiers(&mut out, next + 1, operand_end);
            continue;
        }

        let mut first = next + 1;
        while first < operand_end
            && matches!(
                TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(first))),
                Some(
                    TokenKind::KwConst
                        | TokenKind::KwRestrict
                        | TokenKind::KwVolatile
                        | TokenKind::KwAtomic
                )
            )
        {
            first += 1;
        }
        let first_kind = (first < operand_end)
            .then(|| TokenKind::from_u16(tokens.kind(crate::syntax::ids::TokenId::new(first))));
        let first_text = (first < operand_end).then(|| {
            tokens
                .text(crate::syntax::ids::TokenId::new(first), text)
                .trim()
        });
        let type_like = matches!(
            first_kind.flatten(),
            Some(
                TokenKind::KwBool
                    | TokenKind::KwChar
                    | TokenKind::KwComplex
                    | TokenKind::KwDouble
                    | TokenKind::KwEnum
                    | TokenKind::KwFloat
                    | TokenKind::KwInt
                    | TokenKind::KwInt128
                    | TokenKind::KwLong
                    | TokenKind::KwShort
                    | TokenKind::KwSigned
                    | TokenKind::KwStruct
                    | TokenKind::KwUnion
                    | TokenKind::KwUnsigned
                    | TokenKind::KwVoid
            )
        ) || first_text.is_some_and(|name| {
            resolution
                .resolve_at(name, tokens.start(crate::syntax::ids::TokenId::new(first)))
                .is_some_and(|symbol| symbol.kind == SymbolKind::Typedef)
        });
        let has_array = ((next + 1)..operand_end).any(|index| {
            tokens
                .text(crate::syntax::ids::TokenId::new(index), text)
                .trim()
                == "["
        });
        if !type_like || !has_array {
            mark_identifiers(&mut out, next + 1, operand_end);
        } else if first_kind.flatten() == Some(TokenKind::Identifier) {
            // The leading typedef name is type syntax; later identifiers in
            // the VLA suffix remain evaluated bounds.
            out.insert(first);
        }
    }
    (out, complete)
}

/// The first identifier at or under `node`, which is the assignment target's
/// base variable: `x`, `a[i]`'s `a`, `s.f`'s `s`, `*p`'s `p`.
fn leftmost_name(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    node: NodeId,
) -> Option<(String, Span)> {
    let arena = tree.arena();
    for candidate in arena.preorder(node) {
        if arena.tag(candidate) == Some(NodeTag::NameRef.as_u16()) {
            return name_of(tree, text, token_spans, candidate);
        }
    }
    None
}

/// Whether an `AssignExpr`'s operator is a compound one (`+=`, `<<=`, …).
fn assign_is_compound(tree: &Tree, text: &str, node: NodeId) -> bool {
    operator_text(tree, text, node, |t| t.ends_with('=') && t != "=").is_some()
}

/// Whether this node's own tokens contain `++` or `--`.
fn mentions_inc_dec(tree: &Tree, text: &str, node: NodeId) -> bool {
    operator_text(tree, text, node, |t| t == "++" || t == "--").is_some()
}

/// The first token under `node` whose text satisfies `matches`.
fn operator_text(
    tree: &Tree,
    text: &str,
    node: NodeId,
    matches: impl Fn(&str) -> bool,
) -> Option<String> {
    let (first, end) = tree.arena().token_extent(node)?;
    for index in first..end {
        let id = crate::syntax::ids::TokenId::new(index);
        // Trimmed: `Tokens::text` runs to the next token's start, so `+=`
        // arrives as `"+= "` and no suffix test on it can succeed.
        let token = tree.tokens().text(id, text).trim();
        if matches(token) {
            return Some(token.to_string());
        }
    }
    None
}

/// The name spans of declarators that carry an initializer.
///
/// `int a = 1, b;` initializes `a` and not `b`, so this pairs each declarator
/// with the initializer that follows it before the next one. A declarator with
/// no initializer binds a name and writes nothing, which is the difference
/// between "this local is dead" and "this local is uninitialized".
fn initialized_declarator_spans(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
) -> Vec<Span> {
    let arena = tree.arena();
    let mut out = Vec::new();
    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::Decl.as_u16()) {
            continue;
        }
        // Every declared name and every initializer in this declaration, in
        // source order.
        let mut names: Vec<Span> = Vec::new();
        let mut inits: Vec<Span> = Vec::new();
        let mut arrays: Vec<Span> = Vec::new();
        for inner in arena.preorder(node) {
            match arena.tag(inner).and_then(NodeTag::from_u16) {
                Some(NodeTag::DeclName) => {
                    if let Some((_, span)) = name_of(tree, text, token_spans, inner) {
                        names.push(span);
                    }
                }
                Some(NodeTag::Initializer) | Some(NodeTag::InitList) => {
                    if let Some(span) = arena.span(inner, token_spans) {
                        inits.push(span);
                    }
                }
                Some(NodeTag::ArraySuffix) => {
                    if let Some(span) = arena.span(inner, token_spans) {
                        arrays.push(span);
                    }
                }
                _ => {}
            }
        }
        names.sort_by_key(|span| span.lo);
        for (index, name) in names.iter().enumerate() {
            let next = names.get(index + 1).map_or(u32::MAX, |span| span.lo);
            let has_initializer = inits
                .iter()
                .any(|init| init.lo >= name.hi && init.lo < next);
            // `int a[8];` has no initializer but still defines `a`: the array
            // decays to a well-defined address, and reading `a` to subscript
            // it is not a read of uninitialized storage. Only the *elements*
            // are uninitialized, and this analysis does not model elements.
            let is_array = arrays
                .iter()
                .any(|suffix| suffix.lo >= name.hi && suffix.lo < next);
            if has_initializer || is_array {
                out.push(*name);
            }
        }
    }
    out
}

/// Whether this unary expression is `&` applied to a bare name.
///
/// `&x` only. `&a[i]` and `&s.f` name interior storage, and treating them as
/// a definition of the base would claim the callee can replace the whole
/// object, which over-approximates further than the rest of this module does.
fn takes_address(tree: &Tree, text: &str, node: NodeId) -> bool {
    let arena = tree.arena();
    let Some((first, end)) = arena.token_extent(node) else {
        return false;
    };
    if first >= end {
        return false;
    }
    let id = crate::syntax::ids::TokenId::new(first);
    if tree.tokens().text(id, text).trim() != "&" {
        return false;
    }
    arena
        .children_iter(node)
        .next()
        .is_some_and(|operand| is_direct_name(tree, operand))
}

/// Whether an assignment target is a bare name rather than a place expression.
///
/// `x = v` writes `x`. `a[i] = v`, `s.f = v` and `*p = v` write memory the
/// base points at, and the base itself is unchanged; treating those as
/// definitions of the base kills the base's real reaching definition, which is
/// unsound in the direction that loses edges rather than adds them.
///
/// A parenthesised name is still a name: `(x) = v` writes `x`.
pub(super) fn is_direct_name(tree: &Tree, target: NodeId) -> bool {
    let arena = tree.arena();
    let mut node = target;
    loop {
        match arena.tag(node).and_then(NodeTag::from_u16) {
            Some(NodeTag::NameRef) => return true,
            Some(NodeTag::ParenExpr) => {
                // Descend through the parentheses to whatever they wrap.
                match arena.children_iter(node).next() {
                    Some(child) => node = child,
                    None => return false,
                }
            }
            Some(NodeTag::UnaryExpr) => match dereference_of_address(tree, node) {
                Some((_, target)) => node = target,
                None => return false,
            },
            _ => return false,
        }
    }
}

/// The address node and underlying place in a transparent `*&place` pair.
pub(super) fn dereference_of_address(tree: &Tree, node: NodeId) -> Option<(NodeId, NodeId)> {
    let arena = tree.arena();
    if arena.tag(node) != Some(NodeTag::UnaryExpr.as_u16())
        || !arena.main_token(node).is_some_and(|token| {
            TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Star)
        })
    {
        return None;
    }
    let mut address = arena.children_iter(node).next()?;
    while arena.tag(address) == Some(NodeTag::ParenExpr.as_u16()) {
        address = arena.children_iter(address).next()?;
    }
    if arena.tag(address) != Some(NodeTag::UnaryExpr.as_u16())
        || !arena.main_token(address).is_some_and(|token| {
            TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Amp)
        })
    {
        return None;
    }
    let target = arena.children_iter(address).next()?;
    Some((address, target))
}

/// The spans of names that are types rather than values.
///
/// A declaration's specifiers and a cast's type name both hold identifiers ---
/// `int32_t`, a struct tag, a typedef --- and none of them is a read of a
/// variable. Collected as spans because the walk that needs the answer sees a
/// name before it knows what encloses it.
fn type_name_spans(tree: &Tree, text: &str, token_spans: &[Span], root: NodeId) -> Vec<Span> {
    let arena = tree.arena();
    let mut out = Vec::new();
    for node in arena.preorder(root) {
        let tag = arena.tag(node).and_then(NodeTag::from_u16);

        // The declared positions, where a name is a type by construction.
        if matches!(
            tag,
            Some(NodeTag::DeclSpecifiers) | Some(NodeTag::TypeName) | Some(NodeTag::ParamList)
        ) {
            for inner in arena.preorder(node) {
                if arena.tag(inner) == Some(NodeTag::NameRef.as_u16()) {
                    if let Some((_, span)) = name_of(tree, text, token_spans, inner) {
                        out.push(span);
                    }
                }
            }
            continue;
        }

        // `sizeof(T)` and `_Alignof(T)`. Without a typedef table the parser
        // cannot tell `sizeof(x)` from `sizeof(T)` --- `look.rs` says so and
        // says why --- so the operand arrives as a parenthesised name either
        // way. It is a type position all the same, and counting it as a read
        // of an undefined variable was 517 of the 1,970 unresolved uses this
        // analysis first reported over the fixture corpus.
        // Ambiguous `sizeof(T)`/`sizeof(x)` ownership now lives in the resolved
        // `EvaluationPlan`; this scan only excludes unambiguous type regions.
        let _ = tag;
    }
    out
}

#[derive(Clone, Copy)]
enum LiteralControl {
    And,
    Or,
    Conditional,
}

fn literal_control(tree: &Tree, node: NodeId) -> Option<LiteralControl> {
    let arena = tree.arena();
    match arena.tag(node).and_then(NodeTag::from_u16)? {
        NodeTag::CondExpr => Some(LiteralControl::Conditional),
        NodeTag::BinaryExpr => {
            let (_, left_end) = arena.token_extent(arena.child(node, 0)?)?;
            let (right_start, _) = arena.token_extent(arena.child(node, 1)?)?;
            for index in left_end..right_start {
                let token = crate::syntax::ids::TokenId::new(index);
                match TokenKind::from_u16(tree.tokens().kind(token)) {
                    Some(TokenKind::AmpAmp) => return Some(LiteralControl::And),
                    Some(TokenKind::PipePipe) => return Some(LiteralControl::Or),
                    _ => {}
                }
            }
            None
        }
        _ => None,
    }
}

/// Truth of a syntactically integer constant, without target-width guesses.
///
/// Zero/nonzero is independent of the integer type and suffix. Unsupported or
/// malformed spellings remain unknown rather than pruning executable code.
fn integer_literal_truth(source: &str) -> Option<bool> {
    let compact: String = source
        .chars()
        .filter(|character| *character != '\'')
        .collect();
    let (digits, radix, suffix) = if let Some(rest) = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
    {
        let count = rest
            .chars()
            .take_while(|character| character.is_ascii_hexdigit())
            .count();
        (&rest[..count], 16, &rest[count..])
    } else if let Some(rest) = compact
        .strip_prefix("0b")
        .or_else(|| compact.strip_prefix("0B"))
    {
        let count = rest
            .chars()
            .take_while(|character| matches!(character, '0' | '1'))
            .count();
        (&rest[..count], 2, &rest[count..])
    } else {
        let count = compact
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .count();
        let digits = &compact[..count];
        let radix = if digits.len() > 1 && digits.starts_with('0') {
            8
        } else {
            10
        };
        (digits, radix, &compact[count..])
    };
    if digits.is_empty()
        || !suffix
            .chars()
            .all(|character| matches!(character, 'u' | 'U' | 'l' | 'L'))
        || (radix == 8
            && !digits
                .chars()
                .all(|character| matches!(character, '0'..='7')))
    {
        return None;
    }
    Some(digits.chars().any(|character| character != '0'))
}

fn constant_truth(tree: &Tree, text: &str, spans: &[Span], mut node: NodeId) -> Option<bool> {
    let arena = tree.arena();
    loop {
        match arena.tag(node).and_then(NodeTag::from_u16)? {
            NodeTag::ParenExpr => node = arena.children_iter(node).next()?,
            NodeTag::CommaExpr => node = arena.children_iter(node).last()?,
            NodeTag::Literal => {
                let token = arena.main_token(node)?;
                if TokenKind::from_u16(tree.tokens().kind(token)) != Some(TokenKind::IntLiteral) {
                    return None;
                }
                let span = arena.span(node, spans)?;
                return integer_literal_truth(text.get(span.lo as usize..span.hi as usize)?);
            }
            NodeTag::UnaryExpr => {
                let operator = arena
                    .main_token(node)
                    .and_then(|token| TokenKind::from_u16(tree.tokens().kind(token)))?;
                let operand = arena.children_iter(node).next()?;
                match operator {
                    TokenKind::Bang => return Some(!constant_truth(tree, text, spans, operand)?),
                    TokenKind::Plus | TokenKind::Minus => node = operand,
                    _ => return None,
                }
            }
            _ => return None,
        }
    }
}

fn has_jump_entry(tree: &Tree, node: NodeId) -> bool {
    let arena = tree.arena();
    arena.preorder(node).any(|candidate| {
        matches!(
            arena.tag(candidate).and_then(NodeTag::from_u16),
            Some(NodeTag::LabelStmt | NodeTag::CaseLabel | NodeTag::DefaultLabel)
        )
    })
}

fn node_identifier_at(tree: &Tree, text: &str, node: NodeId, offset: u32) -> Option<String> {
    let (first, end) = tree.arena().token_extent(node)?;
    let raw = first.checked_add(offset)?;
    if raw >= end {
        return None;
    }
    let token = crate::syntax::ids::TokenId::new(raw);
    (TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Identifier))
        .then(|| tree.tokens().text(token, text).to_owned())
}

struct ReachableJumpEntries {
    labels: BTreeSet<String>,
    any_label: bool,
}

fn reachable_jump_entries(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    root: NodeId,
    unevaluated: &Regions,
    executable: &NodeSpanIndex,
) -> ReachableJumpEntries {
    let arena = tree.arena();
    let mut entries = ReachableJumpEntries {
        labels: BTreeSet::new(),
        any_label: false,
    };
    for node in arena.preorder(root) {
        let Some(span) = arena.span(node, spans) else {
            continue;
        };
        if unevaluated.contains(span) || !executable.has_span_within(span) {
            continue;
        }
        match arena.tag(node).and_then(NodeTag::from_u16) {
            Some(NodeTag::GotoStmt) => match node_identifier_at(tree, text, node, 1) {
                Some(label) => {
                    entries.labels.insert(label);
                }
                None => entries.any_label = true,
            },
            // GNU asm-goto can transfer to a C label. The parser intentionally
            // treats asm as opaque, so preserve every ordinary label when its
            // spelling advertises that control-flow form.
            Some(NodeTag::Asm)
                if text
                    .get(span.lo as usize..span.hi as usize)
                    .is_some_and(|source| source.contains("goto")) =>
            {
                entries.any_label = true;
            }
            _ => {}
        }
    }
    entries
}

fn first_jump_entry_start(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    node: NodeId,
    target: Option<&str>,
    reachable: &ReachableJumpEntries,
) -> Option<u32> {
    let arena = tree.arena();
    let mut first = None;
    let mut stack = vec![(node, 0usize)];
    while let Some((candidate, nested_switches)) = stack.pop() {
        let tag = arena.tag(candidate).and_then(NodeTag::from_u16);
        let is_entry = match tag {
            Some(NodeTag::LabelStmt) => {
                let name = node_identifier_at(tree, text, candidate, 0);
                target.map_or_else(
                    || {
                        reachable.any_label
                            || name
                                .as_ref()
                                .is_some_and(|name| reachable.labels.contains(name))
                    },
                    |target| name.as_deref() == Some(target),
                )
            }
            Some(NodeTag::CaseLabel | NodeTag::DefaultLabel) => {
                target.is_none() && nested_switches == 0
            }
            _ => false,
        };
        if is_entry {
            if let Some(span) = arena.span(candidate, spans) {
                first = Some(first.map_or(span.lo, |lo: u32| lo.min(span.lo)));
            }
        }
        let child_depth = nested_switches + usize::from(tag == Some(NodeTag::SwitchStmt));
        let children: Vec<_> = arena.children_iter(candidate).collect();
        stack.extend(children.into_iter().rev().map(|child| (child, child_depth)));
    }
    first
}

fn literal_dead_statements(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    node: NodeId,
) -> Option<Vec<NodeId>> {
    let arena = tree.arena();
    let tag = arena.tag(node).and_then(NodeTag::from_u16)?;
    let children: Vec<_> = arena.children_iter(node).collect();
    let dead = match tag {
        NodeTag::IfStmt => {
            let condition_index = children.iter().position(|&child| {
                arena
                    .tag(child)
                    .and_then(NodeTag::from_u16)
                    .is_some_and(NodeTag::is_expression)
            })?;
            let condition = children[condition_index];
            let truth = constant_truth(tree, text, spans, condition)?;
            let arms = &children[condition_index + 1..];
            vec![if truth { arms.get(1) } else { arms.first() }.copied()?]
        }
        NodeTag::WhileStmt => {
            let condition_index = children.iter().position(|&child| {
                arena
                    .tag(child)
                    .and_then(NodeTag::from_u16)
                    .is_some_and(NodeTag::is_expression)
            })?;
            let condition = children[condition_index];
            if constant_truth(tree, text, spans, condition)? {
                return None;
            }
            vec![*children.get(condition_index + 1)?]
        }
        NodeTag::ForStmt => {
            let condition_group = children
                .iter()
                .copied()
                .find(|child| arena.tag(*child) == Some(NodeTag::ForCond.as_u16()))?;
            let condition = arena.preorder(condition_group).find(|child| {
                arena
                    .tag(*child)
                    .and_then(NodeTag::from_u16)
                    .is_some_and(NodeTag::is_expression)
            })?;
            if constant_truth(tree, text, spans, condition)? {
                return None;
            }
            let body = *children.last()?;
            if has_jump_entry(tree, body) {
                return None;
            }
            let step = children
                .iter()
                .copied()
                .find(|child| arena.tag(*child) == Some(NodeTag::ForStep.as_u16()))?;
            return Some(vec![step, body]);
        }
        _ => return None,
    };
    (!dead.iter().any(|&dead| has_jump_entry(tree, dead))).then_some(dead)
}

fn loop_is_provably_non_exiting(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    node: NodeId,
    unevaluated: &Regions,
) -> bool {
    let arena = tree.arena();
    let Some(tag) = arena.tag(node).and_then(NodeTag::from_u16) else {
        return false;
    };
    let children: Vec<_> = arena.children_iter(node).collect();
    let (condition, body) = match tag {
        NodeTag::WhileStmt => {
            let Some(condition_index) = children.iter().position(|&child| {
                arena
                    .tag(child)
                    .and_then(NodeTag::from_u16)
                    .is_some_and(NodeTag::is_expression)
            }) else {
                return false;
            };
            (
                Some(children[condition_index]),
                children.get(condition_index + 1).copied(),
            )
        }
        NodeTag::DoWhileStmt => {
            let condition = children.iter().rev().copied().find(|child| {
                arena
                    .tag(*child)
                    .and_then(NodeTag::from_u16)
                    .is_some_and(NodeTag::is_expression)
            });
            (condition, children.first().copied())
        }
        NodeTag::ForStmt => {
            let condition_group = children
                .iter()
                .copied()
                .find(|child| arena.tag(*child) == Some(NodeTag::ForCond.as_u16()));
            let condition = condition_group.and_then(|group| {
                arena.preorder(group).find(|child| {
                    arena
                        .tag(*child)
                        .and_then(NodeTag::from_u16)
                        .is_some_and(NodeTag::is_expression)
                })
            });
            (condition, children.last().copied())
        }
        _ => return false,
    };
    let condition_is_true = condition
        .map(|condition| constant_truth(tree, text, spans, condition) == Some(true))
        .unwrap_or(tag == NodeTag::ForStmt);
    if !condition_is_true {
        return false;
    }
    let Some(body) = body else {
        return false;
    };
    !arena.preorder(body).any(|candidate| {
        matches!(
            arena.tag(candidate).and_then(NodeTag::from_u16),
            Some(NodeTag::BreakStmt | NodeTag::GotoStmt)
        ) && arena
            .span(candidate, spans)
            .is_none_or(|span| !unevaluated.contains(span))
    })
}

fn switch_dispatch_prefix_spans(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    root: NodeId,
    unevaluated: &Regions,
    executable: &NodeSpanIndex,
) -> Vec<Span> {
    let arena = tree.arena();
    let mut dead = Vec::new();
    let reachable_entries =
        reachable_jump_entries(tree, text, spans, root, unevaluated, executable);
    for switch in arena.preorder(root) {
        if arena.tag(switch) != Some(NodeTag::SwitchStmt.as_u16()) {
            continue;
        }
        let Some(body) = arena.children_iter(switch).last() else {
            continue;
        };
        let Some(body_span) = arena.span(body, spans) else {
            continue;
        };

        // Case/default labels in a nested switch belong to that switch, while
        // an ordinary label at any depth can still be the target of a goto.
        let mut first_entry = None;
        let mut stack = vec![(body, 0usize)];
        while let Some((node, nested_switches)) = stack.pop() {
            let tag = arena.tag(node).and_then(NodeTag::from_u16);
            let is_entry = match tag {
                Some(NodeTag::LabelStmt) => {
                    reachable_entries.any_label
                        || node_identifier_at(tree, text, node, 0)
                            .is_some_and(|name| reachable_entries.labels.contains(&name))
                }
                Some(NodeTag::CaseLabel | NodeTag::DefaultLabel) => nested_switches == 0,
                _ => false,
            };
            if is_entry {
                if let Some(span) = arena.span(node, spans) {
                    first_entry = Some(first_entry.map_or(span.lo, |lo: u32| lo.min(span.lo)));
                }
            }
            let child_depth = nested_switches + usize::from(tag == Some(NodeTag::SwitchStmt));
            let children: Vec<_> = arena.children_iter(node).collect();
            stack.extend(children.into_iter().rev().map(|child| (child, child_depth)));
        }

        let end = first_entry.unwrap_or(body_span.hi);
        if body_span.lo < end {
            dead.push(Span::new(body_span.lo, end));
        }
    }
    dead
}

fn non_fallthrough_tail_spans(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    root: NodeId,
    unevaluated: &Regions,
    executable: &NodeSpanIndex,
    include_non_exiting_loops: bool,
) -> Vec<Span> {
    let arena = tree.arena();
    let mut dead = Vec::new();
    let reachable_entries =
        reachable_jump_entries(tree, text, spans, root, unevaluated, executable);
    for compound in arena.preorder(root) {
        if arena.tag(compound) != Some(NodeTag::CompoundStmt.as_u16()) {
            continue;
        }
        // `Some(None)` means any jump entry may resume execution (for return,
        // break, continue, or an indirect goto). A named direct goto can only
        // resume at its exact label.
        let mut after_terminator: Option<Option<String>> = None;
        for child in arena.children_iter(compound) {
            if let Some(target) = after_terminator.as_ref() {
                if let Some(entry) = first_jump_entry_start(
                    tree,
                    text,
                    spans,
                    child,
                    target.as_deref(),
                    &reachable_entries,
                ) {
                    // A nested label revives only the suffix beginning at that
                    // label. Its enclosing condition and earlier statements
                    // are bypassed by the jump.
                    if let Some(span) = arena.span(child, spans) {
                        if span.lo < entry {
                            dead.push(Span::new(span.lo, entry));
                        }
                    }
                } else {
                    if let Some(span) = arena.span(child, spans) {
                        dead.push(span);
                    }
                    continue;
                }
            }
            let tag = arena.tag(child).and_then(NodeTag::from_u16);
            after_terminator = if tag == Some(NodeTag::GotoStmt) {
                Some(node_identifier_at(tree, text, child, 1))
            } else if (include_non_exiting_loops
                && loop_is_provably_non_exiting(tree, text, spans, child, unevaluated))
                || matches!(
                    tag,
                    Some(NodeTag::ReturnStmt | NodeTag::BreakStmt | NodeTag::ContinueStmt)
                )
            {
                Some(None)
            } else {
                None
            };
        }
    }
    dead
}

/// Operand shapes whose result cannot be a variable-length array in valid C.
/// Classify the outer expression, not its descendants: `*f()` may be an array
/// even though `f()` itself cannot return one. Unknown shapes remain evaluated.
fn unevaluated_spans(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    root: NodeId,
    uses: &[Use],
    types: &[CType],
    file_symbols: &TranslationUnitSymbols,
) -> Vec<Span> {
    let arena = tree.arena();
    let mut out = Vec::new();
    let prune_literal_control = !arena
        .preorder(root)
        .any(|node| arena.tag(node) == Some(NodeTag::PpDirective.as_u16()));
    // Most functions never need type-sensitive dereference classification.
    // Build an exact-span lookup lazily, retaining the first use just as the
    // former linear find did if malformed input produces duplicate spans.
    let mut bindings_by_span = None;
    for node in arena.preorder(root) {
        if arena.tag(node) == Some(NodeTag::PostfixExpr.as_u16()) {
            let children: Vec<_> = arena.children_iter(node).collect();
            let callee = children.first().and_then(|&child| {
                (arena.tag(child) == Some(NodeTag::NameRef.as_u16()))
                    .then(|| name_of(tree, text, spans, child))
                    .flatten()
            });
            let args: Vec<_> = children
                .iter()
                .copied()
                .filter(|&child| arena.tag(child) == Some(NodeTag::CallArgs.as_u16()))
                .collect();
            if let Some((name, callee_span)) = callee {
                let shadowed = uses
                    .iter()
                    .any(|use_| use_.span == callee_span && !use_.binding.is_free());
                if name == "__builtin_constant_p"
                    && !shadowed
                    && !file_symbols.declares_function(&name)
                    && args.len() == 1
                {
                    let arguments: Vec<_> = arena.children_iter(args[0]).collect();
                    if arguments.len() == 1 {
                        if let Some(span) = arena.span(arguments[0], spans) {
                            out.push(span);
                        }
                    }
                }
            }
        }
        if prune_literal_control {
            if let Some(dead) = literal_dead_statements(tree, text, spans, node) {
                for dead in dead {
                    if let Some(span) = arena.span(dead, spans) {
                        out.push(span);
                    }
                }
            }
            if let Some(control) = literal_control(tree, node) {
                let children: Vec<_> = arena.children_iter(node).collect();
                if let Some(truth) = children
                    .first()
                    .and_then(|condition| constant_truth(tree, text, spans, *condition))
                {
                    let dead = match (control, truth) {
                        (LiteralControl::And, false) | (LiteralControl::Or, true) => {
                            children.get(1)
                        }
                        (LiteralControl::Conditional, true) => children.get(2),
                        (LiteralControl::Conditional, false) => children.get(1),
                        _ => None,
                    };
                    if let Some(span) = dead.and_then(|dead| arena.span(*dead, spans)) {
                        out.push(span);
                    }
                }
            }
        }
        if arena.tag(node) != Some(NodeTag::UnaryExpr.as_u16())
            || !starts_with_sizeof(tree, text, node)
        {
            continue;
        }
        let Some(mut operand) = arena.children_iter(node).next() else {
            continue;
        };
        while arena.tag(operand) == Some(NodeTag::ParenExpr.as_u16()) {
            let Some(child) = arena.children_iter(operand).next() else {
                break;
            };
            operand = child;
        }
        let fixed = match arena.tag(operand).and_then(NodeTag::from_u16) {
            Some(
                NodeTag::BinaryExpr | NodeTag::AssignExpr | NodeTag::CondExpr | NodeTag::Literal,
            ) => true,
            Some(NodeTag::PostfixExpr) => arena.children_iter(operand).last().is_some_and(|last| {
                matches!(
                    arena.tag(last).and_then(NodeTag::from_u16),
                    Some(NodeTag::CallArgs | NodeTag::IncDecSuffix)
                )
            }),
            Some(NodeTag::UnaryExpr) => arena.token_extent(operand).is_some_and(|(first, end)| {
                first < end
                    && matches!(
                        tree.tokens()
                            .text(crate::syntax::ids::TokenId::new(first), text)
                            .trim(),
                        "+" | "-"
                            | "!"
                            | "~"
                            | "&"
                            | "++"
                            | "--"
                            | "sizeof"
                            | "_Alignof"
                            | "alignof"
                            | "__alignof__"
                    )
            }),
            _ => false,
        };
        // A direct dereference can be classified only after lexical binding
        // resolution. Unknown typedefs can hide VLAs; do not infer their size.
        let mut leaf = operand;
        let mut dereferences = 0;
        loop {
            let tag = arena.tag(leaf).and_then(NodeTag::from_u16);
            if tag == Some(NodeTag::UnaryExpr)
                && arena.token_extent(leaf).is_some_and(|(first, end)| {
                    first < end
                        && tree
                            .tokens()
                            .text(crate::syntax::ids::TokenId::new(first), text)
                            == "*"
                })
            {
                dereferences += 1;
            } else if tag != Some(NodeTag::ParenExpr) {
                break;
            }
            let Some(child) = arena.children_iter(leaf).next() else {
                break;
            };
            leaf = child;
        }
        let fixed_pointee = (|| {
            if dereferences == 0 {
                return false;
            }
            let child = leaf;
            if arena.tag(child) != Some(NodeTag::NameRef.as_u16()) {
                return false;
            }
            let Some(span) = arena.span(child, spans) else {
                return false;
            };
            let bindings = bindings_by_span.get_or_insert_with(|| {
                let mut index = std::collections::BTreeMap::new();
                for use_ in uses {
                    index.entry(use_.span).or_insert(use_.binding);
                }
                index
            });
            let Some(binding) = bindings.get(&span) else {
                return false;
            };
            types.get(binding.0 as usize).is_some_and(|ty| {
                ty.array_rank == 0
                    && ty.pointer_depth >= dereferences
                    && !ty.is_empty()
                    && (ty.pointer_depth > dereferences
                        || ty.specifiers.split_whitespace().all(|word| {
                            matches!(
                                word,
                                "char"
                                    | "short"
                                    | "int"
                                    | "long"
                                    | "signed"
                                    | "unsigned"
                                    | "float"
                                    | "double"
                                    | "_Bool"
                                    | "_Complex"
                                    | "const"
                                    | "volatile"
                                    | "static"
                                    | "extern"
                                    | "register"
                                    | "auto"
                            )
                        }))
            })
        })();
        if fixed || fixed_pointee {
            if let Some(span) = arena.span(node, spans) {
                out.push(span);
            }
        }
    }
    out
}

/// Whether `node` subscripts, selects a member of, or calls something.
fn has_place_suffix(tree: &Tree, node: NodeId) -> bool {
    let arena = tree.arena();
    arena.preorder(node).any(|inner| {
        matches!(
            arena.tag(inner).and_then(NodeTag::from_u16),
            Some(NodeTag::IndexSuffix) | Some(NodeTag::MemberSuffix) | Some(NodeTag::CallArgs)
        )
    })
}

/// Whether this unary expression's operator is `sizeof` or `_Alignof`.
fn starts_with_sizeof(tree: &Tree, text: &str, node: NodeId) -> bool {
    let Some((first, end)) = tree.arena().token_extent(node) else {
        return false;
    };
    if first >= end {
        return false;
    }
    let id = crate::syntax::ids::TokenId::new(first);
    matches!(
        tree.tokens().text(id, text).trim(),
        "sizeof" | "_Alignof" | "alignof" | "__alignof__"
    )
}

/// The spans of names in callee position.
///
/// `f(x)` parses as a postfix expression whose first child is the callee and
/// whose `CallArgs` child holds the arguments. The callee's *name* is only a
/// data read when it resolves to a local binding --- a function pointer. A
/// plain `memcpy` resolves to nothing, and counting it as a read of an
/// undefined variable is noise, not a finding.
fn callee_name_spans(tree: &Tree, text: &str, token_spans: &[Span], root: NodeId) -> Vec<Span> {
    let arena = tree.arena();
    let mut out = Vec::new();
    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::PostfixExpr.as_u16()) {
            continue;
        }
        let children: Vec<NodeId> = arena.children_iter(node).collect();
        let calls = children
            .iter()
            .any(|child| arena.tag(*child) == Some(NodeTag::CallArgs.as_u16()));
        if !calls {
            continue;
        }
        let Some(first) = children.first() else {
            continue;
        };
        match arena.tag(*first).and_then(NodeTag::from_u16) {
            Some(NodeTag::NameRef) => {
                if let Some((_, span)) = name_of(tree, text, token_spans, *first) {
                    out.push(span);
                }
            }
            // `(T)(x)` and `(f)(x)` are the same shape, and `look.rs` records
            // that the parser resolves the ambiguity toward the call reading
            // on purpose. Either way the parenthesised name is in callee
            // position, so the binding test decides: a resolved name is a
            // function pointer being read, a free one is a type or a function.
            Some(NodeTag::ParenExpr) => {
                let names: Vec<NodeId> = arena
                    .preorder(*first)
                    .filter(|inner| arena.tag(*inner) == Some(NodeTag::NameRef.as_u16()))
                    .collect();
                if names.len() == 1 {
                    if let Some((_, span)) = name_of(tree, text, token_spans, names[0]) {
                        out.push(span);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// The CFG node whose spans contain `span`, or the entry when none does.
///
/// Falling back to the entry rather than dropping the event is deliberate: a
/// parameter is declared in the header, which no statement node covers, and
/// dropping it would leave every read of that parameter unresolved.
#[cfg(test)]
fn node_for_span(cfg: &Cfg, span: Span) -> u32 {
    for (index, node) in cfg.nodes().iter().enumerate() {
        for covered in node.spans() {
            if covered.lo <= span.lo && span.hi <= covered.hi {
                return index as u32;
            }
        }
    }
    cfg.entry().index() as u32
}

/// Reusable containment index for joining many source events to one CFG.
///
/// Entries are ordered by start offset. Prefix-maximum ends let a reverse
/// query stop as soon as no earlier interval can contain the event. Taking the
/// lowest matching node ID preserves `node_for_span`'s observable choice if
/// recovered node spans overlap.
pub(super) struct NodeSpanIndex {
    entries: Vec<(Span, u32)>,
    prefix_max_hi: Vec<u32>,
    entry: u32,
}

impl NodeSpanIndex {
    pub(super) fn new(cfg: &Cfg) -> Self {
        let mut entries = Vec::new();
        for (index, node) in cfg.nodes().iter().enumerate() {
            entries.extend(node.spans().iter().map(|span| (*span, index as u32)));
        }
        entries.sort_unstable_by_key(|(span, node)| (span.lo, *node, span.hi));
        let mut prefix_max_hi = Vec::with_capacity(entries.len());
        let mut max_hi = 0;
        for (span, _) in &entries {
            max_hi = max_hi.max(span.hi);
            prefix_max_hi.push(max_hi);
        }
        Self {
            entries,
            prefix_max_hi,
            entry: cfg.entry().index() as u32,
        }
    }

    pub(super) fn node_for(&self, span: Span) -> u32 {
        self.covering_node(span).unwrap_or(self.entry)
    }

    fn has_span_within(&self, outer: Span) -> bool {
        let first = self
            .entries
            .partition_point(|(inner, _)| inner.lo < outer.lo);
        self.entries[first..]
            .iter()
            .take_while(|(inner, _)| inner.lo <= outer.hi)
            .any(|(inner, _)| inner.lo < inner.hi && inner.hi <= outer.hi)
    }

    fn covering_node(&self, span: Span) -> Option<u32> {
        let mut cursor = self
            .entries
            .partition_point(|(covered, _)| covered.lo <= span.lo);
        let mut found = None;
        while cursor > 0 {
            cursor -= 1;
            if self.prefix_max_hi[cursor] < span.hi {
                break;
            }
            let (covered, node) = self.entries[cursor];
            if span.hi <= covered.hi {
                found = Some(found.map_or(node, |prior: u32| prior.min(node)));
            }
        }
        found
    }
}

#[cfg(test)]
mod node_span_index_tests {
    use super::*;
    use crate::csource::{cfg::function_cfgs, parse::parse};

    #[test]
    fn indexed_node_lookup_matches_the_linear_reference() {
        let source = "int f(int x){if(x){x++;}else{x=2;}while(x--)x+=3;return x;}";
        let (tree, _) = parse(source).into_parts();
        let (functions, _) = function_cfgs(&tree, source).into_parts();
        let cfg = &functions[0].cfg;
        let index = NodeSpanIndex::new(cfg);
        for lo in 0..=source.len() as u32 {
            for hi in lo..=source.len() as u32 {
                let span = Span::new(lo, hi);
                assert_eq!(index.node_for(span), node_for_span(cfg, span), "{span:?}");
            }
        }
    }
}
