//! Structural C types owned by one semantic function snapshot.
//!
//! The public [`crate::csource::dataflow::CType`] remains a spelling-oriented
//! compatibility view. This module preserves declarator operator order, which
//! counters such as `pointer_depth` and `array_rank` necessarily erase.

use std::collections::{BTreeMap, BTreeSet};

use crate::csource::lex::kind::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::syntax::ids::{NodeId, Span, TokenId};

use super::declarations::{
    parameter_declarations, FunctionResolution, ParameterDeclarator, SymbolKind,
    TranslationUnitSymbols,
};

/// Dense identity of one structural type node within a function snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct TypeId(pub(crate) u32);

/// Dense identity of one runtime array-bound value in a function snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct BoundSlotId(pub(crate) u32);

/// Dense identity of one struct/union definition within a function snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct RecordId(pub(crate) u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum RecordKind {
    Struct,
    Union,
}

/// Source ownership of a runtime type value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundSlot {
    pub(crate) declaration: Span,
    pub(crate) expression: Span,
    pub(crate) input_declarations: Vec<Span>,
}

/// Array-bound syntax without pretending a runtime expression is a constant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ArrayBound {
    Constant(String),
    Runtime(BoundSlotId),
    Incomplete,
    PrototypeStar,
}

/// One node in the structural type graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeNode {
    SpelledBase {
        specifiers: String,
    },
    Record {
        record: RecordId,
    },
    Pointer {
        pointee: TypeId,
    },
    Array {
        element: TypeId,
        bound: ArrayBound,
    },
    Function {
        result: TypeId,
    },
    Qualified {
        unqualified: TypeId,
        qualifiers: Qualifiers,
    },
    Alias {
        declaration: Span,
        target: Option<TypeId>,
    },
    Typeof {
        operand: Span,
        captured: Option<TypeId>,
    },
    Unknown {
        reason: UnknownTypeReason,
    },
}

/// Why semantic type construction could not establish a base type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UnknownTypeReason {
    MissingSpecifier,
    UnresolvedName(String),
    MissingAliasTarget(Span),
    UnresolvedTypeofOperand(String),
    AliasCycle(Span),
}

/// A localized failure produced while constructing the type graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypeIssue {
    pub(crate) reason: UnknownTypeReason,
    pub(crate) span: Span,
}

/// Qualifiers attached to one exact type layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Qualifiers {
    pub(crate) is_const: bool,
    pub(crate) is_volatile: bool,
    pub(crate) is_restrict: bool,
    pub(crate) is_atomic: bool,
}

impl Qualifiers {
    fn is_empty(self) -> bool {
        self == Self::default()
    }
}

/// Lossy compatibility projection retained while public `CType` migrates.
pub(crate) struct TypeShape<'a> {
    pub(crate) specifiers: &'a str,
    pub(crate) pointer_depth: u32,
    pub(crate) array_rank: u32,
}

/// Structural types declared within one function.
#[derive(Debug, Clone, Default)]
pub(crate) struct FunctionTypes {
    nodes: Vec<TypeNode>,
    by_declaration: BTreeMap<Span, DeclaredType>,
    records: Vec<RecordType>,
    record_by_body: BTreeMap<Span, RecordId>,
    named_records: BTreeMap<(RecordKind, String), Vec<RecordBinding>>,
    bound_slots: Vec<BoundSlot>,
    issues: Vec<TypeIssue>,
}

#[derive(Debug, Clone)]
struct RecordType {
    body: NodeId,
    kind: RecordKind,
    members: BTreeMap<String, TypeId>,
}

#[derive(Debug, Clone, Copy)]
struct RecordBinding {
    introduced: u32,
    scope_hi: u32,
    record: RecordId,
}

#[derive(Debug, Clone)]
struct DeclaredType {
    written: TypeId,
    adjusted: TypeId,
    spelling: String,
}

impl FunctionTypes {
    pub(crate) fn bound_slots(&self) -> impl Iterator<Item = (BoundSlotId, &BoundSlot)> {
        self.bound_slots
            .iter()
            .enumerate()
            .map(|(index, slot)| (BoundSlotId(index as u32), slot))
    }

    pub(crate) fn issues(&self) -> &[TypeIssue] {
        &self.issues
    }

    pub(crate) fn bound_slot(&self, id: BoundSlotId) -> Option<&BoundSlot> {
        self.bound_slots.get(id.0 as usize)
    }

    pub(crate) fn runtime_bound_slots_of_declaration(
        &self,
        span: Span,
    ) -> Vec<(BoundSlotId, &BoundSlot)> {
        let Some(mut current) = self.type_of_declaration(span) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut visited = BTreeSet::new();
        while visited.insert(current) {
            match self.node(current) {
                Some(TypeNode::Array {
                    element,
                    bound: ArrayBound::Runtime(id),
                }) => {
                    if let Some(slot) = self.bound_slot(*id) {
                        out.push((*id, slot));
                    }
                    current = *element;
                }
                Some(TypeNode::Array { element, .. }) => current = *element,
                Some(TypeNode::Pointer { pointee }) => current = *pointee,
                Some(TypeNode::Function { result }) => current = *result,
                Some(TypeNode::Qualified { unqualified, .. }) => current = *unqualified,
                Some(TypeNode::Alias {
                    target: Some(target),
                    ..
                }) => current = *target,
                Some(TypeNode::Typeof {
                    captured: Some(target),
                    ..
                }) => current = *target,
                _ => break,
            }
        }
        out
    }

    pub(crate) fn has_typeof_capture_of(&self, operand: Span) -> bool {
        self.nodes.iter().any(|node| {
            matches!(node, TypeNode::Typeof { operand: candidate, captured: Some(_) } if *candidate == operand)
        })
    }

    pub(crate) fn type_of_declaration(&self, span: Span) -> Option<TypeId> {
        self.by_declaration.get(&span).map(|ty| ty.written)
    }

    pub(crate) fn adjusted_type_of_declaration(&self, span: Span) -> Option<TypeId> {
        self.by_declaration.get(&span).map(|ty| ty.adjusted)
    }

    pub(crate) fn adjusted_is_pointer(&self, span: Span) -> bool {
        self.adjusted_outer_node(span)
            .is_some_and(|node| matches!(node, TypeNode::Pointer { .. }))
    }

    pub(crate) fn adjusted_is_array(&self, span: Span) -> bool {
        self.adjusted_outer_node(span)
            .is_some_and(|node| matches!(node, TypeNode::Array { .. }))
    }

