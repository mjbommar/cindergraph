//! Conservative target resolution for GNU C computed `goto` expressions.
//!
//! This first semantic slice consumes grammar nodes rather than searching raw
//! source strings. Unknown ownership or any non-dispatch use of a table returns
//! `None`; the caller then retains the function-wide address-taken fallback.

use std::collections::{BTreeMap, BTreeSet};

use crate::csource::lex::kind::TokenKind;
use crate::csource::parse::{tag::NodeTag, Tree};
use crate::syntax::cfg::{DispatchUncertainty, TargetPrecision};
use crate::syntax::ids::{NodeId, Span, TokenId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedDispatch {
    pub(super) labels: Vec<String>,
    pub(super) precision: TargetPrecision,
    pub(super) may_be_invalid: bool,
    pub(super) reasons: Vec<DispatchUncertainty>,
}

pub(super) fn resolve(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    body: NodeId,
    goto: NodeId,
    address_taken: &[String],
) -> Option<ResolvedDispatch> {
    let value = dispatch_value_root(tree, goto)?;
    let mut scalar = ScalarResolver::new(tree, text, spans, body, address_taken);
    if let Some((labels, reassigned)) = scalar.resolve(value) {
        return Some(ResolvedDispatch {
            labels,
            precision: if reassigned {
                TargetPrecision::Conservative
            } else {
                TargetPrecision::Exact
            },
            may_be_invalid: false,
            reasons: reassigned
                .then_some(DispatchUncertainty::FlowInsensitiveJoin)
                .into_iter()
                .collect(),
        });
    }

    let (table, _, index) = table_base(tree, text, spans, goto)?;
    let labels = immutable_table_labels(tree, text, spans, body, &table, address_taken)?;
    if let Some(index) = constant_index(tree, text, spans, index) {
        if let Some(label) = labels.get(index) {
            return Some(ResolvedDispatch {
                labels: vec![label.clone()],
                precision: TargetPrecision::Exact,
                may_be_invalid: false,
                reasons: Vec::new(),
            });
        }
    }
    Some(ResolvedDispatch {
        labels,
        precision: TargetPrecision::Exact,
        may_be_invalid: true,
        reasons: vec![DispatchUncertainty::IndexMayBeOutOfBounds],
    })
}

/// Resolves only immutable scalar label-pointer values. Anything requiring
/// ordering, aliasing, or mutation falls through to the caller's conservative
/// function-wide target set.
struct ScalarResolver<'a> {
    tree: &'a Tree,
    text: &'a str,
    spans: &'a [Span],
    body: NodeId,
    address_taken: &'a [String],
    consumed_refs: BTreeSet<NodeId>,
    resolved_names: BTreeSet<String>,
    visiting: BTreeSet<String>,
    cache: BTreeMap<String, Vec<String>>,
    reassigned: bool,
}

impl<'a> ScalarResolver<'a> {
    fn new(
        tree: &'a Tree,
        text: &'a str,
        spans: &'a [Span],
        body: NodeId,
        address_taken: &'a [String],
    ) -> Self {
        Self {
            tree,
            text,
            spans,
            body,
            address_taken,
            consumed_refs: BTreeSet::new(),
            resolved_names: BTreeSet::new(),
            visiting: BTreeSet::new(),
            cache: BTreeMap::new(),
            reassigned: false,
        }
    }

    fn resolve(&mut self, node: NodeId) -> Option<(Vec<String>, bool)> {
        let labels = self.resolve_value(unwrap_parens(self.tree, node)?)?;
        for name in &self.resolved_names {
            for candidate in self.tree.arena().preorder(self.body) {
                if tag(self.tree, candidate) == Some(NodeTag::NameRef)
                    && node_name(self.tree, self.text, self.spans, candidate).as_deref()
                        == Some(name.as_str())
                    && !self.consumed_refs.contains(&candidate)
                    && !is_direct_dispatch_value(self.tree, self.body, candidate)
                {
                    return None;
                }
            }
        }
        let mut ordered = self
            .address_taken
            .iter()
            .filter(|label| labels.contains(*label))
            .cloned()
            .collect::<Vec<_>>();
        ordered.dedup();
        (!ordered.is_empty()).then_some((ordered, self.reassigned))
    }

