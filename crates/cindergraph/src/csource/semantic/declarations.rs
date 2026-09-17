//! Shared declaration identities recovered from the tolerant syntax tree.
//!
//! This module is the P2 ownership boundary: dataflow may consume declarations
//! and lexical facts, but it must not rediscover parameter boundaries, typedef
//! visibility, enumerators, or file-scope function identity for itself.

use std::collections::{BTreeMap, BTreeSet};

use crate::csource::lex::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::syntax::ids::{NodeId, Span, TokenId};

/// Dense identity of one declaration within a function's semantic snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SymbolId(pub(crate) u32);

/// Which member of C's ordinary-identifier namespace a declaration denotes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolKind {
    Value,
    Typedef,
    Constant,
}

/// One resolved declaration with an exact C declaration point.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SymbolDeclaration {
    pub(crate) id: SymbolId,
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) kind: SymbolKind,
    declarator_span: Span,
    visible_from: u32,
    scope: u32,
}

#[derive(Debug, Clone)]
struct LexicalScope {
    span: Span,
    parent: Option<u32>,
    depth: u32,
    declarations: Vec<SymbolId>,
}

/// Resolved ordinary-identifier declarations for one function definition.
#[derive(Debug, Clone, Default)]
pub(crate) struct FunctionResolution {
    declarations: Vec<SymbolDeclaration>,
    scopes: Vec<LexicalScope>,
    by_span: BTreeMap<Span, SymbolId>,
    /// How many leading entries of `declarations` are the function's own
    /// parameters; they are pushed first, so the parameters are exactly the
    /// ids below this count.
    parameters: u32,
}

impl FunctionResolution {
    /// Whether `declaration` is one of the function's own parameters.
    pub(crate) fn is_parameter(&self, declaration: &SymbolDeclaration) -> bool {
        declaration.id.0 < self.parameters
    }

    /// Declaration identity carried by this exact declared-name span.
    pub(crate) fn declaration_at(&self, span: Span) -> Option<&SymbolDeclaration> {
        self.by_span
            .get(&span)
            .and_then(|id| self.declarations.get(id.0 as usize))
    }

    /// Innermost visible declaration of `name` at this exact byte offset.
    pub(crate) fn resolve_at(&self, name: &str, offset: u32) -> Option<&SymbolDeclaration> {
        let mut scope = self
            .scopes
            .iter()
            .enumerate()
            .filter(|(_, scope)| scope.span.lo <= offset && offset <= scope.span.hi)
            .max_by_key(|(_, scope)| (scope.depth, u32::MAX - scope.span.len()))
            .map(|(index, _)| index as u32);
        while let Some(scope_id) = scope {
            let current = self.scopes.get(scope_id as usize)?;
            if let Some(found) = current.declarations.iter().rev().find_map(|id| {
                let declaration = self.declarations.get(id.0 as usize)?;
                (declaration.name == name && declaration.visible_from <= offset)
                    .then_some(declaration)
            }) {
                return Some(found);
            }
            scope = current.parent;
        }
        None
    }

    /// Declaration whose declarator structurally owns syntax at `offset`.
    pub(crate) fn declaration_owning(&self, offset: u32) -> Option<&SymbolDeclaration> {
        self.declarations
            .iter()
            .filter(|declaration| {
                declaration.declarator_span.lo <= offset && offset <= declaration.declarator_span.hi
            })
            .min_by_key(|declaration| declaration.declarator_span.len())
    }
}

/// Translation-unit identities needed by lexical semantic recovery.
///
/// Declaration offsets make visibility source ordered. Function declarations
/// and definitions form a set so a parenthesized designator can be recognized
/// without guessing that every free name is a function.
#[derive(Debug, Clone, Default)]
pub(crate) struct TranslationUnitSymbols {
    typedef_symbols: BTreeMap<String, Vec<(u32, Span)>>,
    constants: BTreeMap<String, u32>,
    functions: BTreeSet<String>,
    typedef_declarations: BTreeSet<Span>,
}

#[derive(Debug, Clone)]
struct EnumConstant {
    body_start: u32,
    body_end: u32,
    name: String,
    token: u32,
}

