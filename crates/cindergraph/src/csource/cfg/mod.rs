//! The C syntax-tree to control-flow-event bridge: one CFG per function.
//!
//! # What this is, and what it is deliberately not
//!
//! Both ends of this module already exist. [`crate::csource::parse`] turns C
//! text into an arena of tagged nodes; [`crate::syntax::cfg::CfgBuilder`]
//! turns a stream of [`Flow`] events into a graph, and owns the control-context
//! stack, the label backpatching and the structural invariants. This module is
//! the walk between them: it reads C constructs and emits the events that
//! describe them, and it knows nothing about graphs.
//!
//! What comes out is the **general** CFG: real successors, join points and loop
//! back edges. It is not the parity graph. The parity layer is a projection on
//! top of this graph; its comparison-specific expression granularity,
//! contraction and exit choices must not leak into metrics, dependence or
//! ordinary graph export. Nothing in this module depends on that projection.
//!
//! # The mapping
//!
//! | construct | events |
//! |---|---|
//! | expression statement, declaration with an initializer or runtime-bound candidate, `asm`, an unparsed region | [`Flow::Stmt`] |
//! | non-executable declaration, `;`, a `_Static_assert`, a directive | nothing |
//! | `if` / `else` | [`Flow::Branch`] + `Then` / `Else` / `EndScope` |
//! | `while`, `for`, `do`-`while` | [`Flow::LoopHeader`] with the matching [`crate::syntax::cfg::LoopKind`], plus [`Flow::LoopStep`] for a `for`'s update |
//! | `switch`, `case`, `default` | [`Flow::Switch`] + [`Flow::Case`], fall-through as an edge |
//! | GNU computed `goto` | one [`Flow::IndirectDispatch`] with the statically known label-address target set |
//! | `break`, `continue`, `goto`, a label, `return` | their own variants |
//! | `&&`, `||`, `?:` | a fork apiece, emitted by `expr` |
//!
//! The last row is the one that matters. The parser leaves all three as
//! ordinary expression nodes on purpose, so turning them into control flow is
//! this module's job wherever they appear: a condition, an initializer, a
//! `return` value, a call argument. A graph that treats `if (a && b)` as a
//! single condition is wrong in exactly the way the parity milestone measures.
//!
//! # Three rules the implementation is shaped by
//!
//! * **No native recursion.** There is one loop over
//!   a `Vec<Task>`, exactly as the parser next door has one loop over its own
//!   task stack, so a body nested thousands of levels deep costs heap and not
//!   stack. Nothing here calls itself, and no type here is recursive, so
//!   `Debug` and `Drop` cannot recurse either.
//! * **Recovery is represented.** Entry points return [`Parsed<Cfg>`]. A
//!   [`NodeTag::Error`] node from a recovered parse becomes a straight-line
//!   node and malformed constructs add diagnostics. Adversarial tests exercise
//!   missing children and deeply nested input without relying on native stack.
//! * **Determinism.** Events are emitted in source
//!   order, node ids are dense in emission order, and the only ordered
//!   containers whose iteration reaches the output are sorted.
//!
//! # Intentional divergence cycles
//!
//! The useful structural invariant that every non-diverging path reaches the
//! function end is false of a program with an inescapable loop, and
//! decompiler output has them: `spin: g(); goto spin;` reaches the function end
//! on no path at all. `while (1) {}` escapes the same verdict only because the
//! builder gives every loop header a second edge out of the construct;
//! inventing that edge for a `goto` loop would be a lie about what can happen.
//! The corpus gates below count that shape structurally --- and only where
//! every stranded node still has a successor, which is what makes the stranded
//! set a cycle rather than a dangling node --- and require every other failure
//! to be zero.
//!
//! Two further shapes were found while building this module and were **defects
//! in [`crate::syntax::cfg`]'s back-edge marking**, not in the mapping here: a
//! loop formed by a backward `goto` was not recognised as a loop, and a
//! `do`-`while`'s `continue` was marked a back edge although it jumps forwards.
//! Both are fixed upstream, and
//! `the_two_back_edge_defects_this_module_found_stay_fixed` is the
//! consumer-side regression test that keeps them fixed.
//!
//! # Why this is a directory
//!
//! Five files, one reason to change apiece: `mod.rs` (here) owns the public
//! surface and task machine, `stmt` owns the statement grammar's mapping,
//! `expr` owns expression control flow, `dispatch` resolves computed targets,
//! and `reach` owns the pre-pass that answers "can control get here" before a
//! node is placed. The `Emitter` type is
//! declared here so all four consumers can read its fields, which is the same
//! arrangement [`crate::syntax::cfg`] uses for [`Cfg`].

mod dispatch;
mod expr;
mod reach;
mod stmt;

use crate::csource::lex::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::{FunctionDef, Tree};
use crate::syntax::cfg::{Cfg, CfgBuilder, Flow};
use crate::syntax::diag::{Diagnostic, Diagnostics, Parsed};
use crate::syntax::ids::{NodeId, Span, Symbol, TokenId};
use crate::syntax::intern::SymbolTable;

pub use reach::Coverage;
use reach::{is_body, Reach};

/// How many task steps one function's walk may take before it stops.
///
/// `REQ-SYN-4` requires a work budget on every entry point, and `REQ-ROB-3`
/// requires a pathological input to terminate. The machine spends a small
/// constant number of steps per AST node, so a budget of 64 steps per node in
/// the function admits every real body by a wide margin and still bounds a
/// generator gone wrong. Exhausting it records a diagnostic and returns the
/// partial graph, which is what `REQ-ROB-2` means by per-function failure.
const STEPS_PER_NODE: u32 = 64;

/// One function definition's control-flow graph, with what it took to build.
///
/// [`FunctionCfg::short_circuits`] is carried rather than recomputed because it
/// is the census `REQ-CFG-6` is checked by: a corpus on which it is zero has an
/// unimplemented requirement however green the rest of the suite looks.
#[derive(Debug, Clone)]
pub struct FunctionCfg {
    /// The function's declared name, empty when the declarator had none.
    pub name: String,
    /// The span of the whole definition, specifiers through closing brace.
    pub span: Span,
    /// The definition node in the tree used to build this graph. Retaining it
    /// lets later passes visit this function without rescanning the whole unit.
    pub node: NodeId,
    /// The name token's byte span in that same source text.
    pub name_span: Span,
    /// The graph.
    pub cfg: Cfg,
    /// How many `&&`, `||` and `?:` operators were expanded into forks.
    pub short_circuits: u32,
    /// Nodes that exist only because a `&&`, `||` or `?:` was expanded
    /// (`REQ-CFG-6`): the tests and arms of those operators, and a nested
    /// operator's own join node inside a larger expression. Ascending. The
    /// node the whole statement, condition or `return` ends at is never here,
    /// so a consumer that wants statement-level control flow can collapse
    /// exactly these into the node they flow into. A node is listed only when
    /// every span it covers is such an operand; a node coalescing operand and
    /// statement spans is not.
    pub expression_internal: Vec<NodeId>,
}

