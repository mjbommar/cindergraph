//! Source metrics for C: what one translation unit and each of its functions
//! look like, measured.
//!
//! These are properties of one source unit, not similarity scores against a
//! reference program. They support hotspot ranking, corpus feature extraction,
//! complexity gates and comparisons between two versions of the same named
//! functions. The implementation reuses the parser's token buffer, arena tree
//! and general CFG rather than constructing a second model.
//!
//! # Layering
//!
//! The graph half is language-neutral and lives in
//! [`crate::syntax::metrics`], for the same reason [`crate::syntax::ged`] does:
//! `E - N + 2` reads adjacency and knows nothing about C. This module supplies
//! the C-specific half: node tags, token kinds, parameter spellings and report
//! assembly.
//!
//! **These metrics are computed on [`crate::csource::cfg`], never on
//! [`crate::csource::parity`].** The parity layer reproduces another tool's
//! artifacts on purpose: coalesced expression chains, a function-end node
//! deleted when it stayed a singleton, entry and exit as derived flags. A
//! cyclomatic number taken from that graph would measure comparison-specific
//! expression granularity instead of the ordinary source CFG.
//!
//! # Totality
//!
//! [`analyze`] returns a report and diagnostics together. A partly recovered
//! file retains the functions that were recovered and reports parser/CFG
//! problems alongside them. Robustness is exercised by bundled and seeded
//! malformed-input tests; finite tests are not a proof over every byte string.

pub mod halstead;
pub mod shape;
pub mod size;

#[cfg(test)]
mod tests;

use crate::csource::cfg::{function_cfgs_with, Coverage};
use crate::csource::lex::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::{parse, Tree};
use crate::syntax::diag::Parsed;
use crate::syntax::ids::{NodeId, Span};
use crate::syntax::metrics::GraphMetrics;

pub use halstead::Halstead;
pub use shape::ShapeMetrics;
pub use size::{FileLines, LineIndex, SizeMetrics};

/// Everything measured about one function definition.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FunctionMetrics {
    /// The declared name, empty when the declarator had none.
    pub name: String,
    /// The whole definition, specifiers through closing brace.
    pub span: Span,
    /// Lines, tokens and bytes.
    pub size: SizeMetrics,
    /// Control-flow properties, from the general CFG.
    pub graph: GraphMetrics,
    /// Syntax properties, from the AST.
    pub shape: ShapeMetrics,
    /// Halstead's token measures.
    pub halstead: Halstead,
    /// Declared named/fixed parameters; `(void)`, `()`, and `...` add none.
    pub parameters: u32,
    /// How many `&&`, `||` and `?:` operators the graph builder expanded into
    /// forks. Reported because it is the gap between the source's statement
    /// count and its branch count.
    pub short_circuits: u32,
    /// Whether the parser recovered a body. A definition without one is a
    /// recovery artifact, and every graph figure above it is the empty graph's.
    pub has_body: bool,
    /// Statements the source contains that no path from the entry reaches:
    /// code after a `return`, code after a `goto`, an arm no `case` selects.
    ///
    /// This cannot be read off [`FunctionMetrics::graph`], because the general
    /// CFG contains only reachable statements by construction --- an
    /// unreachable one is never emitted, so
    /// [`GraphMetrics::unreachable_nodes`] on it is structurally zero. The
    /// figure therefore comes from a second, syntax-directed build
    /// ([`Coverage::Syntactic`]) in which an unreachable region becomes a
    /// component with no path from the entry.
    ///
    /// **It is a lower bound.** The front end does not fold constants, so the
    /// statement after `for (;;) { }` is not counted: the loop header still
    /// carries a false arm to it. What is counted is genuinely unreachable;
    /// what is not counted may still be.
    pub unreachable_statements: u32,
}

/// Everything measured about one translation unit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SourceReport {
    /// Whole-file line counts.
    pub lines: FileLines,
    /// Tokens in the file.
    pub tokens: u32,
    /// Bytes in the file.
    pub bytes: u32,
    /// One entry per function definition with a body, in source order.
    pub functions: Vec<FunctionMetrics>,
}