    fn resolve_value(&mut self, node: NodeId) -> Option<BTreeSet<String>> {
        let node = unwrap_parens(self.tree, node)?;
        match tag(self.tree, node)? {
            NodeTag::LabelAddr => {
                let label = node_name(self.tree, self.text, self.spans, node)?;
                self.address_taken
                    .contains(&label)
                    .then(|| BTreeSet::from([label]))
            }
            NodeTag::CondExpr => {
                let expressions = self
                    .tree
                    .arena()
                    .children_iter(node)
                    .filter(|child| tag(self.tree, *child).is_some_and(NodeTag::is_expression))
                    .collect::<Vec<_>>();
                let [_, yes, no] = expressions.as_slice() else {
                    return None;
                };
                let mut labels = self.resolve_value(*yes)?;
                labels.extend(self.resolve_value(*no)?);
                Some(labels)
            }
            NodeTag::NameRef => {
                let name = node_name(self.tree, self.text, self.spans, node)?;
                self.consumed_refs.insert(node);
                self.resolve_name(&name)
                    .map(|labels| labels.into_iter().collect())
            }
            _ => None,
        }
    }

    fn resolve_name(&mut self, name: &str) -> Option<Vec<String>> {
        if let Some(labels) = self.cache.get(name) {
            return Some(labels.clone());
        }
        if !self.visiting.insert(name.to_owned()) {
            return None;
        }
        let initializer =
            scalar_pointer_initializer(self.tree, self.text, self.spans, self.body, name)?;
        let mut labels = self.resolve_value(initializer)?;
        for (lhs, rhs) in scalar_assignments(self.tree, self.text, self.spans, self.body, name)? {
            self.consumed_refs.insert(lhs);
            labels.extend(self.resolve_value(rhs)?);
            self.reassigned = true;
        }
        let labels = labels.into_iter().collect::<Vec<_>>();
        self.visiting.remove(name);
        self.resolved_names.insert(name.to_owned());
        self.cache.insert(name.to_owned(), labels.clone());
        Some(labels)
    }
}

fn scalar_assignments(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    body: NodeId,
    name: &str,
) -> Option<Vec<(NodeId, NodeId)>> {
    let arena = tree.arena();
    let mut assignments = Vec::new();
    for node in arena.preorder(body) {
        if tag(tree, node) != Some(NodeTag::AssignExpr) {
            continue;
        }
        let expressions = arena
            .children_iter(node)
            .filter(|child| tag(tree, *child).is_some_and(NodeTag::is_expression))
            .collect::<Vec<_>>();
        let [lhs, rhs] = expressions.as_slice() else {
            return None;
        };
        if tag(tree, *lhs) != Some(NodeTag::NameRef)
            || node_name(tree, text, spans, *lhs).as_deref() != Some(name)
        {
            continue;
        }
        let lhs_span = arena.span(*lhs, spans)?;
        let rhs_span = arena.span(*rhs, spans)?;
        if text.get(lhs_span.hi as usize..rhs_span.lo as usize)?.trim() != "=" {
            return None;
        }
        assignments.push((*lhs, *rhs));
    }
    Some(assignments)
}

fn is_direct_dispatch_value(tree: &Tree, body: NodeId, candidate: NodeId) -> bool {
    tree.arena().preorder(body).any(|node| {
        tag(tree, node) == Some(NodeTag::GotoStmt)
            && dispatch_value_root(tree, node) == Some(candidate)
    })
}