/// Build the control-flow graph of one function definition.
///
/// Total on every input (`REQ-SYN-2`): a body the parser only partly recovered,
/// a body that is absent altogether, a construct with missing children --- each
/// yields a graph and a diagnostic rather than a failure. The graph always has
/// an entry node and a function-end node, so an empty body is the two of them
/// and one edge.
///
/// A caller building graphs for a whole file should use [`function_cfgs`]
/// instead: this function recomputes the tree's token span table, which costs
/// one re-lex of the file, and paying that per function is quadratic in the
/// number of functions.
pub fn function_cfg(tree: &Tree, text: &str, func: &FunctionDef) -> Parsed<Cfg> {
    let spans = tree.token_spans(text);
    let built = build_one(tree, text, &spans, func, Coverage::Reachable);
    let (value, diagnostics) = built.into_parts();
    Parsed::new(value.cfg, diagnostics)
}

/// Build the control-flow graph of every function definition in `tree`, in
/// source order.
///
/// The whole-file entry point, and the one the corpus gates use. Diagnostics
/// from every function are concatenated in source order, so the result is a
/// file-level report and a per-function failure never voids a sibling
/// (`REQ-ROB-2`). A declaration without a body yields nothing (`REQ-CFG-2`),
/// because [`Tree::functions`] already excludes it.
pub fn function_cfgs(tree: &Tree, text: &str) -> Parsed<Vec<FunctionCfg>> {
    function_cfgs_with(tree, text, Coverage::Reachable)
}

/// Build every function definition's graph with the statement coverage the
/// caller asks for.
///
/// [`Coverage::Reachable`] is [`function_cfgs`] and is what every consumer of
/// the general graph wants; the only caller of [`Coverage::Syntactic`] is
/// [`crate::csource::parity`], which is reproducing a tool whose CFG
/// construction is syntax-directed. The option is threaded rather than made a
/// second emitter because the parity layer must not have its own copy of the
/// statement grammar: an
/// unreachable region still contains `goto`s into live code and `break`s bound
/// to enclosing live constructs, and only the emitter's own control-context
/// stack resolves those. A graph built with [`Coverage::Syntactic`] does not
/// satisfy `REQ-GEN-1` and must not be handed to
/// [`crate::syntax::cfg::Cfg::validate`].
pub fn function_cfgs_with(tree: &Tree, text: &str, coverage: Coverage) -> Parsed<Vec<FunctionCfg>> {
    let spans = tree.token_spans(text);
    let mut graphs = Vec::new();
    let mut diagnostics = Diagnostics::new();
    for func in tree.functions(text) {
        let (graph, reported) = build_one(tree, text, &spans, &func, coverage).into_parts();
        for diagnostic in reported.iter() {
            diagnostics.push(diagnostic.clone());
        }
        graphs.push(graph);
    }
    Parsed::new(graphs, diagnostics)
}

/// Build one function's graph with a token span table the caller already has.
fn build_one(
    tree: &Tree,
    text: &str,
    spans: &[Span],
    func: &FunctionDef,
    coverage: Coverage,
) -> Parsed<FunctionCfg> {
    let mut emitter = Emitter::new(tree, text, spans, func, coverage);
    if let Some(body) = func.body {
        emitter.push_all(&[Task::Stmt(body)]);
        emitter.run();
    }
    emitter.finish(func)
}

/// One suspended step of the walk: the task the emitter's stack holds.
///
/// A `Task` *is* the call stack a recursive walker would have used, made data
/// so that its depth is heap-bounded rather than stack-bounded (`REQ-GEN-4`).
///
/// Tasks are `Copy` and hold no owned data, so pushing and popping one is free
/// and the stack can grow without a move constructor's worth of work.
#[derive(Debug, Clone)]
enum Task {
    /// Map one statement node to events.
    Stmt(NodeId),
    /// Emit everything the evaluation of one expression contributes, ending at
    /// `terminal`.
    Expr {
        /// The expression node.
        node: NodeId,
        /// The node the evaluation ends at.
        terminal: Terminal,
    },
    /// Emit one event verbatim.
    Emit(Flow),
    /// Set the reachability flag, which is how a region's entry re-establishes
    /// liveness after a `return` in the statement before it.
    Live(bool),
}

/// The node an expression's evaluation ends at.
///
/// An expression does not know what kind of node it produces; its *context*
/// does. The same `a && b` is a branch in `if (a && b)`, a return in
/// `return a && b;` and a statement node in `x = a && b;`, and in all three the
/// `&&` operator's own node is that node --- which is what stops the expansion
/// of `REQ-CFG-6` from costing an extra hop every time.
#[derive(Debug, Clone, Copy)]
enum Terminal {
    /// No node: the expression contributes only whatever forks are inside it.
    None,
    /// A straight-line node.
    Stmt(Span),
    /// A two-way test, which opens a scope the caller must close.
    Branch(Span),
    /// The function's `return`.
    Return(Span),
    /// A switch dispatch, which opens a scope the caller must close.
    Switch {
        /// How many arms the emitter intends to follow with.
        arms: u32,
        /// The span of the switched-over expression.
        subject: Span,
    },
}

impl Terminal {
    /// The event this terminal places, if it places one.
    fn flow(self) -> Option<Flow> {
        match self {
            Terminal::None => None,
            Terminal::Stmt(span) => Some(Flow::Stmt(span)),
            Terminal::Branch(cond) => Some(Flow::Branch { cond }),
            Terminal::Return(span) => Some(Flow::Return(span)),
            Terminal::Switch { arms, subject } => Some(Flow::Switch { arms, subject }),
        }
    }

    /// This terminal, or a straight-line node at `span` when it has none.
    ///
    /// A short-circuit operator always gets its own node: it is the join both
    /// arms reach, and without it the two arms would dangle onto whatever
    /// happened to be placed next, which is a different graph.
    fn or_stmt(self, span: Span) -> Terminal {
        match self {
            Terminal::None => Terminal::Stmt(span),
            other => other,
        }
    }
}

/// Which construct one of the emitter's open scopes is.
///
/// A mirror of the builder's own stack, kept because two decisions need it and
/// neither can ask the builder: whether a `case` label has a switch to belong
/// to, and --- through it --- whether an unreachable subtree containing one is
/// still worth emitting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScopeKind {
    /// A two-way branch.
    Branch,
    /// A loop.
    Loop,
    /// A switch.
    Switch,
}