impl TranslationUnitSymbols {
    /// Build the shared file-scope declaration index once.
    pub(crate) fn collect(tree: &Tree, text: &str, token_spans: &[Span]) -> Self {
        let arena = tree.arena();
        let mut typedef_symbols: BTreeMap<String, Vec<(u32, Span)>> = BTreeMap::new();
        let mut constants = BTreeMap::new();
        let mut function_typedefs = BTreeSet::new();
        let enumerators = collect_enumerators(tree, text);
        let typedef_declarations = collect_typedef_declarations(tree, text, token_spans);
        let mut functions: BTreeSet<String> = tree
            .functions(text)
            .into_iter()
            .map(|function| function.name)
            .collect();
        for &root in arena.roots() {
            if arena.tag(root) != Some(NodeTag::Decl.as_u16()) {
                continue;
            }
            let specifiers = arena
                .children_iter(root)
                .find(|child| arena.tag(*child) == Some(NodeTag::DeclSpecifiers.as_u16()));
            if let Some((first, end)) = specifiers.and_then(|node| arena.token_extent(node)) {
                for (name, token) in enumerators_in(&enumerators, first, end) {
                    if let Some(span) = token_spans.get(token as usize) {
                        constants.entry(name.to_owned()).or_insert(span.lo);
                    }
                }
            }
            let is_typedef = specifiers.is_some_and(|node| {
                arena.token_extent(node).is_some_and(|(first, end)| {
                    (first..end).any(|index| {
                        tree.tokens().kind(TokenId::new(index)) == TokenKind::KwTypedef.as_u16()
                    })
                })
            });
            let uses_function_typedef = specifiers.is_some_and(|node| {
                arena.token_extent(node).is_some_and(|(first, end)| {
                    (first..end).any(|index| {
                        let token = TokenId::new(index);
                        tree.tokens().kind(token) == TokenKind::Identifier.as_u16()
                            && function_typedefs.contains(tree.tokens().text(token, text).trim())
                    })
                })
            });
            let Some(offset) = arena.span(root, token_spans).map(|span| span.lo) else {
                continue;
            };
            for declarator in arena.children_iter(root) {
                if arena.tag(declarator) != Some(NodeTag::Declarator.as_u16()) {
                    continue;
                }
                let declared = arena.preorder(declarator).find_map(|node| {
                    (arena.tag(node) == Some(NodeTag::DeclName.as_u16()))
                        .then(|| name_of(tree, text, token_spans, node))
                        .flatten()
                });
                let Some((name, name_span)) = declared else {
                    continue;
                };
                let is_function = declarator_declares_function(tree, declarator)
                    || (uses_function_typedef
                        && declarator_preserves_function_type(tree, declarator));
                if is_typedef {
                    typedef_symbols
                        .entry(name.clone())
                        .or_default()
                        .push((offset, name_span));
                    if is_function {
                        function_typedefs.insert(name);
                    }
                } else if is_function {
                    functions.insert(name);
                }
            }
        }
        Self {
            typedef_symbols,
            constants,
            functions,
            typedef_declarations,
        }
    }

    /// Whether a file-scope typedef is visible before `function_offset`.
    pub(crate) fn typedef_is_visible(&self, name: &str, function_offset: u32) -> bool {
        self.visible_typedef_declaration(name, function_offset)
            .is_some()
    }

    /// Latest file-scope typedef declaration visible at `offset`.
    pub(crate) fn visible_typedef_declaration(&self, name: &str, offset: u32) -> Option<Span> {
        self.typedef_symbols.get(name).and_then(|declarations| {
            declarations
                .iter()
                .rev()
                .find_map(|(declared_at, span)| (*declared_at < offset).then_some(*span))
        })
    }

    /// Whether a file-scope enumerator is visible before `function_offset`.
    pub(crate) fn constant_is_visible(&self, name: &str, function_offset: u32) -> bool {
        self.constants
            .get(name)
            .is_some_and(|offset| *offset < function_offset)
    }

    /// Whether this translation unit declares `name` as a function.
    pub(crate) fn declares_function(&self, name: &str) -> bool {
        self.functions.contains(name)
    }