fn scalar_pointer_initializer(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    body: NodeId,
    name: &str,
) -> Option<NodeId> {
    let arena = tree.arena();
    let mut matches = Vec::new();
    for declaration in arena.preorder(body) {
        if tag(tree, declaration) != Some(NodeTag::Decl) {
            continue;
        }
        for pair in arena
            .children_iter(declaration)
            .collect::<Vec<_>>()
            .windows(2)
        {
            let (declarator, initializer) = (pair[0], pair[1]);
            let names = descendants_with_tag(tree, declarator, NodeTag::DeclName);
            if tag(tree, declarator) == Some(NodeTag::Declarator)
                && tag(tree, initializer) == Some(NodeTag::Initializer)
                && descendants_with_tag(tree, declarator, NodeTag::PointerOperator).len() == 1
                && descendants_with_tag(tree, declarator, NodeTag::ArraySuffix).is_empty()
                && names.len() == 1
                && node_name(tree, text, spans, names[0]).as_deref() == Some(name)
            {
                let values = arena
                    .children_iter(initializer)
                    .filter(|child| tag(tree, *child).is_some_and(NodeTag::is_expression))
                    .collect::<Vec<_>>();
                if let [value] = values.as_slice() {
                    matches.push(*value);
                }
            }
        }
    }
    let [value] = matches.as_slice() else {
        return None;
    };
    Some(*value)
}

fn immutable_table_labels(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    body: NodeId,
    table: &str,
    address_taken: &[String],
) -> Option<Vec<String>> {
    let arena = tree.arena();
    let mut definitions = Vec::new();
    for declaration in arena.preorder(body) {
        if tag(tree, declaration) != Some(NodeTag::Decl) {
            continue;
        }
        let children = arena.children_iter(declaration).collect::<Vec<_>>();
        for pair in children.windows(2) {
            let declarator = pair[0];
            let initializer = pair[1];
            if tag(tree, declarator) != Some(NodeTag::Declarator)
                || tag(tree, initializer) != Some(NodeTag::Initializer)
                || descendants_with_tag(tree, declarator, NodeTag::ArraySuffix).is_empty()
                || descendants_with_tag(tree, declarator, NodeTag::PointerOperator).is_empty()
            {
                continue;
            }
            let names = descendants_with_tag(tree, declarator, NodeTag::DeclName);
            if names.len() == 1 && node_name(tree, text, spans, names[0]).as_deref() == Some(table)
            {
                definitions.push(initializer);
            }
        }
    }
    let [initializer] = definitions.as_slice() else {
        return None;
    };

    // Every value use must be structurally the base of an indexed computed
    // goto. Assignment, escape, decay into a call, or an unrecognized use
    // therefore falls back rather than being mistaken for immutability.
    let mut permitted_uses = BTreeSet::new();
    for node in arena.preorder(body) {
        if tag(tree, node) == Some(NodeTag::GotoStmt) {
            if let Some((name, base, _)) = table_base(tree, text, spans, node) {
                if name == table {
                    permitted_uses.insert(base);
                }
            }
        }
    }
    for node in arena.preorder(body) {
        if tag(tree, node) == Some(NodeTag::NameRef)
            && node_name(tree, text, spans, node).as_deref() == Some(table)
            && !permitted_uses.contains(&node)
        {
            return None;
        }
    }

    let lists = descendants_with_tag(tree, *initializer, NodeTag::InitList);
    let [list] = lists.as_slice() else {
        return None;
    };
    let elements = arena.children_iter(*list).collect::<Vec<_>>();
    if elements.is_empty()
        || elements
            .iter()
            .any(|node| tag(tree, *node) != Some(NodeTag::LabelAddr))
    {
        return None;
    }

    let mut labels = Vec::new();
    for &label_node in &elements {
        let label = node_name(tree, text, spans, label_node)?;
        if !address_taken.contains(&label) {
            return None;
        }
        if !labels.contains(&label) {
            labels.push(label);
        }
    }
    (!labels.is_empty()).then_some(labels)
}