    fn adjusted_outer_node(&self, span: Span) -> Option<&TypeNode> {
        let mut id = self.adjusted_type_of_declaration(span)?;
        let mut remaining = self.nodes.len().saturating_add(1);
        while remaining > 0 {
            remaining -= 1;
            match self.node(id) {
                Some(TypeNode::Qualified { unqualified, .. }) => id = *unqualified,
                Some(TypeNode::Alias {
                    target: Some(target),
                    ..
                }) => id = *target,
                Some(TypeNode::Typeof {
                    captured: Some(target),
                    ..
                }) => id = *target,
                Some(
                    TypeNode::Alias { target: None, .. }
                    | TypeNode::Typeof { captured: None, .. }
                    | TypeNode::Unknown { .. },
                ) => return None,
                node => return node,
            }
        }
        None
    }

    pub(crate) fn node(&self, id: TypeId) -> Option<&TypeNode> {
        self.nodes.get(id.0 as usize)
    }

    pub(crate) fn shape_of_declaration(&self, span: Span) -> Option<TypeShape<'_>> {
        let declared = self.by_declaration.get(&span)?;
        let mut current = self.type_of_declaration(span)?;
        let mut pointer_depth = 0;
        let mut array_rank = 0;
        loop {
            match self.node(current)? {
                TypeNode::SpelledBase { .. } => {
                    return Some(TypeShape {
                        specifiers: &declared.spelling,
                        pointer_depth,
                        array_rank,
                    });
                }
                TypeNode::Record { .. } => {
                    return Some(TypeShape {
                        specifiers: &declared.spelling,
                        pointer_depth,
                        array_rank,
                    });
                }
                TypeNode::Pointer { pointee } => {
                    pointer_depth += 1;
                    current = *pointee;
                }
                TypeNode::Array { element, .. } => {
                    array_rank += 1;
                    current = *element;
                }
                TypeNode::Function { result } => current = *result,
                TypeNode::Qualified { unqualified, .. } => current = *unqualified,
                TypeNode::Alias { .. } => {
                    return Some(TypeShape {
                        specifiers: &declared.spelling,
                        pointer_depth,
                        array_rank,
                    });
                }
                TypeNode::Typeof {
                    captured: Some(target),
                    ..
                } => current = *target,
                TypeNode::Typeof { captured: None, .. } => {
                    return Some(TypeShape {
                        specifiers: &declared.spelling,
                        pointer_depth,
                        array_rank,
                    });
                }
                TypeNode::Unknown { .. } => {
                    return Some(TypeShape {
                        specifiers: &declared.spelling,
                        pointer_depth,
                        array_rank,
                    });
                }
            }
        }
    }

    fn push(&mut self, node: TypeNode) -> TypeId {
        let id = TypeId(self.nodes.len() as u32);
        self.nodes.push(node);
        id
    }

    pub(crate) fn member_type(
        &self,
        mut base: TypeId,
        through_pointer: bool,
        member: &str,
    ) -> Option<TypeId> {
        base = self.peel_type(base)?;
        if through_pointer {
            let TypeNode::Pointer { pointee } = self.node(base)? else {
                return None;
            };
            base = self.peel_type(*pointee)?;
        }
        let TypeNode::Record { record } = self.node(base)? else {
            return None;
        };
        self.records
            .get(record.0 as usize)?
            .members
            .get(member)
            .copied()
    }

    pub(crate) fn record_kind(
        &self,
        mut base: TypeId,
        through_pointer: bool,
    ) -> Option<RecordKind> {
        base = self.peel_type(base)?;
        if through_pointer {
            let TypeNode::Pointer { pointee } = self.node(base)? else {
                return None;
            };
            base = self.peel_type(*pointee)?;
        }
        let TypeNode::Record { record } = self.node(base)? else {
            return None;
        };
        self.records
            .get(record.0 as usize)
            .map(|record| record.kind)
    }

    pub(crate) fn type_is_array(&self, ty: TypeId) -> bool {
        self.peel_type(ty)
            .and_then(|id| self.node(id))
            .is_some_and(|node| matches!(node, TypeNode::Array { .. }))
    }

    pub(crate) fn element_type(&self, ty: TypeId) -> Option<TypeId> {
        match self.node(self.peel_type(ty)?)? {
            TypeNode::Array { element, .. } => Some(*element),
            TypeNode::Pointer { pointee } => Some(*pointee),
            _ => None,
        }
    }

    fn peel_type(&self, mut id: TypeId) -> Option<TypeId> {
        let mut remaining = self.nodes.len().saturating_add(1);
        while remaining > 0 {
            remaining -= 1;
            match self.node(id)? {
                TypeNode::Qualified { unqualified, .. } => id = *unqualified,
                TypeNode::Alias {
                    target: Some(target),
                    ..
                }
                | TypeNode::Typeof {
                    captured: Some(target),
                    ..
                } => id = *target,
                TypeNode::Alias { target: None, .. }
                | TypeNode::Typeof { captured: None, .. }
                | TypeNode::Unknown { .. } => return None,
                _ => return Some(id),
            }
        }
        None
    }
}

fn collect_record_definitions(
    types: &mut FunctionTypes,
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    function_offset: u32,
) {
    let arena = tree.arena();
    let local_nodes = arena.preorder(root).collect::<BTreeSet<_>>();
    let visible_file_nodes = arena
        .roots()
        .iter()
        .copied()
        .filter(|node| arena.tag(*node) == Some(NodeTag::Decl.as_u16()))
        .filter(|node| {
            arena
                .span(*node, token_spans)
                .is_some_and(|span| span.lo < function_offset)
        })
        .flat_map(|node| arena.preorder(node))
        .collect::<BTreeSet<_>>();
    for specifiers in arena.preorder_roots() {
        if arena.tag(specifiers) != Some(NodeTag::DeclSpecifiers.as_u16()) {
            continue;
        }
        let Some(specifier_span) = arena.span(specifiers, token_spans) else {
            continue;
        };
        if !local_nodes.contains(&specifiers) && !visible_file_nodes.contains(&specifiers) {
            continue;
        }
        let Some(body) = arena
            .children_iter(specifiers)
            .find(|node| arena.tag(*node) == Some(NodeTag::StructBody.as_u16()))
        else {
            continue;
        };
        let Some(definition) = arena.span(body, token_spans) else {
            continue;
        };
        let Some((kind, name)) = record_header(tree, text, specifiers, body) else {
            continue;
        };
        let id = RecordId(types.records.len() as u32);
        types.records.push(RecordType {
            body,
            kind,
            members: BTreeMap::new(),
        });
        types.record_by_body.insert(definition, id);
        if let Some(name) = name {
            let scope_hi =
                enclosing_record_scope_hi(arena, token_spans, specifier_span).unwrap_or(u32::MAX);
            types
                .named_records
                .entry((kind, name))
                .or_default()
                .push(RecordBinding {
                    introduced: definition.lo,
                    scope_hi,
                    record: id,
                });
        }
    }
}