/// The walk's whole mutable state.
///
/// One struct rather than a set of free functions taking eight arguments, for
/// the reason [`crate::csource::parse`]'s own machine gives: every step needs
/// the tree, the builder, the task stack and the budget, and threading four
/// `&mut` through a task machine is how borrow errors get resolved by cloning.
struct Emitter<'a> {
    /// The tree being walked.
    tree: &'a Tree,
    /// The source text the tree indexes into.
    text: &'a str,
    /// Every token's exact span, hoisted by the caller.
    spans: &'a [Span],
    /// The pre-pass answers: reachability, jump targets, short-circuits.
    reach: Reach,
    /// Label names, interned so a `goto` and its label agree on one id.
    labels: SymbolTable,
    /// The graph under construction.
    builder: CfgBuilder,
    /// The explicit task stack that replaces native recursion.
    tasks: Vec<Task>,
    /// The builder's open scopes, mirrored.
    scopes: Vec<ScopeKind>,
    /// Whether control can reach the next node placed.
    live: bool,
    /// Steps remaining before the walk gives up on this function.
    steps: u32,
    /// Problems found walking, kept apart from the builder's own until the end.
    diagnostics: Diagnostics,
    /// How many short-circuit operators have been expanded.
    short_circuits: u32,
    /// Spans of the nodes a short-circuit expansion placed for its operands,
    /// recorded at the one place that knows: the expansion itself.
    expression_internal: std::collections::BTreeSet<Span>,
    /// The span to attribute a node the arena gave no extent to.
    fallback: Span,
}

impl<'a> Emitter<'a> {
    /// An emitter positioned at the start of `func`, with the entry and
    /// function-end nodes already placed by the builder.
    fn new(
        tree: &'a Tree,
        text: &'a str,
        spans: &'a [Span],
        func: &FunctionDef,
        coverage: Coverage,
    ) -> Self {
        let body = func.body.unwrap_or(func.node);
        let nodes = tree.arena().preorder(body).count() as u32;
        Self {
            tree,
            text,
            spans,
            reach: Reach::with_coverage(tree, text, spans, body, coverage),
            labels: SymbolTable::new(),
            builder: CfgBuilder::new(func.span),
            tasks: Vec::new(),
            scopes: Vec::new(),
            live: true,
            steps: nodes.saturating_mul(STEPS_PER_NODE).max(1024),
            diagnostics: Diagnostics::new(),
            short_circuits: 0,
            expression_internal: std::collections::BTreeSet::new(),
            fallback: Span::empty_at(func.span.lo),
        }
    }

    /// Run every queued task to completion, or until the budget runs out.
    fn run(&mut self) {
        while let Some(task) = self.tasks.pop() {
            if self.steps == 0 {
                self.diagnostics.push(Diagnostic::error(
                    self.fallback,
                    "control-flow walk exceeded its work budget; the graph is partial",
                ));
                self.tasks.clear();
                return;
            }
            self.steps -= 1;
            match task {
                Task::Stmt(node) => self.statement(node),
                Task::Expr { node, terminal } => self.expression(node, terminal),
                Task::Emit(flow) => self.emit(flow),
                Task::Live(live) => self.live = live,
            }
        }
    }

    /// Queue `tasks` to run in the order written.
    ///
    /// The stack is LIFO, so they go on backwards; every caller writes the
    /// sequence forwards and this is the single place that reverses it.
    fn push_all(&mut self, tasks: &[Task]) {
        for task in tasks.iter().rev() {
            self.tasks.push(task.clone());
        }
    }

    /// Hand one event to the builder, keeping the emitter's own mirrors true.
    ///
    /// Liveness is updated here rather than at the call sites because the rule
    /// is a property of the event: a transfer leaves nothing behind it, a label
    /// or a `case` is reached without falling into it, and the region between a
    /// `switch` dispatch and its first arm is reachable from nothing at all ---
    /// which is what makes `switch (x) { int y; case 1: ... }` legal C with an
    /// unreachable declaration in it.
    fn emit(&mut self, flow: Flow) {
        match flow {
            Flow::Branch { .. } => self.scopes.push(ScopeKind::Branch),
            Flow::LoopHeader { .. } => self.scopes.push(ScopeKind::Loop),
            Flow::Switch { .. } => {
                self.scopes.push(ScopeKind::Switch);
                self.live = false;
            }
            Flow::EndScope => {
                self.scopes.pop();
            }
            Flow::Return(_)
            | Flow::Goto { .. }
            | Flow::IndirectDispatch { .. }
            | Flow::Break { .. }
            | Flow::Continue { .. }
            | Flow::Diverge(_) => self.live = false,
            Flow::Case { .. } | Flow::Label { .. } => self.live = true,
            _ => {}
        }
        self.builder.push(flow);
    }

    /// Whether a `case` label here would have a switch to belong to.
    fn in_switch(&self) -> bool {
        self.scopes.contains(&ScopeKind::Switch)
    }

    /// The byte span of `node`, or the function's start when it consumed no
    /// token.
    ///
    /// `REQ-SYN-7` makes spans total, so there is no "no span" answer; a node
    /// the arena gave no extent to still gets a position.
    fn span(&self, node: NodeId) -> Span {
        self.tree
            .arena()
            .span(node, self.spans)
            .unwrap_or(self.fallback)
    }

    /// Close every scope the events left open, then hand back the graph.
    ///
    /// An unbalanced scope stack is the builder's to complain about --- its
    /// `finish` closes what is open and says so --- but the emitter reports it
    /// too, because a stack that is not empty here means *this* module lost
    /// track, which is a different defect from a malformed tree.
    fn finish(mut self, func: &FunctionDef) -> Parsed<FunctionCfg> {
        if !self.scopes.is_empty() {
            self.diagnostics.push(Diagnostic::error(
                self.fallback,
                format!(
                    "{} control-flow scope(s) were still open at the end of the function",
                    self.scopes.len()
                ),
            ));
        }
        let (cfg, reported) = self.builder.finish().into_parts();
        let mut diagnostics = self.diagnostics;
        for diagnostic in reported.iter() {
            diagnostics.push(diagnostic.clone());
        }
        let expression_internal = cfg
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| {
                !node.spans().is_empty()
                    && node
                        .spans()
                        .iter()
                        .all(|span| self.expression_internal.contains(span))
            })
            .map(|(index, _)| NodeId::new(index as u32))
            .collect();
        Parsed::new(
            FunctionCfg {
                name: func.name.clone(),
                span: func.span,
                node: func.node,
                name_span: func.name_span,
                cfg,
                short_circuits: self.short_circuits,
                expression_internal,
            },
            diagnostics,
        )
    }
}

/// The tag of `node`, or `None` when it names none.
///
/// One place rather than the four that would otherwise unwrap a `u16` into a
/// [`NodeTag`], and the honest answer for a tag a different front end wrote.
fn tag_of(tree: &Tree, node: NodeId) -> Option<NodeTag> {
    tree.arena().tag(node).and_then(NodeTag::from_u16)
}