/// The single `name[index]` postfix expression inside a computed goto.
fn table_base(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    goto: NodeId,
) -> Option<(String, NodeId, NodeId)> {
    let arena = tree.arena();
    let node = dispatch_value_root(tree, goto)?;
    if tag(tree, node) != Some(NodeTag::PostfixExpr) {
        return None;
    }
    let children = arena.children_iter(node).collect::<Vec<_>>();
    if children.len() != 2
        || tag(tree, children[0]) != Some(NodeTag::NameRef)
        || tag(tree, children[1]) != Some(NodeTag::IndexSuffix)
    {
        return None;
    }
    let indices = arena
        .children_iter(children[1])
        .filter(|child| tag(tree, *child).is_some_and(NodeTag::is_expression))
        .collect::<Vec<_>>();
    let [index] = indices.as_slice() else {
        return None;
    };
    Some((
        node_name(tree, text, spans, children[0])?,
        children[0],
        *index,
    ))
}

fn constant_index(tree: &Tree, text: &str, spans: &[Span], node: NodeId) -> Option<usize> {
    let node = unwrap_parens(tree, node)?;
    if tag(tree, node) != Some(NodeTag::Literal) {
        return None;
    }
    let span = tree.arena().span(node, spans)?;
    text.get(span.range())?.trim().parse().ok()
}

/// Value consumed by the grammar-mandated outer unary `*`, with redundant
/// parentheses removed. A nested table/label inside a larger expression is
/// deliberately not returned.
fn dispatch_value_root(tree: &Tree, goto: NodeId) -> Option<NodeId> {
    let arena = tree.arena();
    let roots = arena
        .children_iter(goto)
        .filter(|node| tag(tree, *node).is_some_and(NodeTag::is_expression))
        .collect::<Vec<_>>();
    let [outer] = roots.as_slice() else {
        return None;
    };
    if tag(tree, *outer) != Some(NodeTag::UnaryExpr) {
        return None;
    }
    let operands = arena
        .children_iter(*outer)
        .filter(|node| tag(tree, *node).is_some_and(NodeTag::is_expression))
        .collect::<Vec<_>>();
    let [mut value] = operands.as_slice() else {
        return None;
    };
    while tag(tree, value) == Some(NodeTag::ParenExpr) {
        let children = arena
            .children_iter(value)
            .filter(|node| tag(tree, *node).is_some_and(NodeTag::is_expression))
            .collect::<Vec<_>>();
        let [child] = children.as_slice() else {
            return None;
        };
        value = *child;
    }
    Some(value)
}

fn unwrap_parens(tree: &Tree, mut node: NodeId) -> Option<NodeId> {
    while tag(tree, node) == Some(NodeTag::ParenExpr) {
        let expressions = tree
            .arena()
            .children_iter(node)
            .filter(|child| tag(tree, *child).is_some_and(NodeTag::is_expression))
            .collect::<Vec<_>>();
        let [child] = expressions.as_slice() else {
            return None;
        };
        node = *child;
    }
    Some(node)
}

fn descendants_with_tag(tree: &Tree, root: NodeId, wanted: NodeTag) -> Vec<NodeId> {
    tree.arena()
        .preorder(root)
        .filter(|node| tag(tree, *node) == Some(wanted))
        .collect()
}

fn tag(tree: &Tree, node: NodeId) -> Option<NodeTag> {
    tree.arena().tag(node).and_then(NodeTag::from_u16)
}

fn node_name(tree: &Tree, text: &str, spans: &[Span], node: NodeId) -> Option<String> {
    let (first, end) = tree.arena().token_extent(node)?;
    for raw in first..end {
        let token = TokenId::new(raw);
        if TokenKind::from_u16(tree.tokens().kind(token)) == Some(TokenKind::Identifier) {
            let span = spans.get(raw as usize)?;
            return Some(text.get(span.range())?.to_owned());
        }
    }
    None
}