fn enclosing_record_scope_hi(
    arena: &crate::syntax::tree::Arena,
    token_spans: &[Span],
    declaration: Span,
) -> Option<u32> {
    arena
        .preorder_roots()
        .filter(|node| {
            matches!(
                arena.tag(*node).and_then(NodeTag::from_u16),
                Some(NodeTag::CompoundStmt | NodeTag::StructBody)
            )
        })
        .filter_map(|node| arena.span(node, token_spans))
        .filter(|scope| scope.lo <= declaration.lo && declaration.hi <= scope.hi)
        .min_by_key(|scope| scope.hi.saturating_sub(scope.lo))
        .map(|scope| scope.hi)
}

fn record_header(
    tree: &Tree,
    text: &str,
    specifiers: NodeId,
    body: NodeId,
) -> Option<(RecordKind, Option<String>)> {
    let (start, _) = tree.arena().token_extent(specifiers)?;
    let (body_start, _) = tree.arena().token_extent(body)?;
    let mut header = None;
    for index in start..body_start {
        let token = TokenId::new(index);
        let kind = TokenKind::from_u16(tree.tokens().kind(token))?;
        if matches!(kind, TokenKind::KwStruct | TokenKind::KwUnion) {
            let record_kind = if kind == TokenKind::KwStruct {
                RecordKind::Struct
            } else {
                RecordKind::Union
            };
            let next = TokenId::new(index + 1);
            let name = (index + 1 < body_start
                && tree.tokens().kind(next) == TokenKind::Identifier.as_u16())
            .then(|| tree.tokens().text(next, text).trim().to_owned());
            header = Some((record_kind, name));
        }
    }
    header
}

fn record_of_declaration(
    types: &FunctionTypes,
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    declaration: NodeId,
    at: u32,
) -> Option<RecordId> {
    let specifiers = tree
        .arena()
        .children_iter(declaration)
        .find(|node| tree.arena().tag(*node) == Some(NodeTag::DeclSpecifiers.as_u16()))?;
    if let Some(body) = tree
        .arena()
        .children_iter(specifiers)
        .find(|node| tree.arena().tag(*node) == Some(NodeTag::StructBody.as_u16()))
    {
        let span = tree.arena().span(body, token_spans)?;
        return types.record_by_body.get(&span).copied();
    }
    let (start, end) = tree.arena().token_extent(specifiers)?;
    for index in start..end {
        let token = TokenId::new(index);
        let kind = TokenKind::from_u16(tree.tokens().kind(token))?;
        let record_kind = match kind {
            TokenKind::KwStruct => RecordKind::Struct,
            TokenKind::KwUnion => RecordKind::Union,
            _ => continue,
        };
        let name_token = TokenId::new(index + 1);
        if index + 1 >= end || tree.tokens().kind(name_token) != TokenKind::Identifier.as_u16() {
            return None;
        }
        let name = tree.tokens().text(name_token, text).trim().to_owned();
        return types
            .named_records
            .get(&(record_kind, name))?
            .iter()
            .rev()
            .find_map(|binding| {
                (binding.introduced <= at && at < binding.scope_hi).then_some(binding.record)
            });
    }
    None
}

fn named_record_in_specifiers(
    types: &FunctionTypes,
    specifiers: &str,
    at: u32,
) -> Option<RecordId> {
    let words = specifiers.split_whitespace().collect::<Vec<_>>();
    for pair in words.windows(2) {
        let kind = match pair[0] {
            "struct" => RecordKind::Struct,
            "union" => RecordKind::Union,
            _ => continue,
        };
        let name = pair[1]
            .trim_matches(|character: char| !character.is_ascii_alphanumeric() && character != '_');
        return types
            .named_records
            .get(&(kind, name.to_owned()))?
            .iter()
            .rev()
            .find_map(|binding| {
                (binding.introduced <= at && at < binding.scope_hi).then_some(binding.record)
            });
    }
    None
}