/// Measure one translation unit of C.
///
/// The text is taken as given: [`crate::csource::normalize`] runs before this
/// when the input is a preprocessed unit or decompiler output, never after,
/// because byte offsets here are offsets into what is passed.
pub fn analyze(text: &str) -> Parsed<SourceReport> {
    let (tree, mut diagnostics) = parse(text).into_parts();
    let index = LineIndex::new(text);
    let (graphs, graph_diagnostics) =
        function_cfgs_with(&tree, text, Coverage::Reachable).into_parts();
    for diagnostic in graph_diagnostics.iter() {
        diagnostics.push(diagnostic.clone());
    }
    // A second, syntax-directed build, for `unreachable_statements` alone. Its
    // diagnostics are deliberately dropped: they are the same parse's, already
    // collected above, and a graph that intentionally fails `REQ-GEN-1` has
    // nothing new to report about the source.
    let syntactic = function_cfgs_with(&tree, text, Coverage::Syntactic)
        .into_parts()
        .0;

    let definitions = tree.functions(text);
    let mut functions = Vec::with_capacity(definitions.len());
    for (position, definition) in definitions.iter().enumerate() {
        let extent = tree.arena().token_extent(definition.node);
        let (first, end) = extent.unwrap_or((0, 0));

        let graph = graphs
            .get(position)
            .map(|entry| GraphMetrics::of(&entry.cfg))
            .unwrap_or_default();
        let short_circuits = graphs
            .get(position)
            .map(|entry| entry.short_circuits)
            .unwrap_or(0);
        let shape = definition
            .body
            .map(|body| shape::measure(&tree, text, body))
            .unwrap_or_default();

        functions.push(FunctionMetrics {
            name: definition.name.clone(),
            span: definition.span,
            size: size::measure(tree.tokens(), &index, definition.span, first, end),
            graph,
            shape,
            halstead: halstead::measure(tree.tokens(), text, first, end),
            parameters: parameter_count(&tree, definition.node, definition.body),
            short_circuits,
            has_body: definition.body.is_some(),
            unreachable_statements: syntactic
                .get(position)
                .map(|entry| GraphMetrics::of(&entry.cfg).unreachable_nodes)
                .unwrap_or(0),
        });
    }

    Parsed::new(
        SourceReport {
            lines: size::file_lines(text, tree.tokens(), &index),
            tokens: tree.tokens().len() as u32,
            bytes: text.len() as u32,
            functions,
        },
        diagnostics,
    )
}

/// The declared parameter count of the definition rooted at `node`.
///
/// The declarator's parameter list is the first [`NodeTag::ParamList`] in the
/// definition's subtree that precedes the body. Node ids are assigned in
/// pre-order (`crate::syntax::tree::TreeSink`), so "precedes the body" is an id
/// comparison rather than a second walk, and a `ParamList` belonging to a
/// function-pointer parameter *inside* the body cannot be mistaken for it.
fn parameter_count(tree: &Tree, node: NodeId, body: Option<NodeId>) -> u32 {
    let arena = tree.arena();
    for candidate in arena.preorder(node) {
        if let Some(body) = body {
            if candidate.raw() >= body.raw() {
                break;
            }
        }
        if arena.tag(candidate).and_then(NodeTag::from_u16) == Some(NodeTag::ParamList) {
            return arena
                .children_iter(candidate)
                .filter(|child| arena.tag(*child) == Some(NodeTag::ParamDecl.as_u16()))
                .filter(|child| {
                    let Some((first, end)) = arena.token_extent(*child) else {
                        return false;
                    };
                    if end != first + 1 {
                        return true;
                    }
                    !matches!(
                        TokenKind::from_u16(
                            tree.tokens().kind(crate::syntax::ids::TokenId::new(first))
                        ),
                        Some(TokenKind::KwVoid | TokenKind::Ellipsis)
                    )
                })
                .count() as u32;
        }
    }
    0
}