/// The children of `node` that are statement bodies.
///
/// "Which child is the body" is asked by `if`, `while`, `do`, `for` and
/// `switch`, and every one of them has to skip the same two kinds of child: the
/// controlling expression, and --- for a `for` --- its three clause nodes, which
/// are statements by the tag table's grouping but parts of the header rather
/// than of the body.
fn bodies(tree: &Tree, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
    tree.arena()
        .children_iter(node)
        .filter(move |child| tag_of(tree, *child).is_some_and(is_body))
}

/// The label a `goto` names, or `None` for the computed form `goto *p`.
///
/// Read from the token after the keyword rather than from a child node: the
/// parser consumes the identifier into the `goto` node itself, so there is no
/// child to read, and the computed form is exactly the case where the token
/// after the keyword is not an identifier.
fn goto_label(tree: &Tree, text: &str, spans: &[Span], node: NodeId) -> Option<String> {
    let (first, end) = tree.arena().token_extent(node)?;
    let raw = first.checked_add(1)?;
    if raw >= end {
        return None;
    }
    let id = TokenId::new(raw);
    if TokenKind::from_u16(tree.tokens().kind(id)) != Some(TokenKind::Identifier) {
        return None;
    }
    let span = spans.get(id.index())?;
    Some(text.get(span.range())?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::parse::parse;
    use crate::syntax::cfg::{EdgeKind, LoopKind, NodeKind};
    use crate::syntax::diag::Severity;
    use std::path::{Path, PathBuf};

    /// Every function's graph in `text`, asserting each validates clean.
    fn graphs(text: &str) -> Vec<FunctionCfg> {
        let tree = parse(text).into_parts().0;
        let built = function_cfgs(&tree, text);
        for graph in built.value() {
            let failures: Vec<String> = graph
                .cfg
                .validate()
                .iter()
                .map(|d| d.render(text))
                .collect();
            assert!(
                failures.is_empty(),
                "{} did not validate: {failures:#?}",
                graph.name
            );
        }
        built.into_parts().0
    }

    /// The one graph in `text`.
    fn graph(text: &str) -> Cfg {
        let mut built = graphs(text);
        assert_eq!(built.len(), 1, "expected exactly one function");
        built.remove(0).cfg
    }

    fn kind_count(cfg: &Cfg, kind: NodeKind) -> usize {
        cfg.nodes().iter().filter(|n| n.kind() == kind).count()
    }

    // --- the mapping ---------------------------------------------------------

    #[test]
    fn an_empty_body_is_an_entry_wired_to_the_function_end() {
        let cfg = graph("void f(void) { }");
        assert_eq!(cfg.node_count(), 2);
        assert_eq!(cfg.edge_count(), 1);
    }

    #[test]
    fn a_declaration_with_no_initializer_contributes_nothing() {
        let bare = graph("void f(void) { int a; }");
        assert_eq!(bare.node_count(), 2, "REQ-CFG-3");
        let initialised = graph("void f(void) { int a = 1; }");
        assert_eq!(initialised.node_count(), 3);
    }

    #[test]
    fn a_runtime_bound_is_an_executable_declaration_without_an_initializer() {
        let cfg = graph("void f(int n) { n = 1; int values[n]; return; }");
        assert_eq!(kind_count(&cfg, NodeKind::Stmt), 2);
        let statement_spans = cfg
            .nodes()
            .iter()
            .filter(|node| node.kind() == NodeKind::Stmt)
            .map(|node| node.span())
            .collect::<Vec<_>>();
        assert!(statement_spans[0].lo < statement_spans[1].lo);
    }

    #[test]
    fn conditional_runtime_bound_owns_real_cfg_arms() {
        let cfg = graph("void f(int c, int n, int m) { int values[c ? n + 1 : m + 1]; return; }");
        assert_eq!(kind_count(&cfg, NodeKind::Cond), 1);
        let (branch_index, _branch) = cfg
            .nodes()
            .iter()
            .enumerate()
            .find(|(_, node)| node.kind() == NodeKind::Cond)
            .expect("bound condition");
        let edge_kinds = cfg
            .successor_edges(NodeId::new(branch_index as u32))
            .iter()
            .map(|edge| edge.kind)
            .collect::<Vec<_>>();
        assert_eq!(edge_kinds, [EdgeKind::True, EdgeKind::False]);
        // One node per value arm, the declaration join, and `return`.
        assert_eq!(kind_count(&cfg, NodeKind::Stmt), 4);
    }

    #[test]
    fn straight_line_statements_chain_in_source_order() {
        let cfg = graph("void f(void) { g(); h(); i(); }");
        assert_eq!(kind_count(&cfg, NodeKind::Stmt), 3);
        assert_eq!(cfg.edge_count(), 4);
    }

    #[test]
    fn an_if_else_forks_and_joins() {
        let cfg = graph("void f(int a) { if (a) g(); else h(); i(); }");
        assert_eq!(kind_count(&cfg, NodeKind::Cond), 1);
        let test = NodeId::new(2);
        assert_eq!(cfg.out_degree(test), 2);
        let kinds: Vec<EdgeKind> = cfg.successor_edges(test).iter().map(|e| e.kind).collect();
        assert_eq!(kinds, vec![EdgeKind::True, EdgeKind::False]);
    }

    #[test]
    fn a_while_loop_has_a_back_edge_to_its_header() {
        let cfg = graph("void f(int a) { while (a) g(); }");
        assert_eq!(kind_count(&cfg, NodeKind::LoopHeader), 1);
        assert_eq!(cfg.edges().iter().filter(|e| e.is_back).count(), 1);
    }

    #[test]
    fn a_for_loops_continue_reaches_the_step_and_not_the_test() {
        let cfg = graph("void f(int n) { for (int i = 0; i < n; i++) { if (i) continue; g(); } }");
        let header = cfg
            .nodes()
            .iter()
            .position(|n| n.kind() == NodeKind::LoopHeader)
            .map(|i| NodeId::new(i as u32))
            .expect("a header");
        let target = cfg.continue_targets();
        assert_eq!(target.len(), 1);
        assert_ne!(
            target[0], header,
            "REQ-CFG-5: a for loop's continue lands on the step"
        );
        let continues: Vec<NodeId> = (0..cfg.node_count())
            .map(|i| NodeId::new(i as u32))
            .filter(|id| {
                cfg.node(*id)
                    .is_some_and(|n| n.kind() == NodeKind::Continue)
            })
            .collect();
        assert_eq!(continues.len(), 1);
        assert_eq!(cfg.successors(continues[0]).next(), Some(target[0]));
    }

    #[test]
    fn a_do_while_places_its_test_after_the_body() {
        let cfg = graph("void f(int a) { do { g(); } while (a); }");
        let header = cfg
            .nodes()
            .iter()
            .position(|n| n.kind() == NodeKind::LoopHeader)
            .map(|i| NodeId::new(i as u32))
            .expect("a header");
        assert!(
            !cfg.predecessors(header).is_empty(),
            "the body runs before the test"
        );
        assert!(cfg.edges().iter().any(|e| e.src == header && e.is_back));
    }

    #[test]
    fn a_switch_dispatches_to_every_arm_and_falls_through_between_them() {
        let cfg = graph(
            "void f(int a) { switch (a) { case 1: g(); case 2: h(); break; default: i(); } }",
        );
        assert_eq!(kind_count(&cfg, NodeKind::Switch), 1);
        assert_eq!(kind_count(&cfg, NodeKind::Case), 3);
        let dispatched = cfg
            .edges()
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Case | EdgeKind::Default))
            .count();
        assert_eq!(dispatched, 3);
        assert!(cfg.edges().iter().any(|e| e.kind == EdgeKind::FallThrough));
    }

    #[test]
    fn a_switch_with_no_default_leaves_the_construct_from_its_dispatch() {
        let cfg = graph("void f(int a) { switch (a) { case 1: g(); break; } h(); }");
        assert!(cfg.edges().iter().any(|e| e.kind == EdgeKind::Default));
    }

    #[test]
    fn a_goto_reaches_a_label_defined_later() {
        let cfg = graph("void f(int a) { if (a) goto done; g(); done: h(); }");
        assert_eq!(kind_count(&cfg, NodeKind::Goto), 1);
        assert_eq!(kind_count(&cfg, NodeKind::Label), 1);
        let goto = cfg
            .nodes()
            .iter()
            .position(|n| n.kind() == NodeKind::Goto)
            .map(|i| NodeId::new(i as u32))
            .expect("a goto");
        let label = cfg
            .nodes()
            .iter()
            .position(|n| n.kind() == NodeKind::Label)
            .map(|i| NodeId::new(i as u32))
            .expect("a label");
        assert_eq!(cfg.successors(goto).next(), Some(label));
    }

    #[test]
    fn a_computed_goto_with_no_label_address_records_a_divergence() {
        // Nothing to over-approximate to, so the abstention is all there is.
        // The function end is then reachable from nothing, which is the honest
        // consequence of a transfer whose destination this layer cannot name;
        // `graph` is deliberately not used, because that is a REQ-GEN-1 failure
        // and it is reported rather than hidden.
        let text = "void f(void **p) { goto *p[0]; }";
        let tree = parse(text).into_parts().0;
        let functions = tree.functions(text);
        let cfg = function_cfg(&tree, text, &functions[0]).into_parts().0;
        assert_eq!(kind_count(&cfg, NodeKind::Diverge), 1, "REQ-CFG-7");
    }

    #[test]
    fn a_computed_goto_reaches_every_label_whose_address_was_taken() {
        let cfg = graph(
            "int f(int a) { static void *t[2] = {&&one, &&two}; goto *t[a];\
             one: return 1; two: return 2; }",
        );
        assert_eq!(kind_count(&cfg, NodeKind::Label), 2);
        assert_eq!(kind_count(&cfg, NodeKind::IndirectDispatch), 1);
        assert_eq!(kind_count(&cfg, NodeKind::Cond), 0, "no synthetic tests");
        assert_eq!(kind_count(&cfg, NodeKind::Goto), 0, "no synthetic gotos");
        let dispatch = cfg
            .nodes()
            .iter()
            .position(|node| node.kind() == NodeKind::IndirectDispatch)
            .map(|index| NodeId::new(index as u32))
            .expect("one indirect dispatch");
        let targets: Vec<NodeId> = cfg.successors(dispatch).collect();
        assert_eq!(targets.len(), 2, "one edge per address-taken label");
        assert!(targets.iter().all(|target| {
            cfg.node(*target)
                .is_some_and(|node| node.kind() == NodeKind::Label)
        }));
        assert_eq!(kind_count(&cfg, NodeKind::Diverge), 0);
        let info = cfg
            .indirect_dispatch(dispatch)
            .expect("resolution metadata");
        assert_eq!(info.precision, crate::syntax::cfg::TargetPrecision::Exact);
        assert!(info.may_be_invalid, "the index is not yet range-proven");
        assert_eq!(
            info.reasons,
            [crate::syntax::cfg::DispatchUncertainty::IndexMayBeOutOfBounds]
        );
    }

    #[test]
    fn disjoint_immutable_tables_resolve_each_dispatch_independently() {
        let cfg = graph(concat!(
            "int f(int a, int b) { ",
            "static void *x[] = {&&x0, &&x1}; ",
            "static void *y[] = {&&y0, &&y1}; ",
            "if (a) goto *x[b]; else goto *y[b]; ",
            "x0:return 0; x1:return 1; y0:return 2; y1:return 3; }",
        ));
        let dispatches = cfg.indirect_dispatches();
        assert_eq!(dispatches.len(), 2);
        assert!(dispatches
            .iter()
            .all(|info| info.precision == crate::syntax::cfg::TargetPrecision::Exact));
        let first = cfg.successors(dispatches[0].node).collect::<Vec<_>>();
        let second = cfg.successors(dispatches[1].node).collect::<Vec<_>>();
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);
        assert!(first.iter().all(|target| !second.contains(target)));
    }

    #[test]
    fn an_in_bounds_constant_table_index_resolves_one_valid_target() {
        let cfg = graph(concat!(
            "int f(void) { static void *t[] = {&&a, &&b}; goto *t[1]; ",
            "a:return 1; b:return 2; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(info.precision, crate::syntax::cfg::TargetPrecision::Exact);
        assert!(!info.may_be_invalid);
        assert!(info.reasons.is_empty());
        assert_eq!(cfg.out_degree(info.node), 1);
        assert_eq!(kind_count(&cfg, NodeKind::Label), 1);
    }

    #[test]
    fn a_mutated_label_table_falls_back_to_every_address_taken_label() {
        let cfg = graph(concat!(
            "int f(int n) { static void *t[] = {&&a, &&b}; ",
            "t[0] = &&c; goto *t[n]; ",
            "a:return 1; b:return 2; c:return 3; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(
            info.precision,
            crate::syntax::cfg::TargetPrecision::Conservative
        );
        assert_eq!(
            info.reasons,
            [crate::syntax::cfg::DispatchUncertainty::UnresolvedValue]
        );
        assert_eq!(cfg.out_degree(info.node), 3);
    }

    #[test]
    fn a_direct_label_value_is_an_exact_dispatch() {
        let cfg = graph("int f(void) { goto *&&done; done: return 1; }");
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(info.precision, crate::syntax::cfg::TargetPrecision::Exact);
        assert!(!info.may_be_invalid);
        assert!(info.reasons.is_empty());
        assert_eq!(cfg.out_degree(info.node), 1);
    }

    #[test]
    fn immutable_scalar_copies_and_conditionals_resolve_exactly() {
        let cfg = graph(concat!(
            "int f(int choose) { void *p = choose ? &&a : &&b; void *q = p; ",
            "goto *q; a:return 1; b:return 2; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(info.precision, crate::syntax::cfg::TargetPrecision::Exact);
        assert!(!info.may_be_invalid);
        assert!(info.reasons.is_empty());
        assert_eq!(cfg.out_degree(info.node), 2);
    }

    #[test]
    fn one_immutable_scalar_can_feed_multiple_exact_dispatches() {
        let cfg = graph(concat!(
            "int f(int choose) { void *p = choose ? &&a : &&b; ",
            "if (choose) goto *p; goto *p; a:return 1; b:return 2; }",
        ));
        assert_eq!(cfg.indirect_dispatches().len(), 2);
        assert!(cfg.indirect_dispatches().iter().all(|info| {
            info.precision == crate::syntax::cfg::TargetPrecision::Exact
                && cfg.out_degree(info.node) == 2
        }));
    }

    #[test]
    fn a_direct_conditional_of_label_values_is_exact() {
        let cfg = graph("int f(int c) { goto *(c ? &&a : &&b); a:return 1; b:return 2; }");
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(info.precision, crate::syntax::cfg::TargetPrecision::Exact);
        assert_eq!(cfg.out_degree(info.node), 2);
    }

    #[test]
    fn an_extra_scalar_use_or_mutation_forces_the_safe_fallback() {
        let cfg = graph(concat!(
            "int f(int choose) { void *p = choose ? &&a : &&b; void *other=&&c; ",
            "consume(p); goto *p; a:return 1; b:return 2; c:return 3; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(
            info.precision,
            crate::syntax::cfg::TargetPrecision::Conservative
        );
        assert_eq!(cfg.out_degree(info.node), 3);
    }

    #[test]
    fn scalar_reassignments_form_a_narrow_conservative_may_set() {
        let cfg = graph(concat!(
            "int f(int choose) { void *p=&&a; if(choose) p=&&b; void *q=&&c; ",
            "goto *p; a:return 1; b:return 2; c:return q != 0; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(
            info.precision,
            crate::syntax::cfg::TargetPrecision::Conservative
        );
        assert_eq!(
            info.reasons,
            [crate::syntax::cfg::DispatchUncertainty::FlowInsensitiveJoin]
        );
        assert!(!info.may_be_invalid);
        assert_eq!(cfg.out_degree(info.node), 2);
        assert_eq!(kind_count(&cfg, NodeKind::Label), 2);
    }

    #[test]
    fn an_unsupported_scalar_assignment_keeps_the_function_wide_fallback() {
        let cfg = graph(concat!(
            "int f(int choose, void *unknown) { void *p=&&a; if(choose) p=unknown; ",
            "void *q=&&b; goto *p; a:return 1; b:return q != 0; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(
            info.reasons,
            [crate::syntax::cfg::DispatchUncertainty::UnresolvedValue]
        );
        assert!(info.may_be_invalid);
        assert_eq!(cfg.out_degree(info.node), 2);
    }

    #[test]
    fn a_label_nested_in_unknown_arithmetic_does_not_claim_exact_resolution() {
        let cfg = graph(concat!(
            "int f(int n) { void *other = &&b; goto *(&&a + n); ",
            "a:return 1; b:return 2; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(
            info.precision,
            crate::syntax::cfg::TargetPrecision::Conservative
        );
        assert_eq!(cfg.out_degree(info.node), 2);
    }

    #[test]
    fn a_table_with_an_unknown_initializer_element_falls_back() {
        let cfg = graph(concat!(
            "int f(int n, void *p) { static void *t[] = {&&a, p}; ",
            "void *other = &&b; goto *t[n]; a:return 1; b:return 2; }",
        ));
        let info = &cfg.indirect_dispatches()[0];
        assert_eq!(
            info.precision,
            crate::syntax::cfg::TargetPrecision::Conservative
        );
        assert_eq!(cfg.out_degree(info.node), 2);
    }

    #[test]
    fn a_return_reaches_the_function_end() {
        let cfg = graph("int f(int a) { if (a) return 1; return 0; }");
        assert_eq!(kind_count(&cfg, NodeKind::Return), 2);
        assert_eq!(cfg.in_degree(cfg.exit()), 2);
    }

    #[test]
    fn an_unparsed_region_keeps_its_node() {
        // `@` starts no C statement, so the parser recovers with an error node.
        let cfg = graph("void f(void) { @ ; g(); }");
        assert!(cfg.node_count() >= 4, "REQ-CFG-11: the region kept a node");
    }

    #[test]
    fn unreachable_code_after_a_return_does_not_become_an_orphan_node() {
        // `validate` inside `graph` is the assertion: an emitted node with no
        // predecessor is a REQ-GEN-1 failure.
        let cfg = graph("int f(void) { return 1; g(); h(); }");
        assert_eq!(kind_count(&cfg, NodeKind::Stmt), 0);
    }

    #[test]
    fn a_label_in_otherwise_unreachable_code_is_still_emitted() {
        let cfg = graph("int f(int a) { if (a) goto tail; return 1; tail: return 2; }");
        assert_eq!(kind_count(&cfg, NodeKind::Label), 1);
    }

    #[test]
    fn a_declaration_before_a_switchs_first_arm_is_not_reachable() {
        let cfg = graph("void f(int a) { switch (a) { int y = 1; case 1: g(); } }");
        assert_eq!(kind_count(&cfg, NodeKind::Case), 1);
    }

    // --- robustness ----------------------------------------------------------

    #[test]
    fn a_function_with_no_body_yields_the_two_anchor_nodes() {
        let text = "int f(int a) { return a; }";
        let tree = parse(text).into_parts().0;
        let mut func = tree.functions(text).remove(0);
        func.body = None;
        let cfg = function_cfg(&tree, text, &func).into_parts().0;
        assert_eq!(cfg.node_count(), 2);
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn deeply_nested_blocks_do_not_overflow_the_stack() {
        let depth = 5_000;
        let text = format!(
            "void f(void) {{ {} g(); {} }}",
            "{ ".repeat(depth),
            "} ".repeat(depth)
        );
        let cfg = graph(&text);
        assert!(cfg.node_count() >= 3);
    }

    #[test]
    fn deeply_nested_conditionals_do_not_overflow_the_stack() {
        let depth = 4_000;
        let text = format!("int f(int a) {{ if (a) {} g(); }}", "if (a) ".repeat(depth));
        let cfg = graph(&text);
        assert!(cfg.node_count() > depth);
    }

    #[test]
    fn a_deep_short_circuit_chain_does_not_overflow_the_stack() {
        let depth = 4_000;
        let text = format!("int f(int a) {{ return {}a; }}", "a && ".repeat(depth));
        let tree = parse(&text).into_parts().0;
        let functions = tree.functions(&text);
        let cfg = function_cfg(&tree, &text, &functions[0]).into_parts().0;
        assert!(cfg.node_count() > depth, "every operand but the last forks");
        assert!(cfg.validate().is_empty());
    }

    #[test]
    fn a_deeply_parenthesised_expression_does_not_overflow_the_stack() {
        let depth = 10_000;
        let text = format!(
            "int f(int a) {{ return {}a{}; }}",
            "(".repeat(depth),
            ")".repeat(depth)
        );
        let tree = parse(&text).into_parts().0;
        let functions = tree.functions(&text);
        let cfg = function_cfg(&tree, &text, &functions[0]).into_parts().0;
        assert!(cfg.node_count() >= 3);
    }

    #[test]
    fn malformed_input_yields_a_graph_rather_than_a_panic() {
        for text in [
            "void f(void) { if",
            "void f(void) { while (",
            "void f(void) { switch (a) { case",
            "void f(void) { do } while",
            "void f(void) { break; continue; }",
            "void f(void) { goto; }",
            "void f(void) { for (;;) { } }",
            "void f(void) { a ? : b; }",
            "void f(void) { case 1: g(); }",
            "void f(void) { } } } {",
            "int f(int a) { return a && ; }",
            "void f(void) { __label__ x; x: g(); }",
        ] {
            let tree = parse(text).into_parts().0;
            for func in tree.functions(text) {
                let cfg = function_cfg(&tree, text, &func).into_parts().0;
                assert!(cfg.node_count() >= 2, "{text}");
            }
        }
    }

    #[test]
    fn the_same_input_yields_the_same_graph_every_time() {
        let text = "int f(int a, int b) { if (a && b) { while (a) a--; } return a ? b : 0; }";
        let tree = parse(text).into_parts().0;
        let functions = tree.functions(text);
        let first = function_cfg(&tree, text, &functions[0]).into_parts().0;
        let second = function_cfg(&tree, text, &functions[0]).into_parts().0;
        assert_eq!(first, second, "REQ-SYN-5 / REQ-OUT-3");
    }

    /// The two back-edge defects this module found in [`crate::syntax::cfg`]
    /// stay fixed.
    ///
    /// Both were found from outside, by being the substrate's first real
    /// consumer, and both were fixed there rather than tolerated here: a loop
    /// formed by a backward `goto` was not recognised as a loop, and a
    /// `do`-`while`'s `continue` was marked a back edge although a bottom-tested
    /// loop's test sits *below* its body. The streams below are the two
    /// reproducers, written as events so that a regression shows up here with
    /// no C in the way; the C that produces each is in the comment above it.
    #[test]
    fn the_two_back_edge_defects_this_module_found_stay_fixed() {
        use crate::syntax::cfg::CfgBuilder;

        // void f(int c) { goto head; body: g(); head: if (c) goto body; }
        let head = Symbol::new(0);
        let body = Symbol::new(1);
        let mut builder = CfgBuilder::new(Span::new(0, 100));
        builder.extend(vec![
            Flow::Goto {
                label: head,
                span: Span::new(0, 1),
            },
            Flow::Label {
                name: body,
                span: Span::new(10, 11),
            },
            Flow::Stmt(Span::new(20, 21)),
            Flow::Label {
                name: head,
                span: Span::new(30, 31),
            },
            Flow::Branch {
                cond: Span::new(40, 41),
            },
            Flow::Then,
            Flow::Goto {
                label: body,
                span: Span::new(50, 51),
            },
            Flow::Else,
            Flow::EndScope,
        ]);
        let (goto_loop, diagnostics) = builder.finish().into_parts();
        assert!(diagnostics.is_empty());
        let reported: Vec<String> = goto_loop
            .validate()
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            reported.is_empty(),
            "a goto-formed loop is a loop: {reported:#?}"
        );

        // void f(int c) { do { g(); continue; } while (c); }
        let mut builder = CfgBuilder::new(Span::new(0, 100));
        builder.extend(vec![
            Flow::LoopHeader {
                kind: LoopKind::DoWhile,
                cond: Span::new(40, 41),
            },
            Flow::Stmt(Span::new(10, 11)),
            Flow::Continue {
                label: None,
                span: Span::new(20, 21),
            },
            Flow::EndScope,
        ]);
        let (do_while, diagnostics) = builder.finish().into_parts();
        assert!(diagnostics.is_empty());
        let reported: Vec<String> = do_while
            .validate()
            .iter()
            .map(|d| d.message.clone())
            .collect();
        assert!(
            reported.is_empty(),
            "a bottom-tested loop's continue jumps forwards: {reported:#?}"
        );

        // The same two, through the whole front end this time.
        for text in [
            "void f(int c) { goto head; body: g(); head: if (c) goto body; }",
            "void f(int c) { do { g(); continue; } while (c); }",
        ] {
            graph(text);
        }
    }

    /// The short-circuit census moves when the operators do.
    ///
    /// A count that is merely non-zero proves nothing about what it counts, so
    /// the same body is measured three ways: with the operators, with each
    /// replaced by its non-short-circuiting bitwise cousin, and with one more
    /// added. Only the middle one may be zero.
    #[test]
    fn the_short_circuit_census_is_sensitive_to_the_operators_it_counts() {
        let forks = |text: &str| {
            let tree = parse(text).into_parts().0;
            function_cfgs(&tree, text)
                .into_parts()
                .0
                .iter()
                .map(|g| g.short_circuits)
                .sum::<u32>()
        };
        let body =
            "int f(int a, int b, int c) { int d = a && b; if (b || c) d = a ? b : c; return d; }";
        let bitwise =
            "int f(int a, int b, int c) { int d = a & b; if (b | c) d = a + b + c; return d; }";
        let more = "int f(int a, int b, int c) { int d = a && b; if (b || c) d = a ? b : c; return d && c; }";
        assert_eq!(forks(body), 3, "one per &&, || and ?:");
        assert_eq!(
            forks(bitwise),
            0,
            "the bitwise operators are not control flow"
        );
        assert_eq!(forks(more), 4, "one more operator, one more fork");
    }

    // --- the corpus gates ----------------------------------------------------

    /// Every `.c` file directly under `dir`, sorted, or an empty list when the
    /// directory is absent --- which is how the gates below skip cleanly.
    fn c_files(dir: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut found: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "c"))
            .collect();
        found.sort();
        found
    }

    /// Every `.c` file anywhere under `root`, walked with an explicit stack.
    fn every_c(root: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.filter_map(|entry| entry.ok()) {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|ext| ext == "c") {
                    found.push(path);
                }
            }
        }
        found.sort();
        found
    }

    /// What one corpus lane cost, and every number the gate reports.
    #[derive(Default)]
    struct Census {
        files: usize,
        functions: usize,
        nodes: usize,
        edges: usize,
        forks: usize,
        with_forks: usize,
        diagnostics: usize,
        failures: usize,
        /// Failures that are an inescapable loop in the program; see
        /// [`inescapable_region_allowance`].
        no_exit: usize,
        /// How many functions those failures came from.
        no_exit_functions: usize,
        /// Failures this module's mapping is answerable for. Must be empty.
        unexplained: Vec<String>,
    }

    impl Census {
        fn report(&self, what: &str) {
            println!("{what}");
            println!("  files                        = {}", self.files);
            println!("  functions                    = {}", self.functions);
            println!("  CFGs built                   = {}", self.functions);
            println!("  validation failures          = {}", self.failures);
            println!(
                "    inescapable loop regions   = {} in {} function(s)",
                self.no_exit, self.no_exit_functions
            );
            println!(
                "    unexplained                = {}",
                self.unexplained.len()
            );
            println!("  CFG nodes                    = {}", self.nodes);
            println!("  CFG edges                    = {}", self.edges);
            println!("  build diagnostics            = {}", self.diagnostics);
            println!("  functions with &&/||/?:      = {}", self.with_forks);
            println!("  short-circuit forks emitted  = {}", self.forks);
        }
    }

    /// How many of `cfg`'s validation failures are `REQ-GEN-1`'s second
    /// invariant --- "every path reaches the function end or a diverging
    /// construct" --- being false *of the program*, not of the graph.
    ///
    /// A loop written with `goto` and never left is inescapable, and the honest
    /// graph of one has no edge out of it: `void f(void) { spin: g(); goto
    /// spin; }` reaches its function end on no path at all, and neither does an
    /// inner spin loop inside a function that otherwise returns. `while (1) {}`
    /// escapes the same verdict only because the builder gives every loop
    /// header a second edge out of the construct; inventing that edge for a
    /// `goto` loop would be a lie about what can happen, and inventing a
    /// [`NodeKind::Diverge`] node would be a lie about where control goes.
    ///
    /// The allowance is narrow: it applies only when *every* stranded node
    /// still has a successor, which is what makes the stranded set a cycle
    /// rather than a dead end. A node that reaches nothing at all is a wiring
    /// bug in this module and stays unexplained.
    ///
    /// Unlike the two back-edge defects this module found in the substrate --
    /// fixed in `98d72b41`, and now guarded by
    /// `the_two_back_edge_defects_this_module_found_stay_fixed` -- this one does
    /// not go away when anything is fixed: it is what a program with an
    /// inescapable loop actually looks like.
    fn inescapable_region_allowance(cfg: &Cfg) -> usize {
        let reachable = cfg.reachable();
        let co_reachable = cfg.co_reachable();
        let stranded: Vec<NodeId> = (0..cfg.node_count())
            .filter(|index| reachable[*index] && !co_reachable[*index])
            .map(|index| NodeId::new(index as u32))
            .collect();
        if stranded.is_empty() || stranded.iter().any(|id| cfg.out_degree(*id) == 0) {
            return 0;
        }
        // A stranded set closed under successors in which every node has one
        // must contain a cycle, so the region really is inescapable. When the
        // whole function is that region its end is unreachable too, which is
        // the one further diagnostic this explains.
        let unreachable_end = usize::from(!reachable[cfg.exit().index()]);
        stranded.len() + unreachable_end
    }

    /// Build every function's graph in `files`, validating each.
    fn census(files: &[PathBuf]) -> Census {
        let mut census = Census {
            files: files.len(),
            ..Census::default()
        };
        for path in files {
            let raw = std::fs::read(path).expect("a readable corpus file");
            let text = String::from_utf8_lossy(&raw).into_owned();
            let tree = crate::csource::parse::parse(&text).into_parts().0;
            let (graphs, diagnostics) = function_cfgs(&tree, &text).into_parts();
            census.diagnostics += diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .count();
            for graph in &graphs {
                census.functions += 1;
                census.nodes += graph.cfg.node_count();
                census.edges += graph.cfg.edge_count();
                census.forks += graph.short_circuits as usize;
                if graph.short_circuits > 0 {
                    census.with_forks += 1;
                }
                let reported = graph.cfg.validate();
                let no_exit = inescapable_region_allowance(&graph.cfg);
                census.failures += reported.len();
                census.no_exit += no_exit;
                census.no_exit_functions += usize::from(no_exit > 0);
                if reported.len() > no_exit {
                    for failure in reported.iter() {
                        census.unexplained.push(format!(
                            "{}: {}: {}",
                            path.display(),
                            graph.name,
                            failure.message
                        ));
                    }
                }
            }
        }
        census
    }

    /// Assert a lane's gate: every function built a graph, no failure this
    /// module is answerable for, and `REQ-CFG-6` actually fired.
    fn assert_gate(census: &Census) {
        let sample: Vec<&String> = census.unexplained.iter().take(20).collect();
        assert!(
            census.unexplained.is_empty(),
            "{} function(s) built a CFG this module is answerable for:\n{sample:#?}",
            census.unexplained.len()
        );
        assert!(
            census.forks > 0,
            "REQ-CFG-6 is not implemented if no corpus fork was emitted"
        );
    }

    #[test]
    fn every_function_of_the_in_repo_corpus_builds_a_valid_cfg() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<PathBuf> = Vec::new();
        for relative in ["tests/decompiler_fixtures/src", "tests/decbench_corpus/src"] {
            files.extend(
                crate::test_corpus::sources(&root.join(relative))
                    .into_iter()
                    .map(|(path, _)| path),
            );
        }
        let census = census(&files);
        census.report("in-repo corpus");
        assert_gate(&census);
    }

    #[test]
    fn real_decompiler_output_builds_valid_cfgs() {
        // Lives outside the repository, so absence is a skip rather than a
        // failure --- the same demand switch `csource::parse`'s own corpus gate
        // uses, so the two lanes stay comparable.
        let Ok(home) = std::env::var("HOME") else {
            println!("SKIP: no HOME");
            return;
        };
        let root = PathBuf::from(home).join(".cache/glaurung/decbench-full/tree");
        let files = if std::env::var("GLAURUNG_PARSE_FULL_CORPUS").is_ok() {
            every_c(&root)
        } else {
            c_files(&root.join("O0/zlib/decompiled"))
        };
        if files.is_empty() {
            println!("SKIP: no decompiled corpus under {}", root.display());
            return;
        }
        let census = census(&files);
        census.report("decompiled corpus");
        assert_gate(&census);
    }
}