fn populate_record_members(
    types: &mut FunctionTypes,
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    resolution: &FunctionResolution,
    symbols: &TranslationUnitSymbols,
    alias_targets: &BTreeMap<Span, TypeId>,
) {
    let records = types
        .records
        .iter()
        .enumerate()
        .map(|(index, record)| (RecordId(index as u32), record.body))
        .collect::<Vec<_>>();
    for (record_id, body) in records {
        for declaration in tree.arena().children_iter(body) {
            if tree.arena().tag(declaration) != Some(NodeTag::MemberDecl.as_u16()) {
                continue;
            }
            let specifiers = declaration_specifiers(tree, text, token_spans, declaration);
            for declarator in tree.arena().children_iter(declaration) {
                if tree.arena().tag(declarator) != Some(NodeTag::Declarator.as_u16()) {
                    continue;
                }
                let Some(name_span) = declarator_name_span(tree, token_spans, declarator) else {
                    continue;
                };
                let Some(name) = text.get(name_span.range()).map(str::to_owned) else {
                    continue;
                };
                let nested_record = record_of_declaration(
                    types,
                    tree,
                    text,
                    token_spans,
                    declaration,
                    name_span.lo,
                );
                let base = resolved_base(
                    types,
                    &specifiers,
                    name_span,
                    resolution,
                    symbols,
                    alias_targets,
                    nested_record,
                );
                let ty = apply_operators(
                    types,
                    base,
                    declarator_operators(tree, text, token_spans, declarator),
                    name_span,
                );
                if let Some(record) = types.records.get_mut(record_id.0 as usize) {
                    record.members.insert(name, ty);
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
enum DeclaratorOperator {
    Pointer(Qualifiers),
    Array(ParsedArrayBound),
    Function,
}

#[derive(Debug, Clone)]
enum ParsedArrayBound {
    Constant(String),
    Runtime(Span),
    Incomplete,
    PrototypeStar,
}

/// Resolve local declarators into an ordered type graph.
pub(crate) fn resolve_types(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    resolution: &FunctionResolution,
    symbols: &TranslationUnitSymbols,
    function_offset: u32,
) -> FunctionTypes {
    let arena = tree.arena();
    let mut types = FunctionTypes::default();
    collect_record_definitions(&mut types, tree, text, token_spans, root, function_offset);
    let mut alias_targets = BTreeMap::new();
    // Materialize visible file typedefs first so parameters and locals can
    // point at the same declaration identities and target graph.
    for &declaration in arena.roots() {
        if arena.tag(declaration) != Some(NodeTag::Decl.as_u16())
            || arena
                .span(declaration, token_spans)
                .is_none_or(|span| span.lo >= function_offset)
        {
            continue;
        }
        let specifiers = declaration_specifiers(tree, text, token_spans, declaration);
        for declarator in arena.children_iter(declaration) {
            if arena.tag(declarator) != Some(NodeTag::Declarator.as_u16()) {
                continue;
            }
            let Some(name_span) = declarator_name_span(tree, token_spans, declarator) else {
                continue;
            };
            if !symbols.is_typedef_declaration(name_span) {
                continue;
            }
            let record =
                record_of_declaration(&types, tree, text, token_spans, declaration, name_span.lo);
            let base = resolved_base(
                &mut types,
                &specifiers,
                name_span,
                resolution,
                symbols,
                &alias_targets,
                record,
            );
            let ty = apply_operators(
                &mut types,
                base,
                declarator_operators(tree, text, token_spans, declarator),
                name_span,
            );
            types.by_declaration.insert(
                name_span,
                DeclaredType {
                    written: ty,
                    adjusted: ty,
                    spelling: specifiers.clone(),
                },
            );
            alias_targets.insert(name_span, ty);
        }
    }
    let parameters =
        parameter_declarations(tree, text, token_spans, root, symbols, function_offset);
    for parameter in parameters.named() {
        let (operators, declarator_start) = parameter_operators(tree, text, token_spans, parameter);
        let specifiers = (parameter.group_start..declarator_start)
            .map(|index| tree.tokens().text(TokenId::new(index), text).trim())
            .filter(|word| !word.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let record = named_record_in_specifiers(&types, &specifiers, parameter.span.lo);
        let base = resolved_base(
            &mut types,
            &specifiers,
            parameter.span,
            resolution,
            symbols,
            &alias_targets,
            record,
        );
        let written = apply_operators(&mut types, base, operators, parameter.span);
        let adjusted = match types.node(written).cloned() {
            Some(TypeNode::Array { element, .. }) => {
                types.push(TypeNode::Pointer { pointee: element })
            }
            Some(TypeNode::Function { .. }) => types.push(TypeNode::Pointer { pointee: written }),
            _ => written,
        };
        types.by_declaration.insert(
            parameter.span,
            DeclaredType {
                written,
                adjusted,
                spelling: specifiers,
            },
        );
    }
    for declaration in arena.preorder(root) {
        if arena.tag(declaration) != Some(NodeTag::Decl.as_u16()) {
            continue;
        }
        let specifiers = declaration_specifiers(tree, text, token_spans, declaration);

        for declarator in arena.children_iter(declaration) {
            if arena.tag(declarator) != Some(NodeTag::Declarator.as_u16()) {
                continue;
            }
            let Some(name_span) = declarator_name_span(tree, token_spans, declarator) else {
                continue;
            };
            if resolution.declaration_at(name_span).is_none() {
                continue;
            }
            let record =
                record_of_declaration(&types, tree, text, token_spans, declaration, name_span.lo);
            let mut ty = resolved_base(
                &mut types,
                &specifiers,
                name_span,
                resolution,
                symbols,
                &alias_targets,
                record,
            );
            ty = apply_operators(
                &mut types,
                ty,
                declarator_operators(tree, text, token_spans, declarator),
                name_span,
            );
            types.by_declaration.insert(
                name_span,
                DeclaredType {
                    written: ty,
                    adjusted: ty,
                    spelling: specifiers.clone(),
                },
            );
            if resolution
                .declaration_at(name_span)
                .is_some_and(|declaration| declaration.kind == SymbolKind::Typedef)
            {
                alias_targets.insert(name_span, ty);
            }
        }
    }
    populate_record_members(
        &mut types,
        tree,
        text,
        token_spans,
        resolution,
        symbols,
        &alias_targets,
    );
    types.resolve_bound_inputs(tree, text, token_spans, resolution);
    types.record_alias_graph_issues();
    types
}

impl FunctionTypes {
    fn resolve_bound_inputs(
        &mut self,
        tree: &Tree,
        text: &str,
        token_spans: &[Span],
        resolution: &FunctionResolution,
    ) {
        for slot in &mut self.bound_slots {
            for (index, span) in token_spans.iter().copied().enumerate() {
                if span.lo < slot.expression.lo || span.hi > slot.expression.hi {
                    continue;
                }
                let token = TokenId::new(index as u32);
                if tree.tokens().kind(token) != TokenKind::Identifier.as_u16() {
                    continue;
                }
                let name = tree.tokens().text(token, text).trim();
                let Some(declaration) = resolution
                    .resolve_at(name, span.lo)
                    .filter(|declaration| declaration.kind == SymbolKind::Value)
                else {
                    continue;
                };
                if !slot.input_declarations.contains(&declaration.span) {
                    slot.input_declarations.push(declaration.span);
                }
            }
        }
    }

    fn record_alias_graph_issues(&mut self) {
        let declarations = self
            .by_declaration
            .iter()
            .map(|(span, declared)| (*span, declared.written))
            .collect::<Vec<_>>();
        for (span, mut current) in declarations {
            let mut visited = BTreeSet::new();
            loop {
                if !visited.insert(current) {
                    let reason = UnknownTypeReason::AliasCycle(span);
                    let issue = TypeIssue { reason, span };
                    if !self.issues.contains(&issue) {
                        self.issues.push(issue);
                    }
                    break;
                }
                match self.node(current) {
                    Some(TypeNode::Qualified { unqualified, .. }) => current = *unqualified,
                    Some(TypeNode::Alias {
                        target: Some(target),
                        ..
                    }) => current = *target,
                    Some(TypeNode::Typeof {
                        captured: Some(target),
                        ..
                    }) => current = *target,
                    _ => break,
                }
            }
        }
    }
}

fn declaration_specifiers(
    tree: &Tree,
    text: &str,
    _token_spans: &[Span],
    declaration: NodeId,
) -> String {
    let Some(specifiers) = tree
        .arena()
        .children_iter(declaration)
        .find(|child| tree.arena().tag(*child) == Some(NodeTag::DeclSpecifiers.as_u16()))
    else {
        return String::new();
    };
    let Some((start, end)) = tree.arena().token_extent(specifiers) else {
        return String::new();
    };
    let excluded = tree
        .arena()
        .children_iter(specifiers)
        .filter(|child| {
            matches!(
                tree.arena().tag(*child).and_then(NodeTag::from_u16),
                Some(NodeTag::StructBody | NodeTag::EnumBody)
            )
        })
        .filter_map(|child| tree.arena().token_extent(child))
        .collect::<Vec<_>>();
    (start..end)
        .filter(|index| {
            !excluded
                .iter()
                .any(|(body_start, body_end)| *body_start <= *index && *index < *body_end)
        })
        .map(|index| tree.tokens().text(TokenId::new(index), text).trim())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn declarator_name_span(tree: &Tree, token_spans: &[Span], declarator: NodeId) -> Option<Span> {
    tree.arena()
        .preorder(declarator)
        .find(|node| tree.arena().tag(*node) == Some(NodeTag::DeclName.as_u16()))
        .and_then(|node| tree.arena().span(node, token_spans))
}

fn resolved_base(
    types: &mut FunctionTypes,
    specifiers: &str,
    declaration_span: Span,
    resolution: &FunctionResolution,
    symbols: &TranslationUnitSymbols,
    alias_targets: &BTreeMap<Span, TypeId>,
    record: Option<RecordId>,
) -> TypeId {
    if let Some(record) = record {
        let base = types.push(TypeNode::Record { record });
        return qualify(types, base, qualifiers_in(specifiers));
    }
    if let Some(name) = simple_typeof_operand(specifiers) {
        let operand = resolution
            .resolve_at(name, declaration_span.lo)
            .filter(|declaration| declaration.kind == SymbolKind::Value)
            .map(|declaration| declaration.span);
        let captured = operand.and_then(|span| types.type_of_declaration(span));
        if let (Some(operand), Some(captured)) = (operand, captured) {
            let base = types.push(TypeNode::Typeof {
                operand,
                captured: Some(captured),
            });
            return qualify(types, base, qualifiers_in(specifiers));
        }
        let reason = UnknownTypeReason::UnresolvedTypeofOperand(name.to_owned());
        types.issues.push(TypeIssue {
            reason: reason.clone(),
            span: declaration_span,
        });
        let base = types.push(TypeNode::Unknown { reason });
        return qualify(types, base, qualifiers_in(specifiers));
    }

    let alias = specifiers.split_whitespace().find_map(|word| {
        if let Some(declaration) = resolution.resolve_at(word, declaration_span.lo) {
            return (declaration.kind == SymbolKind::Typedef).then_some(declaration.span);
        }
        symbols.visible_typedef_declaration(word, declaration_span.lo)
    });
    let base = if let Some(declaration) = alias {
        let target = alias_targets.get(&declaration).copied();
        if target.is_none() {
            types.issues.push(TypeIssue {
                reason: UnknownTypeReason::MissingAliasTarget(declaration),
                span: declaration_span,
            });
        }
        types.push(TypeNode::Alias {
            declaration,
            target,
        })
    } else {
        let base = strip_type_qualifiers(specifiers);
        if is_known_base(&base) {
            types.push(TypeNode::SpelledBase { specifiers: base })
        } else {
            let reason = if base.is_empty() {
                UnknownTypeReason::MissingSpecifier
            } else {
                UnknownTypeReason::UnresolvedName(base)
            };
            types.issues.push(TypeIssue {
                reason: reason.clone(),
                span: declaration_span,
            });
            types.push(TypeNode::Unknown { reason })
        }
    };
    qualify(types, base, qualifiers_in(specifiers))
}

fn simple_typeof_operand(specifiers: &str) -> Option<&str> {
    let source = specifiers.trim();
    let open = source.find('(')?;
    let keyword = source[..open].trim();
    if !matches!(keyword, "typeof" | "__typeof" | "__typeof__") || !source.ends_with(')') {
        return None;
    }
    let mut operand = source[open + 1..source.len() - 1].trim();
    while let Some(inner) = redundant_parentheses_inner(operand) {
        operand = inner.trim();
    }
    (!operand.is_empty()
        && operand
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
        && operand
            .chars()
            .next()
            .is_some_and(|character| character == '_' || character.is_ascii_alphabetic()))
    .then_some(operand)
}

fn redundant_parentheses_inner(source: &str) -> Option<&str> {
    if !source.starts_with('(') || !source.ends_with(')') {
        return None;
    }
    let mut depth = 0u32;
    for (index, character) in source.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 && index + character.len_utf8() != source.len() {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then(|| &source[1..source.len() - 1])
}

fn is_known_base(specifiers: &str) -> bool {
    let trimmed = specifiers.trim();
    if [
        "struct ",
        "union ",
        "enum ",
        "typeof(",
        "typeof (",
        "__typeof(",
        "__typeof__ (",
        "_Atomic(",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
    {
        return true;
    }
    let mut saw_type = false;
    for word in specifiers.split_whitespace() {
        match word {
            "typedef" | "extern" | "static" | "auto" | "register" | "inline" | "_Noreturn" => {}
            "signed" | "unsigned" | "short" | "long" | "_Complex" => saw_type = true,
            "void" | "char" | "int" | "float" | "double" | "_Bool" | "__int128" => {
                saw_type = true;
            }
            _ => return false,
        }
    }
    saw_type
}

fn apply_operators(
    types: &mut FunctionTypes,
    mut ty: TypeId,
    operators: Vec<DeclaratorOperator>,
    declaration: Span,
) -> TypeId {
    for operator in operators.into_iter().rev() {
        ty = match operator {
            DeclaratorOperator::Pointer(qualifiers) => {
                let pointer = types.push(TypeNode::Pointer { pointee: ty });
                qualify(types, pointer, qualifiers)
            }
            DeclaratorOperator::Array(bound) => {
                let bound = match bound {
                    ParsedArrayBound::Constant(value) => ArrayBound::Constant(value),
                    ParsedArrayBound::Runtime(expression) => {
                        let id = BoundSlotId(types.bound_slots.len() as u32);
                        types.bound_slots.push(BoundSlot {
                            declaration,
                            expression,
                            input_declarations: Vec::new(),
                        });
                        ArrayBound::Runtime(id)
                    }
                    ParsedArrayBound::Incomplete => ArrayBound::Incomplete,
                    ParsedArrayBound::PrototypeStar => ArrayBound::PrototypeStar,
                };
                types.push(TypeNode::Array { element: ty, bound })
            }
            DeclaratorOperator::Function => types.push(TypeNode::Function { result: ty }),
        };
    }
    ty
}

fn qualify(types: &mut FunctionTypes, ty: TypeId, qualifiers: Qualifiers) -> TypeId {
    if qualifiers.is_empty() {
        ty
    } else {
        types.push(TypeNode::Qualified {
            unqualified: ty,
            qualifiers,
        })
    }
}

fn qualifiers_in(specifiers: &str) -> Qualifiers {
    let mut qualifiers = Qualifiers::default();
    for word in specifiers.split_whitespace() {
        match word {
            "const" => qualifiers.is_const = true,
            "volatile" => qualifiers.is_volatile = true,
            "restrict" | "__restrict" | "__restrict__" => qualifiers.is_restrict = true,
            "_Atomic" => qualifiers.is_atomic = true,
            _ => {}
        }
    }
    qualifiers
}

fn strip_type_qualifiers(specifiers: &str) -> String {
    specifiers
        .split_whitespace()
        .filter(|word| {
            !matches!(
                *word,
                "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Atomic"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Recover parameter operator precedence inside its parser-owned token group.
fn parameter_operators(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    parameter: &ParameterDeclarator,
) -> (Vec<DeclaratorOperator>, u32) {
    let tokens = tree.tokens();
    let spelling = |index| tokens.text(TokenId::new(index), text).trim();
    let mut current_start = parameter.group_start;
    let mut current_end = parameter.group_end;
    let mut deferred: Vec<(Vec<DeclaratorOperator>, Vec<Qualifiers>)> = Vec::new();
    let mut declarator_start = parameter.name_index;
    loop {
        let mut depth = 0u32;
        let mut core_start = parameter.name_index;
        let mut core_end = parameter.name_index + 1;
        let mut containing_open = None;
        let mut index = current_start;
        while index < current_end {
            match spelling(index) {
                "(" => {
                    if depth == 0 && index < parameter.name_index {
                        containing_open = Some(index);
                    }
                    depth += 1;
                }
                ")" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0
                        && containing_open.is_some_and(|open| open < parameter.name_index)
                        && parameter.name_index < index
                    {
                        core_start = containing_open.expect("checked");
                        core_end = index + 1;
                        break;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        let pointer_start = (current_start..core_start).find(|index| spelling(*index) == "*");
        declarator_start = declarator_start.min(pointer_start.unwrap_or(core_start));
        let mut pointers = (current_start..core_start)
            .filter(|index| spelling(*index) == "*")
            .map(|index| {
                let mut words = String::new();
                let mut qualifier = index + 1;
                while qualifier < core_start
                    && matches!(
                        spelling(qualifier),
                        "const"
                            | "volatile"
                            | "restrict"
                            | "__restrict"
                            | "__restrict__"
                            | "_Atomic"
                    )
                {
                    words.push(' ');
                    words.push_str(spelling(qualifier));
                    qualifier += 1;
                }
                qualifiers_in(&words)
            })
            .collect::<Vec<_>>();
        pointers.reverse();
        let mut suffixes = Vec::new();
        let mut suffix = core_end;
        while suffix < current_end {
            let opener = spelling(suffix);
            if !matches!(opener, "[" | "(") {
                suffix += 1;
                continue;
            }
            let closer = if opener == "[" { "]" } else { ")" };
            let mut nested = 1u32;
            let mut end = suffix + 1;
            while end < current_end && nested > 0 {
                if spelling(end) == opener {
                    nested += 1;
                } else if spelling(end) == closer {
                    nested -= 1;
                }
                end += 1;
            }
            if nested != 0 {
                break;
            }
            if opener == "[" {
                suffixes.push(DeclaratorOperator::Array(array_bound_range(
                    tree,
                    text,
                    token_spans,
                    suffix,
                    end,
                )));
            } else {
                suffixes.push(DeclaratorOperator::Function);
            }
            suffix = end;
        }
        deferred.push((suffixes, pointers));
        if core_start == parameter.name_index {
            break;
        }
        current_start = core_start + 1;
        current_end = core_end - 1;
    }
    let mut operators = Vec::new();
    while let Some((suffixes, pointers)) = deferred.pop() {
        operators.extend(suffixes);
        operators.extend(pointers.into_iter().map(DeclaratorOperator::Pointer));
    }
    (operators, declarator_start)
}

/// Operators encountered from the declared name outwards.
fn declarator_operators(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    declarator: NodeId,
) -> Vec<DeclaratorOperator> {
    let arena = tree.arena();
    let mut deferred: Vec<(Vec<DeclaratorOperator>, Vec<Qualifiers>)> = Vec::new();
    let mut current = declarator;
    let mut out = Vec::new();
    loop {
        let children: Vec<_> = arena.children_iter(current).collect();
        let core = children.iter().position(|child| {
            matches!(
                arena.tag(*child).and_then(NodeTag::from_u16),
                Some(NodeTag::DeclName | NodeTag::ParenthesizedDeclarator)
            )
        });
        let Some(core) = core else {
            break;
        };
        let mut pointers = children[..core]
            .iter()
            .filter(|child| arena.tag(**child) == Some(NodeTag::PointerOperator.as_u16()))
            .map(|child| pointer_qualifiers(tree, text, *child))
            .collect::<Vec<_>>();
        pointers.reverse();
        let suffixes = children[core + 1..]
            .iter()
            .filter_map(
                |child| match arena.tag(*child).and_then(NodeTag::from_u16) {
                    Some(NodeTag::ArraySuffix) => Some(DeclaratorOperator::Array(array_bound(
                        tree,
                        text,
                        token_spans,
                        *child,
                    ))),
                    Some(NodeTag::ParamList) => Some(DeclaratorOperator::Function),
                    _ => None,
                },
            )
            .collect();
        deferred.push((suffixes, pointers));
        if arena.tag(children[core]) == Some(NodeTag::ParenthesizedDeclarator.as_u16()) {
            current = children[core];
        } else {
            break;
        }
    }
    while let Some((suffixes, pointers)) = deferred.pop() {
        out.extend(suffixes);
        out.extend(pointers.into_iter().map(DeclaratorOperator::Pointer));
    }
    out
}

fn pointer_qualifiers(tree: &Tree, text: &str, node: NodeId) -> Qualifiers {
    let Some((first, end)) = tree.arena().token_extent(node) else {
        return Qualifiers::default();
    };
    let words = (first..end)
        .map(|index| tree.tokens().text(TokenId::new(index), text).trim())
        .collect::<Vec<_>>()
        .join(" ");
    qualifiers_in(&words)
}

fn array_bound(tree: &Tree, text: &str, token_spans: &[Span], node: NodeId) -> ParsedArrayBound {
    let arena = tree.arena();
    let Some((first, end)) = arena.token_extent(node) else {
        return ParsedArrayBound::Incomplete;
    };
    array_bound_range(tree, text, token_spans, first, end)
}

fn array_bound_range(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    first: u32,
    end: u32,
) -> ParsedArrayBound {
    if end <= first + 1 {
        return ParsedArrayBound::Incomplete;
    }
    let inner_start = first + 1;
    let inner_end = end.saturating_sub(1);
    let spelling = (inner_start..inner_end)
        .map(|index| tree.tokens().text(TokenId::new(index), text).trim())
        .collect::<String>();
    if spelling.is_empty() {
        return ParsedArrayBound::Incomplete;
    }
    if spelling == "*" {
        return ParsedArrayBound::PrototypeStar;
    }
    if spelling.chars().all(|character| character.is_ascii_digit()) {
        return ParsedArrayBound::Constant(spelling);
    }
    let lo = token_spans.get(inner_start as usize).map(|span| span.lo);
    let hi = token_spans
        .get(inner_end.saturating_sub(1) as usize)
        .map(|span| span.hi);
    match (lo, hi) {
        (Some(lo), Some(hi)) => ParsedArrayBound::Runtime(Span::new(lo, hi)),
        _ => ParsedArrayBound::Incomplete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::parse::parse;
    use crate::csource::semantic::declarations::{resolve_function, TranslationUnitSymbols};

    #[test]
    fn pointer_array_precedence_produces_different_type_graphs() {
        let text = "int f(int n) { int *a[4]; int (*p)[4]; int v[n]; return n; }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let declaration = |name: &str| {
            resolution
                .resolve_at(name, text.find("return").unwrap() as u32)
                .expect("declaration")
                .span
        };

        let a = types.type_of_declaration(declaration("a")).expect("a type");
        let TypeNode::Array { element, .. } = types.node(a).expect("a node") else {
            panic!("a is an array")
        };
        assert!(matches!(
            types.node(*element),
            Some(TypeNode::Pointer { .. })
        ));

        let p = types.type_of_declaration(declaration("p")).expect("p type");
        let TypeNode::Pointer { pointee } = types.node(p).expect("p node") else {
            panic!("p is a pointer")
        };
        assert!(matches!(types.node(*pointee), Some(TypeNode::Array { .. })));

        let v_declaration = declaration("v");
        let v = types.type_of_declaration(v_declaration).expect("v type");
        assert!(matches!(
            types.node(v),
            Some(TypeNode::Array {
                bound: ArrayBound::Runtime(_),
                ..
            })
        ));
        let slots = types.runtime_bound_slots_of_declaration(v_declaration);
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].0, BoundSlotId(0));
        assert_eq!(slots[0].1.declaration, v_declaration);
        assert_eq!(&text[slots[0].1.expression.range()], "n");
    }

    #[test]
    fn record_members_have_identity_and_structural_declarator_types() {
        let text = concat!(
            "struct Node { int values[4]; struct Node *next; };",
            "int f(struct Node *node) { return node->values[0]; }"
        );
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let node = resolution
            .resolve_at("node", function.span.hi)
            .expect("parameter");
        let node_type = types
            .adjusted_type_of_declaration(node.span)
            .expect("parameter type");
        let values = types
            .member_type(node_type, true, "values")
            .expect("array member");
        assert!(types.type_is_array(values));
        let next = types
            .member_type(node_type, true, "next")
            .expect("self pointer member");
        let TypeNode::Pointer { pointee } = types.node(next).expect("next type") else {
            panic!("next is a pointer")
        };
        assert!(matches!(
            types.node(*pointee),
            Some(TypeNode::Record { .. })
        ));
    }

    #[test]
    fn block_record_tags_stop_shadowing_at_their_lexical_scope() {
        let text = concat!(
            "struct S { int outer[2]; };",
            "int f(void) { { struct S { int inner; }; struct S local; local.inner=1; }",
            "struct S after; return after.outer[0]; }"
        );
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let local = resolution
            .resolve_at("local", text.find("local.inner").expect("local use") as u32)
            .expect("local declaration");
        let local_type = types
            .adjusted_type_of_declaration(local.span)
            .expect("local type");
        assert!(types.member_type(local_type, false, "inner").is_some());
        assert!(types.member_type(local_type, false, "outer").is_none());

        let after = resolution
            .resolve_at("after", text.find("after.outer").expect("after use") as u32)
            .expect("after declaration");
        let after_type = types
            .adjusted_type_of_declaration(after.span)
            .expect("after type");
        assert!(types.member_type(after_type, false, "inner").is_none());
        let outer = types
            .member_type(after_type, false, "outer")
            .expect("outer member restored");
        assert!(types.type_is_array(outer));
    }

    #[test]
    fn parameters_retain_written_types_and_explicit_body_adjustment() {
        let text = "int f(int matrix[4][8], int callback(int), int (*pointer)(int)) { return 0; }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let parameter = |name: &str| {
            resolution
                .resolve_at(name, text.find('{').unwrap() as u32)
                .expect("parameter")
                .span
        };

        let matrix = parameter("matrix");
        let written = types.type_of_declaration(matrix).expect("written matrix");
        assert!(matches!(types.node(written), Some(TypeNode::Array { .. })));
        let adjusted = types
            .adjusted_type_of_declaration(matrix)
            .expect("adjusted matrix");
        let TypeNode::Pointer { pointee } = types.node(adjusted).expect("adjusted node") else {
            panic!("an array parameter adjusts to pointer")
        };
        assert!(matches!(types.node(*pointee), Some(TypeNode::Array { .. })));

        let callback = parameter("callback");
        assert!(matches!(
            types.node(
                types
                    .type_of_declaration(callback)
                    .expect("written callback")
            ),
            Some(TypeNode::Function { .. })
        ));
        assert!(types.adjusted_is_pointer(callback));

        let pointer = parameter("pointer");
        let written_pointer = types.type_of_declaration(pointer).expect("written pointer");
        assert_eq!(
            types.adjusted_type_of_declaration(pointer),
            Some(written_pointer),
            "a function-pointer parameter is already adjusted"
        );
    }

    #[test]
    fn qualifiers_belong_to_their_exact_type_layer() {
        let text = "int f(void) { const int *pointer; int *const fixed; volatile int *const volatile both; return 0; }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let declared = |name: &str| {
            let span = resolution
                .resolve_at(name, text.find("return").unwrap() as u32)
                .expect("declaration")
                .span;
            types.type_of_declaration(span).expect("type")
        };

        let TypeNode::Pointer { pointee } = types.node(declared("pointer")).expect("pointer")
        else {
            panic!("pointer itself is unqualified")
        };
        assert!(matches!(
            types.node(*pointee),
            Some(TypeNode::Qualified {
                qualifiers: Qualifiers { is_const: true, .. },
                ..
            })
        ));

        let TypeNode::Qualified {
            unqualified,
            qualifiers,
        } = types.node(declared("fixed")).expect("fixed")
        else {
            panic!(
                "fixed pointer is qualified: {:?}",
                types.node(declared("fixed"))
            )
        };
        assert!(qualifiers.is_const);
        assert!(matches!(
            types.node(*unqualified),
            Some(TypeNode::Pointer { .. })
        ));

        let TypeNode::Qualified {
            unqualified,
            qualifiers,
        } = types.node(declared("both")).expect("both")
        else {
            panic!("both pointer is qualified")
        };
        assert!(qualifiers.is_const && qualifiers.is_volatile);
        let TypeNode::Pointer { pointee } = types.node(*unqualified).expect("both pointer") else {
            panic!("qualified layer wraps pointer")
        };
        assert!(matches!(
            types.node(*pointee),
            Some(TypeNode::Qualified {
                qualifiers: Qualifiers {
                    is_volatile: true,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn local_aliases_retain_identity_and_follow_targets_iteratively() {
        let text = "int f(void) { typedef int *P; P p; typedef P Q; Q q; return sizeof(q); }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let at_return = text.find("return").unwrap() as u32;
        for name in ["p", "q"] {
            let declaration = resolution.resolve_at(name, at_return).expect("declaration");
            let id = types
                .type_of_declaration(declaration.span)
                .expect("declared type");
            assert!(matches!(types.node(id), Some(TypeNode::Alias { .. })));
            assert!(types.adjusted_is_pointer(declaration.span));
            let shape = types
                .shape_of_declaration(declaration.span)
                .expect("compatibility shape");
            assert_eq!(shape.pointer_depth, 0, "the written spelling has no star");
        }
    }

    #[test]
    fn malformed_alias_cycles_terminate_without_guessing_a_shape() {
        let span = Span::new(1, 2);
        let mut types = FunctionTypes::default();
        let first = types.push(TypeNode::Alias {
            declaration: Span::new(3, 4),
            target: Some(TypeId(1)),
        });
        types.push(TypeNode::Alias {
            declaration: Span::new(5, 6),
            target: Some(first),
        });
        types.by_declaration.insert(
            span,
            DeclaredType {
                written: first,
                adjusted: first,
                spelling: "Broken".to_owned(),
            },
        );

        assert!(!types.adjusted_is_pointer(span));
        types.record_alias_graph_issues();
        assert!(types.issues().iter().any(|issue| {
            issue.span == span && matches!(issue.reason, UnknownTypeReason::AliasCycle(_))
        }));
    }

    #[test]
    fn unresolved_parameter_type_is_an_explicit_unknown() {
        let text = "int f(HeaderType value) { return 0; }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let value = resolution
            .resolve_at("value", text.find("return").unwrap() as u32)
            .expect("parameter");
        let ty = types.type_of_declaration(value.span).expect("type node");
        assert!(matches!(
            types.node(ty),
            Some(TypeNode::Unknown {
                reason: UnknownTypeReason::UnresolvedName(name)
            }) if name == "HeaderType"
        ));
        assert_eq!(types.issues().len(), 1);
        assert_eq!(types.issues()[0].span, value.span);
    }

    #[test]
    fn typeof_value_capture_reuses_runtime_bound_identity() {
        let text = "int f(int n) { int a[n]; typeof(a) b; typeof(b) c; return sizeof(c); }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let at_return = text.find("return").unwrap() as u32;
        let declaration = |name| resolution.resolve_at(name, at_return).expect(name).span;
        let a = declaration("a");
        let b = declaration("b");
        let c = declaration("c");
        let a_slots = types.runtime_bound_slots_of_declaration(a);
        let b_slots = types.runtime_bound_slots_of_declaration(b);
        let c_slots = types.runtime_bound_slots_of_declaration(c);
        assert_eq!(a_slots.len(), 1);
        assert_eq!(a_slots[0].0, b_slots[0].0);
        assert_eq!(b_slots[0].0, c_slots[0].0);
        assert_eq!(c_slots[0].1.declaration, a);
        assert_eq!(&text[c_slots[0].1.expression.range()], "n");
        assert!(matches!(
            types.node(types.type_of_declaration(c).expect("c type")),
            Some(TypeNode::Typeof {
                operand,
                captured: Some(_)
            }) if *operand == b
        ));
    }

    #[test]
    fn file_alias_chains_are_owned_once_and_respect_source_order() {
        let text = "typedef int *P; typedef P Q; int f(Q parameter) { Q local; return sizeof(local); } typedef int *Later;";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let resolution = resolve_function(
            &tree,
            text,
            &spans,
            function.node,
            function.span,
            function.name_span,
            &symbols,
        );
        let types = resolve_types(
            &tree,
            text,
            &spans,
            function.node,
            &resolution,
            &symbols,
            function.span.lo,
        );
        let at_return = text.find("return").unwrap() as u32;
        for name in ["parameter", "local"] {
            let declaration = resolution.resolve_at(name, at_return).expect("declaration");
            assert!(matches!(
                types.node(types.type_of_declaration(declaration.span).expect("type")),
                Some(TypeNode::Alias {
                    target: Some(_),
                    ..
                })
            ));
            assert!(types.adjusted_is_pointer(declaration.span));
        }
        assert!(symbols
            .visible_typedef_declaration("Later", function.span.lo)
            .is_none());
    }
}