    /// Whether this exact declaration introduces a typedef name.
    pub(crate) fn is_typedef_declaration(&self, span: Span) -> bool {
        self.typedef_declarations.contains(&span)
    }
}

/// One resolved outer function parameter and its grammar-owned token group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParameterDeclarator {
    pub(crate) name: String,
    pub(crate) span: Span,
    pub(crate) group_start: u32,
    pub(crate) group_end: u32,
    pub(crate) name_index: u32,
}

/// The complete outer parameter-list partition plus its named declarations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ParameterDeclarations {
    groups: Vec<(u32, u32)>,
    named: Vec<ParameterDeclarator>,
}

impl ParameterDeclarations {
    /// Grammar-owned groups, including unnamed parameters.
    pub(crate) fn groups(&self) -> &[(u32, u32)] {
        &self.groups
    }

    /// Parameters that introduce an ordinary-identifier binding.
    pub(crate) fn named(&self) -> &[ParameterDeclarator] {
        &self.named
    }
}

/// Resolve the outer parameter declarations of one function.
pub(crate) fn parameter_declarations(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    symbols: &TranslationUnitSymbols,
    function_offset: u32,
) -> ParameterDeclarations {
    let arena = tree.arena();
    let groups = arena
        .preorder(root)
        .find(|node| arena.tag(*node) == Some(NodeTag::ParamList.as_u16()))
        .map(|list| {
            arena
                .children_iter(list)
                .filter(|child| arena.tag(*child) == Some(NodeTag::ParamDecl.as_u16()))
                .filter_map(|child| arena.token_extent(child))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let named = groups
        .iter()
        .filter_map(|&(start, end)| {
            parameter_in_group(
                tree,
                text,
                token_spans,
                start,
                end,
                symbols,
                function_offset,
            )
        })
        .collect();
    ParameterDeclarations { groups, named }
}

/// Build declaration identity and lexical scope ownership for one function.
pub(crate) fn resolve_function(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    function_span: Span,
    function_name_span: Span,
    symbols: &TranslationUnitSymbols,
) -> FunctionResolution {
    let arena = tree.arena();
    let mut resolution = FunctionResolution {
        scopes: vec![LexicalScope {
            span: function_span,
            parent: None,
            depth: 0,
            declarations: Vec::new(),
        }],
        ..FunctionResolution::default()
    };

    let parameters =
        parameter_declarations(tree, text, token_spans, root, symbols, function_span.lo);
    for parameter in parameters.named() {
        let declarator_span =
            parameter_group_span(token_spans, parameter.group_start, parameter.group_end)
                .unwrap_or(parameter.span);
        resolution.push_declaration(
            0,
            parameter.name.clone(),
            parameter.span,
            declarator_span,
            SymbolKind::Value,
        );
    }
    resolution.parameters = resolution.declarations.len() as u32;

    let mut stack = vec![(root, 0u32, None)];
    while let Some((node, inherited_scope, inherited_declarator)) = stack.pop() {
        let tag = arena.tag(node).and_then(NodeTag::from_u16);
        let declarator = if tag == Some(NodeTag::Declarator) {
            arena.span(node, token_spans).or(inherited_declarator)
        } else {
            inherited_declarator
        };
        let scope = if node != root && matches!(tag, Some(NodeTag::CompoundStmt | NodeTag::ForStmt))
        {
            let span = arena.span(node, token_spans).unwrap_or(function_span);
            let id = resolution.scopes.len() as u32;
            let depth = resolution
                .scopes
                .get(inherited_scope as usize)
                .map_or(0, |parent| parent.depth + 1);
            resolution.scopes.push(LexicalScope {
                span,
                parent: Some(inherited_scope),
                depth,
                declarations: Vec::new(),
            });
            id
        } else {
            inherited_scope
        };

        match tag {
            Some(NodeTag::DeclName) => {
                if let Some((name, span)) = name_of(tree, text, token_spans, node) {
                    if span != function_name_span {
                        let kind = if symbols.is_typedef_declaration(span) {
                            SymbolKind::Typedef
                        } else {
                            SymbolKind::Value
                        };
                        resolution.push_declaration(
                            scope,
                            name,
                            span,
                            declarator.unwrap_or(span),
                            kind,
                        );
                    }
                }
            }
            Some(NodeTag::Enumerator) => {
                if let Some((name, span)) = name_of(tree, text, token_spans, node) {
                    resolution.push_declaration(scope, name, span, span, SymbolKind::Constant);
                }
            }
            _ => {}
        }

        let children: Vec<_> = arena.children_iter(node).collect();
        for child in children.into_iter().rev() {
            stack.push((child, scope, declarator));
        }
    }
    for scope in &mut resolution.scopes {
        scope.declarations.sort_by_key(|id| {
            resolution
                .declarations
                .get(id.0 as usize)
                .map_or(u32::MAX, |declaration| declaration.visible_from)
        });
    }
    resolution
}

impl FunctionResolution {
    fn push_declaration(
        &mut self,
        scope: u32,
        name: String,
        span: Span,
        declarator_span: Span,
        kind: SymbolKind,
    ) {
        if self.by_span.contains_key(&span) {
            return;
        }
        let id = SymbolId(self.declarations.len() as u32);
        self.declarations.push(SymbolDeclaration {
            id,
            name,
            span,
            kind,
            declarator_span,
            visible_from: span.hi,
            scope,
        });
        self.by_span.insert(span, id);
        if let Some(owner) = self.scopes.get_mut(scope as usize) {
            owner.declarations.push(id);
        }
    }
}

fn parameter_group_span(token_spans: &[Span], start: u32, end: u32) -> Option<Span> {
    let first = token_spans.get(start as usize)?;
    let last = token_spans.get(end.checked_sub(1)? as usize)?;
    Some(Span::new(first.lo, last.hi))
}

/// The identifier `node` carries, with its exact token span.
pub(crate) fn name_of(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    node: NodeId,
) -> Option<(String, Span)> {
    let (first, end) = tree.arena().token_extent(node)?;
    for index in first..end {
        let token = TokenId::new(index);
        if tree.tokens().kind(token) == TokenKind::Identifier.as_u16() {
            let span = *token_spans.get(index as usize)?;
            return Some((text.get(span.range())?.to_owned(), span));
        }
    }
    None
}

/// Index every grammar-owned enumerator once for range-based scope queries.
fn collect_enumerators(tree: &Tree, text: &str) -> Vec<EnumConstant> {
    let arena = tree.arena();
    let mut constants = Vec::new();
    for body in arena.preorder_roots() {
        if arena.tag(body) != Some(NodeTag::EnumBody.as_u16()) {
            continue;
        }
        let Some((body_start, body_end)) = arena.token_extent(body) else {
            continue;
        };
        for enumerator in arena.children_iter(body) {
            if arena.tag(enumerator) != Some(NodeTag::Enumerator.as_u16()) {
                continue;
            }
            let Some((first, item_end)) = arena.token_extent(enumerator) else {
                continue;
            };
            let Some(index) = (first..item_end).find(|index| {
                tree.tokens().kind(TokenId::new(*index)) == TokenKind::Identifier.as_u16()
            }) else {
                continue;
            };
            constants.push(EnumConstant {
                body_start,
                body_end,
                name: tree
                    .tokens()
                    .text(TokenId::new(index), text)
                    .trim()
                    .to_owned(),
                token: index,
            });
        }
    }
    constants
}

fn enumerators_in(
    enumerators: &[EnumConstant],
    start: u32,
    end: u32,
) -> impl Iterator<Item = (&str, u32)> {
    enumerators
        .iter()
        .filter(move |item| item.body_start >= start && item.body_end <= end)
        .map(|item| (item.name.as_str(), item.token))
}

fn collect_typedef_declarations(tree: &Tree, text: &str, token_spans: &[Span]) -> BTreeSet<Span> {
    let arena = tree.arena();
    let mut declarations = BTreeSet::new();
    for declaration in arena.preorder_roots() {
        if arena.tag(declaration) != Some(NodeTag::Decl.as_u16()) {
            continue;
        }
        let is_typedef = arena.children_iter(declaration).any(|child| {
            arena.tag(child) == Some(NodeTag::DeclSpecifiers.as_u16())
                && arena.token_extent(child).is_some_and(|(first, end)| {
                    (first..end).any(|index| {
                        tree.tokens().kind(TokenId::new(index)) == TokenKind::KwTypedef.as_u16()
                    })
                })
        });
        if !is_typedef {
            continue;
        }
        for declarator in arena.children_iter(declaration) {
            if arena.tag(declarator) != Some(NodeTag::Declarator.as_u16()) {
                continue;
            }
            if let Some(span) = arena.preorder(declarator).find_map(|node| {
                (arena.tag(node) == Some(NodeTag::DeclName.as_u16()))
                    .then(|| name_of(tree, text, token_spans, node))
                    .flatten()
                    .map(|(_, span)| span)
            }) {
                declarations.insert(span);
            }
        }
    }
    declarations
}

fn parameter_in_group(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    group_start: u32,
    group_end: u32,
    symbols: &TranslationUnitSymbols,
    function_offset: u32,
) -> Option<ParameterDeclarator> {
    let token_text = |index| tree.tokens().text(TokenId::new(index), text).trim();
    let mut index = group_start;
    while index + 2 < group_end {
        if token_text(index) == "(" && token_text(index + 1) == "*" {
            let mut cursor = index + 2;
            let mut has_name = false;
            while cursor < group_end && token_text(cursor) != ")" {
                has_name |=
                    tree.tokens().kind(TokenId::new(cursor)) == TokenKind::Identifier.as_u16();
                cursor += 1;
            }
            if cursor < group_end && !has_name {
                return None;
            }
            break;
        }
        index += 1;
    }

    for index in group_start..group_end {
        let token = TokenId::new(index);
        if tree.tokens().kind(token) != TokenKind::Identifier.as_u16() {
            continue;
        }
        let follows = if index + 1 == group_end {
            ")"
        } else {
            token_text(index + 1)
        };
        if !matches!(follows, ")" | "[" | "(") {
            continue;
        }
        let preceded_by_tag = index > group_start
            && matches!(
                tree.tokens().kind(TokenId::new(index - 1)),
                kind if kind == TokenKind::KwStruct.as_u16()
                    || kind == TokenKind::KwUnion.as_u16()
                    || kind == TokenKind::KwEnum.as_u16()
            );
        if preceded_by_tag {
            continue;
        }
        let span = *token_spans.get(index as usize)?;
        let name = text.get(span.range())?.to_owned();
        let identifiers = (group_start..group_end)
            .filter(|candidate| {
                tree.tokens().kind(TokenId::new(*candidate)) == TokenKind::Identifier.as_u16()
            })
            .count();
        let has_explicit_base = (group_start..index).any(|candidate| {
            matches!(
                TokenKind::from_u16(tree.tokens().kind(TokenId::new(candidate))),
                Some(
                    TokenKind::KwBool
                        | TokenKind::KwChar
                        | TokenKind::KwComplex
                        | TokenKind::KwDouble
                        | TokenKind::KwFloat
                        | TokenKind::KwInt
                        | TokenKind::KwInt128
                        | TokenKind::KwLong
                        | TokenKind::KwShort
                        | TokenKind::KwSigned
                        | TokenKind::KwUnsigned
                        | TokenKind::KwVoid
                )
            )
        });
        if identifiers == 1
            && !has_explicit_base
            && symbols.typedef_is_visible(&name, function_offset)
        {
            continue;
        }
        return Some(ParameterDeclarator {
            name,
            span,
            group_start,
            group_end,
            name_index: index,
        });
    }
    None
}

fn declarator_preserves_function_type(tree: &Tree, declarator: NodeId) -> bool {
    let Some((first, end)) = tree.arena().token_extent(declarator) else {
        return false;
    };
    !(first..end).any(|index| {
        matches!(
            TokenKind::from_u16(tree.tokens().kind(TokenId::new(index))),
            Some(TokenKind::Star | TokenKind::LBracket)
        )
    })
}

fn declarator_declares_function(tree: &Tree, declarator: NodeId) -> bool {
    let arena = tree.arena();
    let Some(name) = arena
        .preorder(declarator)
        .find(|node| arena.tag(*node) == Some(NodeTag::DeclName.as_u16()))
        .and_then(|node| arena.main_token(node))
    else {
        return false;
    };
    let Some(params) = arena
        .preorder(declarator)
        .find(|node| arena.tag(*node) == Some(NodeTag::ParamList.as_u16()))
        .and_then(|node| arena.token_extent(node).map(|extent| extent.0))
    else {
        return false;
    };
    if params <= name.raw() {
        return false;
    }
    let Some((first, _)) = arena.token_extent(declarator) else {
        return false;
    };
    let mut depth = 0u32;
    let mut pointer_depths = BTreeSet::new();
    for index in first..name.raw() {
        match TokenKind::from_u16(tree.tokens().kind(TokenId::new(index))) {
            Some(TokenKind::LParen) => depth += 1,
            Some(TokenKind::RParen) => depth = depth.saturating_sub(1),
            Some(TokenKind::Star) if depth > 0 => {
                pointer_depths.insert(depth);
            }
            _ => {}
        }
    }
    let name_depth = depth;
    if name_depth == 0 || !pointer_depths.contains(&name_depth) {
        return true;
    }
    for index in name.raw() + 1..params {
        match TokenKind::from_u16(tree.tokens().kind(TokenId::new(index))) {
            Some(TokenKind::LParen) => depth += 1,
            Some(TokenKind::RParen) => {
                depth = depth.saturating_sub(1);
                if depth < name_depth {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::parse::parse;

    #[test]
    fn parameter_resolution_consumes_parser_owned_groups_once() {
        let text =
            "typedef int T; int f(T value, int (*cb)(int, int), struct P p) { return value; }";
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let symbols = TranslationUnitSymbols::collect(&tree, text, &spans);
        let function = tree.functions(text).pop().expect("definition");
        let declarations = parameter_declarations(
            &tree,
            text,
            &spans,
            function.node,
            &symbols,
            function.span.lo,
        );
        assert_eq!(declarations.groups().len(), 3);
        assert_eq!(
            declarations
                .named()
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>(),
            ["value", "cb", "p"]
        );
    }

    #[test]
    fn lexical_resolution_obeys_declaration_points_and_nested_shadowing() {
        let text = "int f(int param) { int before = param; { typedef int param; int n = sizeof(param); } return param; }";
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
        let offsets: Vec<u32> = text
            .match_indices("param")
            .map(|(offset, _)| offset as u32)
            .collect();
        assert_eq!(offsets.len(), 5);

        assert!(
            resolution.resolve_at("param", offsets[0]).is_none(),
            "a declaration is not visible before its declarator completes"
        );
        let parameter = resolution
            .resolve_at("param", offsets[1])
            .expect("parameter");
        assert_eq!(parameter.kind, SymbolKind::Value);
        let inner = resolution
            .resolve_at("param", offsets[3])
            .expect("inner typedef");
        assert_eq!(inner.kind, SymbolKind::Typedef);
        assert_ne!(inner.id, parameter.id);
        assert_eq!(
            resolution
                .resolve_at("param", offsets[4])
                .map(|item| item.id),
            Some(parameter.id),
            "leaving the nested block restores the parameter"
        );
    }

    #[test]
    fn declarator_ownership_identifies_array_suffixes_without_traversal_state() {
        let text = "int f(int n) { typedef int A[n]; int a[n]; return n; }";
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
        let suffixes: Vec<u32> = text
            .match_indices("[n]")
            .map(|(offset, _)| offset as u32 + 1)
            .collect();

        let alias = resolution
            .declaration_owning(suffixes[0])
            .expect("typedef owns its suffix");
        assert_eq!(
            (alias.name.as_str(), alias.kind),
            ("A", SymbolKind::Typedef)
        );
        let value = resolution
            .declaration_owning(suffixes[1])
            .expect("value owns its suffix");
        assert_eq!((value.name.as_str(), value.kind), ("a", SymbolKind::Value));
    }
}
