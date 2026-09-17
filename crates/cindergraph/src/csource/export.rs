//! Turning a parsed C file into serializable graphs.
//!
//! The writers live in [`crate::syntax::graph_export`], which knows nothing
//! about C. This module is the other half: it reads a [`Tree`] and the
//! [`Cfg`]s built from it and produces the labelled views those writers take.
//!
//! [`Repr`] uses the conventional AST/CFG/DDG/CDG/PDG vocabulary. It does not
//! expose a code property graph, and the dependence representations describe
//! Cindergraph's own analysis rather than another tool's internal graph.
//!
//! * [`Repr::Ddg`] labels every edge with the variable the dependence is
//!   about. `joern-export --repr ddg` does too, in its DOT; pyjoern's
//!   `Function.ddg` does not, so a Python caller of that API cannot tell which
//!   value an edge is for.
//! * [`Repr::Cdg`] labels every edge with the *arm* of the branch that decides
//!   it, so "runs when the guard holds" and "runs when it does not" are
//!   distinguishable rather than both being a bare pair.
//! * [`Repr::Pdg`] is the union, with each edge tagged `control` or `data`, on
//!   one node set --- which is what makes a slice computable from it.
//!
//! # Which control-flow graph
//!
//! [`Repr::Cfg`] exports [`crate::csource::cfg`], the general graph, and never
//! [`crate::csource::parity`]. The parity projection carries comparison-specific
//! expression and contraction choices that ordinary graph consumers should not
//! inherit. A caller that explicitly needs it can use
//! [`crate::csource::parity::parity_cfgs`].
//!
//! # Totality
//!
//! A file with no recovered C functions exports zero graphs plus diagnostics;
//! partial functions retain the graph that could be built. Span slicing goes
//! through a checked helper and produces an empty label for an invalid UTF-8
//! boundary instead of indexing the string directly.

use crate::csource::cfg::FunctionCfg;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::csource::semantic::expr_types::{
    CType, ExpressionTyper, ExpressionTypes, Layer, ParameterFacts,
};
use crate::csource::semantic::AnalysisUnit;
use crate::syntax::cfg::{Cfg, NodeKind};
use crate::syntax::diag::Parsed;
use crate::syntax::dominance::ControlDependence;
use crate::syntax::graph_export::{ExportEdge, ExportNode, GraphView};
use crate::syntax::ids::{NodeId, Span};

mod loops;
mod ops;

/// How long a source snippet in a node label may get, in characters.
///
/// Long enough to identify the statement, short enough that a Graphviz node
/// stays a box rather than a paragraph.
const LABEL_CHARS: usize = 48;

/// Which graph to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Repr {
    /// The general control-flow graph, one per function.
    Cfg,
    /// The syntax tree, one per function definition.
    Ast,
    /// The data-dependence graph: definitions, uses, and the reaching edges
    /// between them.
    Ddg,
    /// The control-dependence graph: which branch decides each statement.
    Cdg,
    /// The program-dependence graph: control and data dependence on one node
    /// set, which is the graph a slice is taken from.
    Pdg,
    /// The typed operations: each function's evaluation lowering as a list of
    /// operations with C types, explicit conversions, value inputs and CFG
    /// positions. Nodes are operations and edges are value flow.
    Ops,
}

impl Repr {
    /// Every representation, in declaration order, for a CLI choice list.
    pub const ALL: [Repr; 6] = [
        Repr::Cfg,
        Repr::Ast,
        Repr::Ddg,
        Repr::Cdg,
        Repr::Pdg,
        Repr::Ops,
    ];

    /// This representation's stable lowercase name, as a CLI accepts it.
    pub const fn name(self) -> &'static str {
        match self {
            Repr::Cfg => "cfg",
            Repr::Ast => "ast",
            Repr::Ddg => "ddg",
            Repr::Cdg => "cdg",
            Repr::Pdg => "pdg",
            Repr::Ops => "ops",
        }
    }

    /// Stable graph-identity name used when coordinating graph-local IDs.
    pub const fn graph_kind(self) -> &'static str {
        match self {
            Repr::Cfg => "executable_cfg",
            Repr::Ast => "syntax_ast",
            Repr::Ddg => "data_dependence_graph",
            Repr::Cdg => "control_dependence_graph",
            Repr::Pdg => "program_dependence_graph",
            Repr::Ops => "typed_operations",
        }
    }

    /// Parses a representation name.
    pub fn parse(name: &str) -> Option<Repr> {
        match name.trim().to_ascii_lowercase().as_str() {
            "cfg" | "control-flow" | "control_flow" => Some(Repr::Cfg),
            "ast" | "tree" | "syntax" => Some(Repr::Ast),
            "ddg" | "dataflow" | "data-flow" => Some(Repr::Ddg),
            "cdg" | "control" | "control-dependence" => Some(Repr::Cdg),
            "pdg" | "program-dependence" => Some(Repr::Pdg),
            "ops" | "operations" | "typed-operations" | "typed_operations" => Some(Repr::Ops),
            _ => None,
        }
    }
}

/// Every function's graph in `text`, in source order.
///
/// Functions are returned as a list rather than a name-keyed map because two
/// definitions in one file can carry the same name after recovery, and a map
/// would silently drop one --- the same reason
/// [`crate::csource::metrics`] reports a list.
pub fn export(text: &str, repr: Repr) -> Parsed<Vec<GraphView>> {
    let unit = AnalysisUnit::new(text);
    let views = export_unit(&unit, repr);
    Parsed::new(views, unit.diagnostics().clone())
}

/// Every function's graph from an already-owned analysis snapshot.
///
/// Unlike [`export`], this does not parse, rebuild CFGs, or recompute dataflow.
/// It is the export path for session-oriented callers that need several graph
/// products to carry the same source and function identity.
pub fn export_unit(unit: &AnalysisUnit, repr: Repr) -> Vec<GraphView> {
    let tree = unit.tree();
    let text = unit.source();
    match repr {
        Repr::Cfg => unit
            .functions()
            .iter()
            .enumerate()
            .map(|(index, function)| {
                let mut view = cfg_view(&function.name, &function.cfg, text);
                mark_expression_internal(&mut view, &function.expression_internal);
                mark_loops(&mut view, unit, index, function);
                view
            })
            .collect(),
        Repr::Ast => {
            let spans = unit.token_spans();
            tree.functions(text)
                .iter()
                .map(|function| {
                    let semantics = unit
                        .functions()
                        .iter()
                        .position(|candidate| candidate.node == function.node)
                        .and_then(|index| unit.expression_typer(index))
                        .map(|typer| AstSemantics {
                            typer,
                            function_offset: function.span.lo,
                        });
                    ast_view_with(
                        &function.name,
                        tree,
                        function.node,
                        spans,
                        text,
                        semantics.as_ref(),
                    )
                })
                .collect()
        }
        Repr::Ddg => unit
            .dataflows()
            .iter()
            .map(|flow| ddg_view(flow, text))
            .collect(),
        Repr::Cdg => unit
            .functions()
            .iter()
            .enumerate()
            .map(|(index, function)| {
                let mut view = cdg_view(&function.name, &function.cfg, text);
                mark_expression_internal(&mut view, &function.expression_internal);
                mark_loops(&mut view, unit, index, function);
                view
            })
            .collect(),
        Repr::Pdg => unit
            .functions()
            .iter()
            .zip(unit.dataflows())
            .enumerate()
            .map(|(index, (function, flow))| {
                let mut view = pdg_view(&function.name, &function.cfg, flow, text);
                mark_expression_internal(&mut view, &function.expression_internal);
                mark_loops(&mut view, unit, index, function);
                view
            })
            .collect(),
        Repr::Ops => unit
            .functions()
            .iter()
            .zip(unit.evaluations())
            .enumerate()
            .map(|(index, (function, plan))| {
                let typer = unit.expression_typer(index);
                ops::ops_view(
                    &function.name,
                    text,
                    tree,
                    unit.token_spans(),
                    function,
                    plan,
                    typer.as_ref(),
                )
            })
            .collect(),
    }
}

/// Adds `expr_internal` to every node of a view over the CFG node set.
///
/// `true` marks a node that exists only because a `&&`, `||` or `?:` was
/// expanded into control flow ([`crate::csource::cfg::FunctionCfg::expression_internal`]);
/// a consumer enumerating statement-level paths collapses those into the node
/// they flow to. The value is written on every node, like `back` on edges, so
/// a reader never has to distinguish "absent" from "false".
fn mark_expression_internal(view: &mut GraphView, internal: &[NodeId]) {
    for node in view.nodes.iter_mut() {
        let flagged = internal.binary_search(&NodeId::new(node.id)).is_ok();
        node.attrs.push((
            "expr_internal".to_owned(),
            if flagged { "true" } else { "false" }.to_owned(),
        ));
    }
}

/// Adds the loop metadata ([`loops`]) to every `loop_header` node of a view
/// over the CFG node set of `function`, the function at `index` in the unit.
///
/// A `loop_header`'s span is the loop's condition (or the whole statement
/// when it has none), which is exactly the key [`loops::loops_by_header_span`]
/// returns, so the join is by span and never by position. Parameter bounds
/// are resolved through the unit's declaration resolution; without it a name
/// bound would be `runtime`.
fn mark_loops(view: &mut GraphView, unit: &AnalysisUnit, index: usize, function: &FunctionCfg) {
    let resolution = unit.resolutions().get(index);
    let is_parameter = resolution.map(|resolution| {
        move |name: &str, offset: u32| {
            resolution
                .resolve_at(name, offset)
                .is_some_and(|declaration| resolution.is_parameter(declaration))
        }
    });
    let is_parameter: Option<loops::IsParameter<'_>> = is_parameter
        .as_ref()
        .map(|predicate| predicate as &dyn Fn(&str, u32) -> bool);
    let facts = loops::loops_by_header_span(
        unit.tree(),
        function.node,
        unit.token_spans(),
        unit.source(),
        is_parameter,
    );
    for (position, node) in function.cfg.nodes().iter().enumerate() {
        if node.kind() != NodeKind::LoopHeader {
            continue;
        }
        let Some(facts) = facts.get(&node.span()) else {
            continue;
        };
        if let Some(exported) = view.nodes.get_mut(position) {
            for (key, value) in facts.attributes() {
                exported.attrs.push((key.to_owned(), value));
            }
        }
    }
}

/// One function's control-flow graph as a view.
///
/// Node labels carry the kind and the source the node covers, because a CFG
/// whose nodes read `stmt` eleven times tells a reader nothing. Edge labels
/// carry the edge kind; a back edge is marked in an attribute rather than the
/// label so the label stays the census key a consumer groups by.
///
/// Every node carries `kind`, `span` (`lo:hi`, **byte** offsets into the
/// analysed source, end exclusive --- slice bytes, not a decoded string) and
/// the 1-based `line` and byte `column` of `lo`. The unit-owned export path
/// ([`export_unit`]) adds `expr_internal`; see
/// [`crate::csource::cfg::FunctionCfg::expression_internal`].
pub fn cfg_view(name: &str, cfg: &Cfg, text: &str) -> GraphView {
    let mut view = GraphView::new(name);
    let lines = LineIndex::new(text);
    for (index, node) in cfg.nodes().iter().enumerate() {
        let kind = node.kind().name();
        let span = node.span();
        let text_of = snippet(text, span);
        let label = if text_of.is_empty() {
            kind.to_string()
        } else {
            format!("{kind}\n{text_of}")
        };
        let (line, column) = lines.position(span.lo);
        let mut exported = ExportNode::new(index as u32, label)
            .with("kind", kind)
            .with("span", format!("{}:{}", span.lo, span.hi))
            .with("line", line.to_string())
            .with("column", column.to_string());
        if let Some(info) = cfg.indirect_dispatch(NodeId::new(index as u32)) {
            let reasons = info
                .reasons
                .iter()
                .map(|reason| reason.name())
                .collect::<Vec<_>>()
                .join(",");
            exported = exported
                .with("dispatch_precision", info.precision.name())
                .with(
                    "dispatch_may_be_invalid",
                    if info.may_be_invalid { "true" } else { "false" },
                )
                .with("dispatch_reasons", reasons);
        }
        view.nodes.push(exported);
    }
    for edge in cfg.edges() {
        view.edges.push(
            ExportEdge::new(
                edge.src.index() as u32,
                edge.dst.index() as u32,
                edge.kind.name(),
            )
            .with("kind", edge.kind.name())
            .with("back", if edge.is_back { "true" } else { "false" }),
        );
    }
    view
}

/// One function definition's syntax tree as a view.
///
/// Every node is tagged; a node with no children also carries the source it
/// covers, which is what makes an exported AST readable at all. Interior nodes
/// are left to their tag, because their span is the union of their children's
/// and repeating it at every level is noise.
///
/// # Node attributes
///
/// * `tag` --- the [`NodeTag`] name.
/// * `span` --- `lo:hi`, **byte** offsets into the analysed source (the text
///   after any dialect normalization), end exclusive. They are not character
///   indices: slice the UTF-8 bytes, not a decoded string, or every span after
///   the first multi-byte character is off.
/// * `line`, `column` --- 1-based position of `span.lo`; the column counts
///   bytes from the start of the line, so it agrees with `span` on every input
///   and with an editor's column only on ASCII lines.
/// * `op` --- on `binary_expr`, `assign_expr`, `unary_expr`, `cond_expr` and
///   `inc_dec_suffix`: the operator token as written (`+`, `<<=`, `!`, `++`),
///   `?:` for a conditional. A flat chain (`a + b - c` is one `binary_expr`
///   with three operands; see `parse/tag.rs`) reports its first operator in
///   `op` and every operator, comma-separated in source order, in `ops`.
///   `ops` is present only on chains with more than one operator.
/// * `loop_kind`, `bound_kind`, `bound_expr`, `induction`, `step`,
///   `init_value`, `bound_value` --- on `for_stmt`, `while_stmt` and
///   `do_while_stmt`, what a bounded unroller needs (the `loops` submodule).
///   Here, with no declaration resolution, a bound that is a name is
///   `runtime`; the unit-owned paths classify a function parameter as
///   `parameter`.
///
/// The one-shot [`export`] and [`export_unit`] paths add the semantic
/// attributes (`type`, `operand_type`, declarator and parameter fields) that
/// need the owning [`AnalysisUnit`]; this tree-only builder carries the
/// syntactic ones above.
pub fn ast_view(
    name: &str,
    tree: &Tree,
    root: NodeId,
    token_spans: &[Span],
    text: &str,
) -> GraphView {
    ast_view_with(name, tree, root, token_spans, text, None)
}

/// [`ast_view`] plus the semantic attributes `semantics` can answer.
fn ast_view_with(
    name: &str,
    tree: &Tree,
    root: NodeId,
    token_spans: &[Span],
    text: &str,
    semantics: Option<&AstSemantics<'_>>,
) -> GraphView {
    let arena = tree.arena();
    let lines = LineIndex::new(text);
    let mut view = GraphView::new(name);
    // Dense output ids, assigned in preorder, so the export is stable and does
    // not leak arena indices that mean nothing outside this process.
    //
    // The reverse map is a slice indexed by arena id rather than a search
    // through `ids`: a linear lookup per child edge is quadratic in the node
    // count, and one recovered `sshd` function in the DecBench corpus carries
    // over a thousand statements.
    let mut ids: Vec<(NodeId, u32)> = Vec::new();
    let mut dense_of: Vec<Option<u32>> = vec![None; arena.len()];
    for node in arena.preorder(root) {
        let next = ids.len() as u32;
        if let Some(slot) = dense_of.get_mut(node.index()) {
            *slot = Some(next);
        }
        ids.push((node, next));
    }
    let dense = |node: NodeId| dense_of.get(node.index()).copied().flatten();
    let tables = semantics.map(|semantics| semantics.expression_types(root));
    let is_parameter = semantics
        .map(|semantics| move |name: &str, offset: u32| semantics.is_parameter(name, offset));
    let is_parameter: Option<loops::IsParameter<'_>> = is_parameter
        .as_ref()
        .map(|predicate| predicate as &dyn Fn(&str, u32) -> bool);

    for (node, id) in &ids {
        let tag = arena.tag(*node).and_then(NodeTag::from_u16);
        let tag_name = tag.map_or("?", |tag| tag.name());
        let leaf = arena.child_count(*node) == 0;
        let span = arena.span(*node, token_spans).unwrap_or_default();
        let text_of = if leaf {
            snippet(text, span)
        } else {
            String::new()
        };
        let label = if text_of.is_empty() {
            tag_name.to_string()
        } else {
            format!("{tag_name}\n{text_of}")
        };
        let (line, column) = lines.position(span.lo);
        let mut exported = ExportNode::new(*id, label)
            .with("tag", tag_name)
            .with("span", format!("{}:{}", span.lo, span.hi))
            .with("line", line.to_string())
            .with("column", column.to_string());
        if let Some(tag) = tag {
            let ops = operators_of(tree, *node, tag, token_spans, text);
            if let Some(first) = ops.first() {
                exported = exported.with("op", first.clone());
                if ops.len() > 1 {
                    exported = exported.with("ops", ops.join(","));
                }
            }
            if let (Some(tables), Some(semantics)) = (tables.as_ref(), semantics) {
                exported = semantics.decorate(exported, *node, tag, tables);
            }
            if let Some(facts) =
                loops::loop_facts(tree, root, *node, tag, token_spans, text, is_parameter)
            {
                for (key, value) in facts.attributes() {
                    exported = exported.with(key, value);
                }
            }
        }
        view.nodes.push(exported);
    }
    for (node, id) in &ids {
        for child in arena.children_iter(*node) {
            if let Some(child_id) = dense(child) {
                view.edges.push(ExportEdge::new(*id, child_id, ""));
            }
        }
    }
    view
}

/// The operator tokens of `node`, in source order, as written.
///
/// A binary or assignment chain is flat (one node per precedence level, see
/// `parse/tag.rs`), so its operators are the tokens in the gaps between
/// consecutive children; a comment in a gap is trivia and never a token, so
/// `a /* x */ + b` yields `+`. A prefix operator is the node's first token. A
/// conditional has no single token and is reported as `?:`. Anything else has
/// no operator and yields an empty list.
fn operators_of(
    tree: &Tree,
    node: NodeId,
    tag: NodeTag,
    token_spans: &[Span],
    text: &str,
) -> Vec<String> {
    let arena = tree.arena();
    let lexeme = |raw: u32| -> Option<String> {
        let span = token_spans.get(raw as usize)?;
        text.get(span.lo as usize..span.hi as usize)
            .map(str::to_owned)
    };
    match tag {
        NodeTag::BinaryExpr | NodeTag::AssignExpr => {
            let children: Vec<NodeId> = arena.children_iter(node).collect();
            children
                .windows(2)
                .filter_map(|pair| {
                    let (_, left_end) = arena.token_extent(pair[0])?;
                    let (right_start, _) = arena.token_extent(pair[1])?;
                    (left_end..right_start).find_map(lexeme)
                })
                .collect()
        }
        NodeTag::UnaryExpr | NodeTag::IncDecSuffix => arena
            .token_extent(node)
            .and_then(|(first, _)| lexeme(first))
            .into_iter()
            .collect(),
        NodeTag::CondExpr => vec!["?:".to_owned()],
        _ => Vec::new(),
    }
}

/// Byte offset to 1-based line and byte column, built once per file.
struct LineIndex {
    /// Byte offset of the first byte of every line; line 1 starts at 0.
    starts: Vec<u32>,
}

impl LineIndex {
    fn new(text: &str) -> Self {
        let mut starts = vec![0u32];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index as u32 + 1),
        );
        Self { starts }
    }

    /// `(line, column)`, both 1-based, of byte offset `at`.
    ///
    /// The column is a byte count, which is what keeps it consistent with
    /// `span` on a line with multi-byte characters. An offset past the end of
    /// the text lands on the last line.
    fn position(&self, at: u32) -> (u32, u32) {
        let line = self.starts.partition_point(|start| *start <= at).max(1);
        let start = self.starts[line - 1];
        (line as u32, at.saturating_sub(start) + 1)
    }
}

/// The semantic facts an owned [`AnalysisUnit`] lets the AST export publish.
///
/// Everything here is answered by the semantic layer, never re-derived from
/// source text: expression types by [`ExpressionTyper`], declarator and
/// parameter types by the structural type graph it reads.
struct AstSemantics<'a> {
    typer: ExpressionTyper<'a>,
    function_offset: u32,
}

/// The per-function tables [`AstSemantics::decorate`] reads.
struct AstSemanticTables {
    expressions: ExpressionTypes,
    parameters: std::collections::BTreeMap<NodeId, ParameterFacts>,
}

impl AstSemantics<'_> {
    /// Whether `name`, referenced at byte `offset`, resolves to one of the
    /// function's own parameters.
    fn is_parameter(&self, name: &str, offset: u32) -> bool {
        self.typer
            .resolution
            .resolve_at(name, offset)
            .is_some_and(|declaration| self.typer.resolution.is_parameter(declaration))
    }

    fn expression_types(&self, root: NodeId) -> AstSemanticTables {
        AstSemanticTables {
            expressions: self.typer.compute(root),
            parameters: self.typer.parameters(root, self.function_offset),
        }
    }

    /// Adds the semantic attributes for `node` to its export.
    ///
    /// * Expression nodes (`name_ref`, `literal`, `paren_expr`, `comma_expr`,
    ///   `unary_expr`, `binary_expr`, `cast_expr`, `cond_expr`, `assign_expr`,
    ///   `postfix_expr`, `sizeof_type`, `alignof_type`, `compound_literal`,
    ///   `stmt_expr`, `builtin_expr`, `label_addr`) carry `type`: the C type of
    ///   the expression after lvalue conversion, integer promotion and the
    ///   usual arithmetic conversions, or `unknown`. A `binary_expr` or
    ///   `assign_expr` whose operator converts its operands also carries
    ///   `operand_type`, the common type the operands of `op` are converted to
    ///   (for a shift, the promoted left operand; for a plain `=`, the
    ///   assigned-to type); a chain carries `operand_types`, one per operator.
    ///   `&&` and `||` convert nothing and carry no operand type.
    /// * `declarator` carries `name` and, when the structural layer resolved
    ///   the declaration, `type` (the written type); an array declarator adds
    ///   `element_type`, `array_bound` (`constant`, `runtime`, `incomplete` or
    ///   `star`) and, for a constant bound, `count`.
    /// * `param_decl` carries `type` (the adjusted parameter type: arrays and
    ///   functions become pointers), `pointer_depth`, and `name` when the
    ///   parameter has one.
    fn decorate(
        &self,
        mut node: ExportNode,
        id: NodeId,
        tag: NodeTag,
        tables: &AstSemanticTables,
    ) -> ExportNode {
        match tag {
            NodeTag::Declarator => {
                let facts = self.typer.declarator(id);
                if let Some(name) = facts.name {
                    node = node.with("name", name);
                }
                if let Some(declared) = facts.declared {
                    node = node.with("type", declared.render());
                    if let Some(Layer::Array(bound)) = declared.layers.first() {
                        let kind = match bound.as_str() {
                            "" => "incomplete",
                            "*" => "star",
                            digits if digits.bytes().all(|byte| byte.is_ascii_digit()) => {
                                "constant"
                            }
                            _ => "runtime",
                        };
                        node = node.with("array_bound", kind);
                        if kind == "constant" {
                            node = node.with("count", bound.clone());
                        }
                        if let Some(element) = declared.clone().pointee() {
                            node = node.with("element_type", element.render());
                        }
                    }
                }
                node
            }
            NodeTag::ParamDecl => match tables.parameters.get(&id) {
                Some(facts) => {
                    node = node
                        .with("type", facts.adjusted.render())
                        .with("pointer_depth", facts.adjusted.pointer_depth().to_string());
                    if let Some(name) = &facts.name {
                        node = node.with("name", name.clone());
                    }
                    node
                }
                None => node,
            },
            _ => {
                let Some(ty) = tables.expressions.type_of(id) else {
                    return node;
                };
                node = node.with("type", ty.render());
                if let Some(operands) = tables.expressions.operand_types_of(id) {
                    if let Some(first) = operands.first() {
                        node = node.with("operand_type", first.render());
                    }
                    if operands.len() > 1 {
                        let rendered: Vec<String> = operands.iter().map(CType::render).collect();
                        node = node.with("operand_types", rendered.join(","));
                    }
                }
                node
            }
        }
    }
}

/// One function's data-dependence graph as a view.
///
/// Nodes are the definitions and then the uses, in that order, so a node id is
/// stable and a reader can tell the two halves apart by the `role` attribute
/// without following an edge. Each edge is labelled with its variable, which
/// is the information the external comparison drops.
///
/// A dead store and an unresolved use are marked on the node rather than left
/// for the reader to derive from degree, because "this write is never read" is
/// the answer someone exports this graph to get.
pub fn ddg_view(flow: &crate::csource::dataflow::DataFlow, text: &str) -> GraphView {
    use crate::csource::dataflow::DataFlow;

    let mut view = GraphView::new(&flow.name);
    let lines = LineIndex::new(text);
    let position = |span: Span| {
        let (line, column) = lines.position(span.lo);
        (line.to_string(), column.to_string())
    };
    let def_count = flow.definitions.len() as u32;
    let use_count = flow.uses.len() as u32;
    let memory_def_start = def_count + use_count;
    let memory_use_start = memory_def_start + flow.memory_definitions.len() as u32;

    for (index, definition) in flow.definitions.iter().enumerate() {
        let dead = flow.is_dead_store(index as u32);
        let source = snippet(text, definition.span);
        view.nodes.push(
            ExportNode::new(
                index as u32,
                format!(
                    "def {}{}",
                    definition.name,
                    if dead { " (dead)" } else { "" }
                ),
            )
            .with("role", "definition")
            .with("variable", definition.name.clone())
            .with("def_kind", definition.kind.name())
            .with("dead_store", if dead { "true" } else { "false" })
            .with("cfg_node", definition.node.to_string())
            .with(
                "span",
                format!("{}:{}", definition.span.lo, definition.span.hi),
            )
            .with("line", position(definition.span).0)
            .with("column", position(definition.span).1)
            .with("text", source),
        );
    }
    for (index, use_) in flow.uses.iter().enumerate() {
        let unresolved = flow.unresolved_uses.contains(&(index as u32));
        let source = snippet(text, use_.span);
        view.nodes.push(
            ExportNode::new(
                def_count + index as u32,
                format!(
                    "use {}{}",
                    use_.name,
                    if unresolved { " (unresolved)" } else { "" }
                ),
            )
            .with("role", "use")
            .with("variable", use_.name.clone())
            .with("unresolved", if unresolved { "true" } else { "false" })
            .with("cfg_node", use_.node.to_string())
            .with("span", format!("{}:{}", use_.span.lo, use_.span.hi))
            .with("line", position(use_.span).0)
            .with("column", position(use_.span).1)
            .with("text", source),
        );
    }
    for edge in &flow.edges {
        view.edges.push(
            ExportEdge::new(edge.def, def_count + edge.use_, edge.name.clone())
                .with("variable", edge.name.clone()),
        );
    }
    for (index, definition) in flow.memory_definitions.iter().enumerate() {
        let name = flow
            .memory_region_name(definition.region)
            .unwrap_or_else(|| format!("region#{}", definition.region.0));
        view.nodes.push(
            ExportNode::new(
                memory_def_start + index as u32,
                format!("memory def {name}"),
            )
            .with("role", "memory_definition")
            .with("region", definition.region.0.to_string())
            .with("memory", name)
            .with("precision", definition.precision.name())
            .with("cfg_node", definition.node.to_string())
            .with(
                "span",
                format!("{}:{}", definition.span.lo, definition.span.hi),
            )
            .with("line", position(definition.span).0)
            .with("column", position(definition.span).1)
            .with("text", snippet(text, definition.span)),
        );
    }
    for (index, use_) in flow.memory_uses.iter().enumerate() {
        let name = flow
            .memory_region_name(use_.region)
            .unwrap_or_else(|| format!("region#{}", use_.region.0));
        view.nodes.push(
            ExportNode::new(
                memory_use_start + index as u32,
                format!("memory use {name}"),
            )
            .with("role", "memory_use")
            .with("region", use_.region.0.to_string())
            .with("memory", name)
            .with("precision", use_.precision.name())
            .with("cfg_node", use_.node.to_string())
            .with("span", format!("{}:{}", use_.span.lo, use_.span.hi))
            .with("line", position(use_.span).0)
            .with("column", position(use_.span).1)
            .with("text", snippet(text, use_.span)),
        );
    }
    for edge in &flow.memory_edges {
        let name = flow
            .memory_region_name(edge.use_region)
            .unwrap_or_else(|| format!("region#{}", edge.use_region.0));
        view.edges.push(
            ExportEdge::new(
                memory_def_start + edge.definition,
                memory_use_start + edge.use_,
                name.clone(),
            )
            .with("memory", name)
            .with("definition_region", edge.definition_region.0.to_string())
            .with("use_region", edge.use_region.0.to_string())
            .with("overlap", edge.overlap.map_or("same", |kind| kind.name())),
        );
    }
    // Silence the unused-import warning in builds where the type alias is the
    // only reference; the parameter above already names it.
    let _: Option<&DataFlow> = None;
    view
}

/// One function's control-dependence graph as a view.
///
/// Nodes are the CFG's own nodes, so a reader can line this up against
/// `--repr cfg` node for node. Each edge carries the arm of the branch that
/// decides it, and each node carries its control-dependence depth --- the
/// length of the longest chain of decisions above it, computed on the graph
/// rather than from the syntax, so a `goto` out of a block or a decompiler's
/// flattened dispatch cannot fool it the way a brace count can.
pub fn cdg_view(name: &str, cfg: &Cfg, text: &str) -> GraphView {
    let cdg = ControlDependence::of(cfg);
    let post = cdg.post_dominators();
    let mut view = GraphView::new(name);
    let lines = LineIndex::new(text);

    for (index, node) in cfg.nodes().iter().enumerate() {
        let id = index as u32;
        let kind = node.kind().name();
        let span = node.span();
        let (line, column) = lines.position(span.lo);
        let source = snippet(text, span);
        let label = if source.is_empty() {
            kind.to_string()
        } else {
            format!("{kind}\n{source}")
        };
        let mut export = ExportNode::new(id, label)
            .with("kind", kind)
            .with("depth", cdg.depth(id).to_string())
            .with("span", format!("{}:{}", span.lo, span.hi))
            .with("line", line.to_string())
            .with("column", column.to_string());
        if let Some(parent) = post.immediate(id) {
            export = export.with("ipdom", parent.to_string());
        }
        if post.dead_ends().contains(&id) {
            // The function end is unreachable from here: an infinite loop, a
            // `noreturn` call, or a transfer the builder could not resolve.
            export = export.with("reaches_exit", "false");
        }
        view.nodes.push(export);
    }
    for edge in cdg.edges() {
        view.edges.push(
            ExportEdge::new(edge.on, edge.node, edge.kind.name()).with("kind", edge.kind.name()),
        );
    }
    view
}

/// One function's program-dependence graph: control and data on one node set.
///
/// The union is the point. A control-dependence graph says which branch
/// decides a statement; a data-dependence graph says which write a read sees;
/// a slice needs both at once, and it needs them over the *same* nodes. So the
/// nodes here are the CFG's, the control edges are as in [`cdg_view`], and
/// each data edge is lifted from its (definition, use) pair to the pair of CFG
/// nodes those sit on. Every edge is tagged `control` or `data`.
///
/// Lifting loses the within-node ordering the data-dependence graph has, which
/// is why [`Repr::Ddg`] still exists separately: for reading dependences it is
/// the more precise graph, and this one is for slicing.
pub fn pdg_view(
    name: &str,
    cfg: &Cfg,
    flow: &crate::csource::dataflow::DataFlow,
    text: &str,
) -> GraphView {
    let mut view = cdg_view(name, cfg, text);
    for edge in view.edges.iter_mut() {
        *edge = edge.clone().with("dependence", "control");
    }

    // Data edges, lifted to the CFG nodes their endpoints sit on. A dependence
    // wholly inside one node adds a self-edge, which is real --- `x = x + 1`
    // on one straight-line node does depend on itself.
    let mut seen: Vec<(u32, u32, String)> = Vec::new();
    for edge in &flow.edges {
        let (Some(definition), Some(use_)) = (
            flow.definitions.get(edge.def as usize),
            flow.uses.get(edge.use_ as usize),
        ) else {
            continue;
        };
        let key = (definition.node, use_.node, edge.name.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        view.edges.push(
            ExportEdge::new(definition.node, use_.node, edge.name.clone())
                .with("dependence", "data")
                .with("variable", edge.name.clone()),
        );
    }
    for edge in &flow.memory_edges {
        let (Some(definition), Some(use_)) = (
            flow.memory_definitions.get(edge.definition as usize),
            flow.memory_uses.get(edge.use_ as usize),
        ) else {
            continue;
        };
        let name = flow
            .memory_region_name(edge.use_region)
            .unwrap_or_else(|| format!("region#{}", edge.use_region.0));
        let key = (definition.node, use_.node, name.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        view.edges.push(
            ExportEdge::new(definition.node, use_.node, name.clone())
                .with("dependence", "data")
                .with("memory", name)
                .with("definition_region", edge.definition_region.0.to_string())
                .with("use_region", edge.use_region.0.to_string())
                .with("overlap", edge.overlap.map_or("same", |kind| kind.name())),
        );
    }
    view
}

/// The source `span` covers, collapsed to one line and cut to [`LABEL_CHARS`].
///
/// Returns an empty string rather than panicking when the span is empty, is out
/// of range, or does not land on character boundaries. All three are reachable:
/// a recovered parse inserts empty spans, and a node can cover multi-byte text.
fn snippet(text: &str, span: Span) -> String {
    let Some(raw) = text.get(span.lo as usize..span.hi as usize) else {
        return String::new();
    };
    let mut out = String::with_capacity(raw.len().min(LABEL_CHARS * 2));
    let mut space = false;
    let mut taken = 0usize;
    for ch in raw.chars() {
        if taken >= LABEL_CHARS {
            out.push_str("...");
            break;
        }
        if ch.is_whitespace() {
            space = !out.is_empty();
            continue;
        }
        if space {
            out.push(' ');
            taken += 1;
            space = false;
        }
        out.push(ch);
        taken += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::graph_export::{write, Format};

    const HELLO: &str = r#"
int greet(const char *name, int times)
{
    if (name == 0) {
        return -1;
    }
    for (int i = 0; i < times; i++) {
        puts(name);
    }
    return times;
}
"#;

    #[test]
    fn an_owned_unit_exports_every_representation_without_a_second_front_end() {
        let unit = AnalysisUnit::new(HELLO);
        for repr in Repr::ALL {
            let from_unit = export_unit(&unit, repr);
            let one_shot = export(HELLO, repr).into_parts().0;
            assert_eq!(from_unit, one_shot, "{} export differs", repr.name());
        }
    }

    #[test]
    fn a_cfg_export_names_the_function_and_its_branches() {
        let views = export(HELLO, Repr::Cfg).into_parts().0;
        assert_eq!(views.len(), 1);
        let view = &views[0];
        assert_eq!(view.name, "greet");
        assert!(view.nodes.len() > 4, "{} nodes", view.nodes.len());
        assert!(view
            .nodes
            .iter()
            .any(|node| node.attrs.iter().any(|(k, v)| k == "kind" && v == "entry")));
        assert!(view.nodes.iter().any(|node| {
            node.attrs
                .iter()
                .any(|(k, v)| k == "kind" && v == "loop_header")
        }));
        assert!(view
            .edges
            .iter()
            .any(|edge| edge.attrs.iter().any(|(k, v)| k == "back" && v == "true")));
    }

    #[test]
    fn a_cfg_node_label_carries_the_source_it_covers() {
        let views = export(HELLO, Repr::Cfg).into_parts().0;
        let labels: Vec<&str> = views[0].nodes.iter().map(|n| n.label.as_str()).collect();
        assert!(
            labels.iter().any(|label| label.contains("puts(name)")),
            "{labels:?}"
        );
    }

    #[test]
    fn a_cfg_export_carries_indirect_dispatch_uncertainty() {
        let source = concat!(
            "int f(int n) { static void *table[] = {&&a, &&b}; ",
            "goto *table[n]; a: return 1; b: return 2; }",
        );
        let views = export(source, Repr::Cfg).into_parts().0;
        let dispatch = views[0]
            .nodes
            .iter()
            .find(|node| {
                node.attrs
                    .iter()
                    .any(|(key, value)| key == "kind" && value == "indirect_dispatch")
            })
            .expect("one exported indirect dispatch");
        let attr = |key: &str| {
            dispatch
                .attrs
                .iter()
                .find(|(candidate, _)| candidate == key)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(attr("dispatch_precision"), Some("exact"));
        assert_eq!(attr("dispatch_may_be_invalid"), Some("true"));
        assert_eq!(attr("dispatch_reasons"), Some("index_may_be_out_of_bounds"));
    }

    #[test]
    fn an_ast_export_is_a_tree_with_one_root() {
        let views = export(HELLO, Repr::Ast).into_parts().0;
        assert_eq!(views.len(), 1);
        let view = &views[0];
        // A tree over n nodes has exactly n-1 edges, and every node but the
        // root is the destination of exactly one of them.
        assert_eq!(view.edges.len(), view.nodes.len() - 1);
        let mut targets: Vec<u32> = view.edges.iter().map(|e| e.dst).collect();
        targets.sort_unstable();
        targets.dedup();
        assert_eq!(targets.len(), view.nodes.len() - 1);
        assert!(!targets.contains(&0), "node 0 is the root");
    }

    #[test]
    fn an_ast_leaf_carries_its_source_and_an_interior_node_does_not() {
        let views = export(HELLO, Repr::Ast).into_parts().0;
        let view = &views[0];
        assert!(
            view.nodes.iter().any(|n| n.label.contains('\n')),
            "no leaf label carried source"
        );
        let root = &view.nodes[0];
        assert!(!root.label.contains('\n'), "root label: {}", root.label);
    }

    #[test]
    fn every_representation_and_format_is_total_on_junk() {
        for junk in [
            "",
            "\u{0}\u{1}not C at all",
            "int f(",
            "}}}",
            "\u{4e2d}\u{6587}",
        ] {
            for repr in Repr::ALL {
                let views = export(junk, repr).into_parts().0;
                for view in &views {
                    for format in Format::ALL {
                        assert!(!write(view, format).is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn a_partly_recovered_file_still_exports_the_functions_it_parsed() {
        // The second definition never closes; the first must survive it.
        let text = "int a(void) { return 1; }\nint b(void) { return 2;\n";
        let views = export(text, Repr::Cfg).into_parts().0;
        let names: Vec<&str> = views.iter().map(|v| v.name.as_str()).collect();
        assert!(names.contains(&"a"), "{names:?}");
    }

    #[test]
    fn a_multibyte_snippet_is_cut_without_panicking() {
        let text = "int f(void) { const char *s = \"\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\u{4e2d}\u{6587}\"; return 0; }";
        let views = export(text, Repr::Cfg).into_parts().0;
        assert_eq!(views.len(), 1);
        for node in &views[0].nodes {
            assert!(node.label.chars().count() < 128);
        }
    }

    #[test]
    fn repr_names_round_trip() {
        for repr in Repr::ALL {
            assert_eq!(Repr::parse(repr.name()), Some(repr));
        }
        assert_eq!(Repr::parse("ddg"), Some(Repr::Ddg));
        assert_eq!(Repr::parse("cdg"), Some(Repr::Cdg));
        assert_eq!(Repr::parse("pdg"), Some(Repr::Pdg));
        // `cpg14` is a code property graph, which this conventional CFG
        // exporter declines. It stays refused rather than faked.
        assert_eq!(Repr::parse("cpg14"), None, "not offered rather than faked");
        assert_eq!(Repr::parse("cpg"), None, "not offered rather than faked");
    }
}

#[cfg(test)]
mod ddg_tests {
    use super::*;
    use crate::syntax::graph_export::{write, Format};

    const SUM: &str = "int f(int n) { int s = 0; int dead = 7; for (int i = 0; i < n; i++) { s = s + i; } return s; }";

    #[test]
    fn a_ddg_export_separates_definitions_from_uses() {
        let views = export(SUM, Repr::Ddg).into_parts().0;
        assert_eq!(views.len(), 1);
        let view = &views[0];
        let roles: Vec<&str> = view
            .nodes
            .iter()
            .filter_map(|n| {
                n.attrs
                    .iter()
                    .find(|(k, _)| k == "role")
                    .map(|(_, v)| v.as_str())
            })
            .collect();
        assert!(roles.contains(&"definition"));
        assert!(roles.contains(&"use"));
        // Every edge runs definition -> use, never the other way.
        let def_count = roles.iter().filter(|r| **r == "definition").count() as u32;
        for edge in &view.edges {
            assert!(edge.src < def_count, "edge leaves a use: {edge:?}");
            assert!(edge.dst >= def_count, "edge enters a definition: {edge:?}");
        }
    }

    #[test]
    fn every_ddg_edge_names_its_variable() {
        // The thing pyjoern's DDG cannot tell you: which value an edge is for.
        let views = export(SUM, Repr::Ddg).into_parts().0;
        for edge in &views[0].edges {
            assert!(!edge.label.is_empty(), "unlabelled edge: {edge:?}");
            assert!(edge.attrs.iter().any(|(k, _)| k == "variable"));
        }
    }

    #[test]
    fn projected_memory_edges_are_first_class_ddg_nodes() {
        let source = "struct S{int x;};int f(int v){struct S s;s.x=v;return s.x;}";
        let views = export(source, Repr::Ddg).into_parts().0;
        let view = &views[0];
        assert!(view.nodes.iter().any(|node| {
            node.attrs
                .iter()
                .any(|(key, value)| key == "role" && value == "memory_definition")
        }));
        assert!(view.nodes.iter().any(|node| {
            node.attrs
                .iter()
                .any(|(key, value)| key == "role" && value == "memory_use")
        }));
        assert!(view.edges.iter().any(|edge| {
            edge.label == "s.x" && edge.attrs.iter().any(|(key, _)| key == "use_region")
        }));
    }

    #[test]
    fn a_dead_store_is_marked_on_the_node() {
        let views = export(SUM, Repr::Ddg).into_parts().0;
        let dead: Vec<&str> = views[0]
            .nodes
            .iter()
            .filter(|n| {
                n.attrs
                    .iter()
                    .any(|(k, v)| k == "dead_store" && v == "true")
            })
            .filter_map(|n| {
                n.attrs
                    .iter()
                    .find(|(k, _)| k == "variable")
                    .map(|(_, v)| v.as_str())
            })
            .collect();
        assert_eq!(dead, vec!["dead"], "{dead:?}");
    }

    #[test]
    fn the_ddg_serializes_in_every_format() {
        let views = export(SUM, Repr::Ddg).into_parts().0;
        for format in Format::ALL {
            let body = write(&views[0], format);
            assert!(body.contains('s') && !body.is_empty());
        }
    }
}

#[cfg(test)]
mod dependence_tests {
    use super::*;
    use crate::syntax::graph_export::{write, Format};

    /// A branch, a nested branch, a loop, and a value that flows through all
    /// three -- enough shape that every claim below is about something.
    const SHAPES: &str = r#"
int classify(int a, int b, int n)
{
    int total = 0;
    if (a > b) {
        if (n > 0) {
            total = a - b;
        }
    } else {
        total = b - a;
    }
    for (int i = 0; i < n; i++) {
        total = total + i;
    }
    return total;
}
"#;

    fn attr<'a>(node: &'a ExportNode, key: &str) -> Option<&'a str> {
        node.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_cdg_shares_the_cfg_node_set() {
        // Lining the two up node for node is what makes the export readable
        // beside `--repr cfg`, so it is a promise worth pinning.
        let cfg = export(SHAPES, Repr::Cfg).into_parts().0;
        let cdg = export(SHAPES, Repr::Cdg).into_parts().0;
        assert_eq!(cdg.len(), cfg.len());
        assert_eq!(cdg[0].nodes.len(), cfg[0].nodes.len());
        for (a, b) in cdg[0].nodes.iter().zip(cfg[0].nodes.iter()) {
            assert_eq!(a.id, b.id);
        }
    }

    #[test]
    fn every_cdg_edge_names_the_arm_that_decides_it() {
        let views = export(SHAPES, Repr::Cdg).into_parts().0;
        assert!(!views[0].edges.is_empty(), "no control dependence found");
        for edge in &views[0].edges {
            assert!(!edge.label.is_empty(), "unlabelled: {edge:?}");
            // The label is an edge kind, not a variable name.
            assert!(
                [
                    "true",
                    "false",
                    "case",
                    "default",
                    "fall",
                    "fall_through",
                    "jump"
                ]
                .contains(&edge.label.as_str()),
                "{edge:?}"
            );
        }
    }

    #[test]
    fn control_depth_grows_with_nesting() {
        let views = export(SHAPES, Repr::Cdg).into_parts().0;
        let depths: Vec<u32> = views[0]
            .nodes
            .iter()
            .filter_map(|node| attr(node, "depth"))
            .filter_map(|value| value.parse().ok())
            .collect();
        // The entry is unconditional; the doubly nested assignment is not.
        assert!(depths.contains(&0), "{depths:?}");
        assert!(
            depths.iter().any(|depth| *depth >= 2),
            "no doubly nested node found: {depths:?}"
        );
    }

    #[test]
    fn the_entry_depends_on_nothing() {
        let views = export(SHAPES, Repr::Cdg).into_parts().0;
        let entry = views[0]
            .nodes
            .iter()
            .find(|node| attr(node, "kind") == Some("entry"))
            .expect("an entry node");
        assert_eq!(attr(entry, "depth"), Some("0"));
        assert!(
            !views[0].edges.iter().any(|edge| edge.dst == entry.id),
            "the entry is control dependent on something"
        );
    }

    #[test]
    fn a_pdg_carries_both_kinds_of_edge_and_tags_each() {
        let views = export(SHAPES, Repr::Pdg).into_parts().0;
        let view = &views[0];
        let mut control = 0;
        let mut data = 0;
        for edge in &view.edges {
            match attr_edge(edge, "dependence") {
                Some("control") => control += 1,
                Some("data") => data += 1,
                other => panic!("untagged edge {edge:?}: {other:?}"),
            }
        }
        assert!(control > 0, "no control edges");
        assert!(data > 0, "no data edges");
        // Every data edge also names its variable.
        for edge in &view.edges {
            if attr_edge(edge, "dependence") == Some("data") {
                assert!(attr_edge(edge, "variable").is_some(), "{edge:?}");
            }
        }
    }

    fn attr_edge<'a>(edge: &'a ExportEdge, key: &str) -> Option<&'a str> {
        edge.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_pdg_edge_never_names_a_node_it_does_not_have() {
        let views = export(SHAPES, Repr::Pdg).into_parts().0;
        for view in &views {
            let ids: Vec<u32> = view.nodes.iter().map(|node| node.id).collect();
            for edge in &view.edges {
                assert!(ids.contains(&edge.src), "{edge:?}");
                assert!(ids.contains(&edge.dst), "{edge:?}");
            }
        }
    }

    #[test]
    fn every_dependence_graph_serializes_in_every_format() {
        for repr in [Repr::Ddg, Repr::Cdg, Repr::Pdg] {
            let views = export(SHAPES, repr).into_parts().0;
            assert!(!views.is_empty(), "{repr:?} produced nothing");
            for format in Format::ALL {
                assert!(!write(&views[0], format).is_empty(), "{repr:?}/{format:?}");
            }
        }
    }

    #[test]
    fn every_representation_is_total_on_junk() {
        for junk in [
            "",
            "\u{0}\u{1}",
            "int f(",
            "}}}",
            "while(1){}",
            "\u{4e2d}\u{6587}",
        ] {
            for repr in Repr::ALL {
                let views = export(junk, repr).into_parts().0;
                for view in &views {
                    for format in Format::ALL {
                        assert!(!write(view, format).is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn representation_and_coordinate_identity_have_distinct_stable_names() {
        let names = Repr::ALL.map(|repr| (repr.name(), repr.graph_kind()));
        assert_eq!(
            names,
            [
                ("cfg", "executable_cfg"),
                ("ast", "syntax_ast"),
                ("ddg", "data_dependence_graph"),
                ("cdg", "control_dependence_graph"),
                ("pdg", "program_dependence_graph"),
                ("ops", "typed_operations"),
            ]
        );
    }

    #[test]
    fn an_infinite_loop_still_exports_a_cdg() {
        // The shape that has no post-dominator tree without a virtual exit.
        let views = export("int f(void) { while (1) { } return 0; }", Repr::Cdg)
            .into_parts()
            .0;
        assert_eq!(views.len(), 1);
        assert!(!views[0].nodes.is_empty());
    }
}

#[cfg(test)]
mod semantic_attribute_tests {
    //! Pins for the attributes the 2026-09-16 consumer list asked for: the
    //! operator, the resolved C type, declarator and parameter structure,
    //! and line/column coordinates. Each test names the rule it pins.

    use super::*;

    /// The AST of the first function in `text`, as (nodes, children-by-id).
    fn ast(text: &str) -> GraphView {
        let mut views = export(text, Repr::Ast).into_parts().0;
        assert!(!views.is_empty(), "no function parsed from {text:?}");
        views.remove(0)
    }

    fn attr<'a>(node: &'a ExportNode, key: &str) -> Option<&'a str> {
        node.attrs
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    /// The first node whose tag is `tag` and whose source text is `source`.
    fn node<'a>(view: &'a GraphView, text: &str, tag: &str, source: &str) -> &'a ExportNode {
        view.nodes
            .iter()
            .find(|node| {
                attr(node, "tag") == Some(tag)
                    && attr(node, "span").is_some_and(|span| {
                        let (lo, hi) = span.split_once(':').expect("lo:hi");
                        text.get(lo.parse::<usize>().unwrap()..hi.parse::<usize>().unwrap())
                            == Some(source)
                    })
            })
            .unwrap_or_else(|| panic!("no {tag} node covering {source:?}"))
    }

    fn type_of(text: &str, tag: &str, source: &str) -> String {
        let view = ast(text);
        attr(node(&view, text, tag, source), "type")
            .unwrap_or_else(|| panic!("{tag} {source:?} has no type"))
            .to_owned()
    }

    fn operand_type_of(text: &str, source: &str) -> String {
        let view = ast(text);
        attr(node(&view, text, "binary_expr", source), "operand_type")
            .unwrap_or_else(|| panic!("binary_expr {source:?} has no operand_type"))
            .to_owned()
    }

    // ---- item 1: the operator ----------------------------------------------

    #[test]
    fn the_operator_is_a_token_and_survives_a_comment_between_operands() {
        let text = "int f(int a, int b) { return a /* x */ + b; }";
        let view = ast(text);
        assert_eq!(
            attr(node(&view, text, "binary_expr", "a /* x */ + b"), "op"),
            Some("+")
        );
    }

    #[test]
    fn every_operator_bearing_node_reports_its_operator() {
        let text = "int f(int a, int b) { a <<= 2; b = !a; a = b ? 1 : 2; a++; return a - b; }";
        let view = ast(text);
        assert_eq!(
            attr(node(&view, text, "assign_expr", "a <<= 2"), "op"),
            Some("<<=")
        );
        assert_eq!(attr(node(&view, text, "unary_expr", "!a"), "op"), Some("!"));
        assert_eq!(
            attr(node(&view, text, "cond_expr", "b ? 1 : 2"), "op"),
            Some("?:")
        );
        assert_eq!(
            attr(node(&view, text, "inc_dec_suffix", "++"), "op"),
            Some("++")
        );
        assert_eq!(
            attr(node(&view, text, "binary_expr", "a - b"), "op"),
            Some("-")
        );
        // A node without an operator has no `op` attribute at all.
        assert_eq!(attr(node(&view, text, "name_ref", "a"), "op"), None);
    }

    #[test]
    fn a_flat_chain_lists_every_operator_in_source_order() {
        let text = "int f(int a, int b, int c) { return a + b - c; }";
        let view = ast(text);
        let chain = node(&view, text, "binary_expr", "a + b - c");
        assert_eq!(attr(chain, "op"), Some("+"));
        assert_eq!(attr(chain, "ops"), Some("+,-"));
        let single = ast("int f(int a, int b) { return a + b; }");
        assert!(single.nodes.iter().all(|node| attr(node, "ops").is_none()));
    }

    // ---- item 2: the resolved C type ----------------------------------------

    #[test]
    fn integer_promotion_widens_narrow_operands_to_int() {
        // C17 6.3.1.1p2: unsigned short + int computes in int.
        let text = "int f(unsigned short s, int i) { return s + i; }";
        assert_eq!(type_of(text, "binary_expr", "s + i"), "int");
        assert_eq!(operand_type_of(text, "s + i"), "int");
        assert_eq!(type_of(text, "name_ref", "s"), "unsigned short");
        // Two narrow operands promote separately: unsigned char + unsigned
        // short is int, not unsigned anything.
        let text = "int f(unsigned char c, unsigned short s) { return c + s; }";
        assert_eq!(type_of(text, "binary_expr", "c + s"), "int");
        let text = "int f(unsigned short s) { return -s; }";
        assert_eq!(type_of(text, "unary_expr", "-s"), "int");
    }

    #[test]
    fn same_rank_mixed_signedness_goes_unsigned() {
        // C17 6.3.1.8p1: int with unsigned int converts to unsigned int.
        let text = "int f(int i, unsigned int u) { return i + u; }";
        assert_eq!(type_of(text, "binary_expr", "i + u"), "unsigned int");
        assert_eq!(operand_type_of(text, "i + u"), "unsigned int");
    }

    #[test]
    fn a_wider_signed_type_absorbs_a_narrower_unsigned_one() {
        // C17 6.3.1.8p1 under LP64: long can represent every unsigned int.
        let text = "long f(long l, unsigned int u) { return l * u; }";
        assert_eq!(type_of(text, "binary_expr", "l * u"), "long");
        // ...but not every unsigned long: long long with unsigned long is
        // unsigned long long, the unsigned counterpart of the signed type.
        let text = "long f(long long l, unsigned long u) { return l * u; }";
        assert_eq!(type_of(text, "binary_expr", "l * u"), "unsigned long long");
    }

    #[test]
    fn a_comparison_is_int_and_its_operands_convert() {
        // The consumer's own example: int < size_t compares in unsigned long
        // and yields int.
        let text = "typedef unsigned long size_t;\nint f(int i, size_t n) { return i < n; }";
        assert_eq!(type_of(text, "binary_expr", "i < n"), "int");
        assert_eq!(operand_type_of(text, "i < n"), "unsigned long");
        assert_eq!(type_of(text, "name_ref", "n"), "unsigned long");
    }

    #[test]
    fn a_shift_takes_the_promoted_left_operand() {
        let text = "int f(unsigned short s, unsigned long n) { return s << n; }";
        assert_eq!(type_of(text, "binary_expr", "s << n"), "int");
        assert_eq!(operand_type_of(text, "s << n"), "int");
        let text = "int f(unsigned int u, int i) { return u >> i; }";
        assert_eq!(type_of(text, "binary_expr", "u >> i"), "unsigned int");
    }

    #[test]
    fn a_cast_has_the_named_type() {
        let text = "int f(unsigned short s, char *p) { return (unsigned char)s + (int)*p; }";
        assert_eq!(
            type_of(text, "cast_expr", "(unsigned char)s"),
            "unsigned char"
        );
        assert_eq!(type_of(text, "cast_expr", "(int)*p"), "int");
        assert_eq!(type_of(text, "unary_expr", "*p"), "char");
        let text = "typedef unsigned int u32;\nint f(int i) { return (const u32 *)&i; }";
        assert_eq!(
            type_of(text, "cast_expr", "(const u32 *)&i"),
            "const unsigned int *"
        );
        assert_eq!(type_of(text, "unary_expr", "&i"), "int *");
    }

    #[test]
    fn a_conditional_converts_both_arms() {
        let text = "int f(int n, unsigned long m, short s) { return n < 0 ? -n : m; }";
        assert_eq!(
            type_of(text, "cond_expr", "n < 0 ? -n : m"),
            "unsigned long"
        );
        let text = "int f(int n, short s) { return n ? s : 'c'; }";
        assert_eq!(type_of(text, "cond_expr", "n ? s : 'c'"), "int");
        let text = "int f(int n, char *p) { return n ? p : 0; }";
        assert_eq!(type_of(text, "cond_expr", "n ? p : 0"), "char *");
    }

    #[test]
    fn literal_suffixes_and_ranges_decide_the_literal_type() {
        // C17 6.4.4.1p5.
        let text = "int f(void) { return 1 + 1u + 1UL + 0xffffffff + 2147483648 + 1.5f + 'c'; }";
        assert_eq!(type_of(text, "literal", "1"), "int");
        assert_eq!(type_of(text, "literal", "1u"), "unsigned int");
        assert_eq!(type_of(text, "literal", "1UL"), "unsigned long");
        // Hex without a suffix that does not fit int becomes unsigned int...
        assert_eq!(type_of(text, "literal", "0xffffffff"), "unsigned int");
        // ...where the same value in decimal becomes long.
        assert_eq!(type_of(text, "literal", "2147483648"), "long");
        assert_eq!(type_of(text, "literal", "1.5f"), "float");
        assert_eq!(type_of(text, "literal", "'c'"), "int");
    }

    #[test]
    fn unknown_is_the_answer_and_never_a_guess() {
        // An undeclared name, a call with no visible declaration, a name from
        // a header that was not included, and an enum under arithmetic.
        let text = "int f(uint32_t x, enum e v) { return g(x) + y + x + v; }";
        assert_eq!(type_of(text, "name_ref", "y"), "unknown");
        assert_eq!(type_of(text, "postfix_expr", "g(x)"), "unknown");
        assert_eq!(type_of(text, "name_ref", "x"), "unknown");
        assert_eq!(type_of(text, "name_ref", "v"), "enum e");
        assert_eq!(type_of(text, "binary_expr", "g(x) + y + x + v"), "unknown");
        // A pointer to an unresolved type is still known to be a pointer.
        let text = "int f(uint32_t *p) { return p + 1; }";
        assert_eq!(type_of(text, "name_ref", "p"), "unknown *");
        assert_eq!(type_of(text, "binary_expr", "p + 1"), "unknown *");
        // sizeof has type size_t only when that typedef is visible.
        assert_eq!(
            type_of(
                "int f(int x) { return sizeof x; }",
                "unary_expr",
                "sizeof x"
            ),
            "unknown"
        );
        assert_eq!(
            type_of(
                "typedef unsigned long size_t; int f(int x) { return sizeof x; }",
                "unary_expr",
                "sizeof x"
            ),
            "unsigned long"
        );
    }

    #[test]
    fn pointers_arrays_members_and_assignments_are_typed() {
        let text = concat!(
            "struct point { int x; unsigned short y; };\n",
            "int f(unsigned char *dst, struct point *pt, int arr[], unsigned short s) {\n",
            "  unsigned int table[16]; const char *name = \"ab\\n\";\n",
            "  dst[1] += s; pt->y = pt->x + arr[2]; s <<= 3; table[0] = dst - dst;\n",
            "  return name[0] + (dst + 1)[0]; }",
        );
        assert_eq!(type_of(text, "name_ref", "table"), "unsigned int[16]");
        assert_eq!(type_of(text, "postfix_expr", "table[0]"), "unsigned int");
        assert_eq!(type_of(text, "name_ref", "arr"), "int *");
        assert_eq!(type_of(text, "postfix_expr", "pt->y"), "unsigned short");
        assert_eq!(type_of(text, "postfix_expr", "arr[2]"), "int");
        assert_eq!(type_of(text, "postfix_expr", "name[0]"), "char");
        assert_eq!(type_of(text, "binary_expr", "dst + 1"), "unsigned char *");
        assert_eq!(type_of(text, "literal", "\"ab\\n\""), "char[4]");
        // ptrdiff_t is not visible, so a pointer difference is unknown.
        assert_eq!(type_of(text, "binary_expr", "dst - dst"), "unknown");
        let view = ast(text);
        let compound = node(&view, text, "assign_expr", "dst[1] += s");
        assert_eq!(attr(compound, "type"), Some("unsigned char"));
        assert_eq!(attr(compound, "operand_type"), Some("int"));
        let shift = node(&view, text, "assign_expr", "s <<= 3");
        assert_eq!(attr(shift, "type"), Some("unsigned short"));
        assert_eq!(attr(shift, "operand_type"), Some("int"));
        let plain = node(&view, text, "assign_expr", "pt->y = pt->x + arr[2]");
        assert_eq!(attr(plain, "operand_type"), Some("unsigned short"));
    }

    #[test]
    fn a_logical_operator_converts_nothing() {
        let text = "int f(int a, unsigned long b) { return a && b; }";
        let view = ast(text);
        let and = node(&view, text, "binary_expr", "a && b");
        assert_eq!(attr(and, "type"), Some("int"));
        assert_eq!(attr(and, "operand_type"), None);
    }

    #[test]
    fn a_chain_reports_one_operand_type_per_operator() {
        let text = "long f(long lg, unsigned short s, unsigned int u) { return lg + s - u; }";
        let view = ast(text);
        let chain = node(&view, text, "binary_expr", "lg + s - u");
        assert_eq!(attr(chain, "ops"), Some("+,-"));
        assert_eq!(attr(chain, "operand_type"), Some("long"));
        assert_eq!(attr(chain, "operand_types"), Some("long,long"));
        assert_eq!(attr(chain, "type"), Some("long"));
    }

    #[test]
    fn the_tree_only_builder_carries_no_semantic_attributes() {
        let text = "int f(int a) { return a + 1; }";
        let tree = crate::csource::parse::parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let function = &tree.functions(text)[0];
        let view = ast_view(&function.name, &tree, function.node, &spans, text);
        assert!(view.nodes.iter().all(|node| attr(node, "type").is_none()));
        assert!(view.nodes.iter().any(|node| attr(node, "op") == Some("+")));
    }

    // ---- item 4: array declarators ------------------------------------------

    #[test]
    fn an_array_declarator_exports_its_element_type_and_count() {
        let text =
            "int f(int n) { unsigned int table[16]; char vla[n]; int rows[2][3]; return 0; }";
        let view = ast(text);
        let table = node(&view, text, "declarator", "table[16]");
        assert_eq!(attr(table, "name"), Some("table"));
        assert_eq!(attr(table, "type"), Some("unsigned int[16]"));
        assert_eq!(attr(table, "element_type"), Some("unsigned int"));
        assert_eq!(attr(table, "count"), Some("16"));
        assert_eq!(attr(table, "array_bound"), Some("constant"));
        let vla = node(&view, text, "declarator", "vla[n]");
        assert_eq!(attr(vla, "element_type"), Some("char"));
        assert_eq!(attr(vla, "array_bound"), Some("runtime"));
        assert_eq!(attr(vla, "count"), None);
        let rows = node(&view, text, "declarator", "rows[2][3]");
        assert_eq!(attr(rows, "type"), Some("int[2][3]"));
        assert_eq!(attr(rows, "element_type"), Some("int[3]"));
        assert_eq!(attr(rows, "count"), Some("2"));
        // A scalar declarator has a type and no array fields.
        let text = "int f(void) { long x = 1; return x; }";
        let view = ast(text);
        let scalar = node(&view, text, "declarator", "x");
        assert_eq!(attr(scalar, "type"), Some("long"));
        assert_eq!(attr(scalar, "element_type"), None);
    }

    #[test]
    fn an_array_of_an_unresolved_type_keeps_its_shape() {
        let text = "int f(void) { uint32_t table[16]; return 0; }";
        let view = ast(text);
        let table = node(&view, text, "declarator", "table[16]");
        assert_eq!(attr(table, "type"), Some("unknown[16]"));
        assert_eq!(attr(table, "element_type"), Some("unknown"));
        assert_eq!(attr(table, "count"), Some("16"));
    }

    // ---- item 5: parameters as fields ---------------------------------------

    #[test]
    fn a_parameter_exports_type_pointer_depth_and_name() {
        let text = "typedef unsigned long size_t;\nint f(unsigned char *dst, size_t dst_len, const char **names, int arr[], void (*cb)(int), int) { return 0; }";
        let view = ast(text);
        let dst = node(&view, text, "param_decl", "unsigned char *dst");
        assert_eq!(attr(dst, "type"), Some("unsigned char *"));
        assert_eq!(attr(dst, "pointer_depth"), Some("1"));
        assert_eq!(attr(dst, "name"), Some("dst"));
        let len = node(&view, text, "param_decl", "size_t dst_len");
        assert_eq!(attr(len, "type"), Some("unsigned long"));
        assert_eq!(attr(len, "pointer_depth"), Some("0"));
        let names = node(&view, text, "param_decl", "const char **names");
        assert_eq!(attr(names, "type"), Some("const char **"));
        assert_eq!(attr(names, "pointer_depth"), Some("2"));
        // An array parameter is adjusted to a pointer (C17 6.7.6.3p7).
        let arr = node(&view, text, "param_decl", "int arr[]");
        assert_eq!(attr(arr, "type"), Some("int *"));
        assert_eq!(attr(arr, "pointer_depth"), Some("1"));
        // An unnamed parameter has a type and no name.
        let unnamed = node(&view, text, "param_decl", "int");
        assert_eq!(attr(unnamed, "type"), Some("int"));
        assert_eq!(attr(unnamed, "name"), None);
        let void = ast("int g(void) { return 0; }");
        let void_param = void
            .nodes
            .iter()
            .find(|node| attr(node, "tag") == Some("param_decl"))
            .expect("a void parameter");
        assert_eq!(attr(void_param, "type"), Some("void"));
    }

    // ---- item 6: line and column ---------------------------------------------

    #[test]
    fn every_node_carries_a_one_based_line_and_byte_column() {
        let text = "/* \u{2014} */\nint f(int a)\n{\n\treturn a + 1;\n}\n";
        let view = ast(text);
        for node in &view.nodes {
            assert!(attr(node, "line").is_some(), "{node:?}");
            assert!(attr(node, "column").is_some(), "{node:?}");
        }
        let root = &view.nodes[0];
        assert_eq!(attr(root, "line"), Some("2"));
        assert_eq!(attr(root, "column"), Some("1"));
        let sum = node(&view, text, "binary_expr", "a + 1");
        assert_eq!(attr(sum, "line"), Some("4"));
        // The tab is one byte, so `return` starts at column 2 and `a` at 9.
        assert_eq!(attr(sum, "column"), Some("9"));
        // The line and column agree with the byte span, not a char index:
        // the em dash on line 1 is three bytes and shifts nothing after it.
        let (lo, _) = attr(sum, "span").unwrap().split_once(':').unwrap();
        let lo: usize = lo.parse().unwrap();
        let line = text.as_bytes()[..lo]
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            + 1;
        let col = lo
            - text.as_bytes()[..lo]
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(0, |i| i + 1)
            + 1;
        assert_eq!((line, col), (4, 9));
    }

    #[test]
    fn cfg_and_dependence_nodes_carry_line_and_column_too() {
        let text = "int f(int a)\n{\n  if (a) return 1;\n  return 0;\n}\n";
        for repr in [Repr::Cfg, Repr::Cdg, Repr::Pdg, Repr::Ddg] {
            let views = export(text, repr).into_parts().0;
            for node in &views[0].nodes {
                assert!(attr(node, "line").is_some(), "{}: {node:?}", repr.name());
                assert!(attr(node, "column").is_some(), "{}: {node:?}", repr.name());
            }
        }
        let cfg = export(text, Repr::Cfg).into_parts().0.remove(0);
        let cond = cfg
            .nodes
            .iter()
            .find(|node| attr(node, "kind") == Some("cond"))
            .expect("a cond node");
        assert_eq!(attr(cond, "line"), Some("3"));
        assert_eq!(attr(cond, "column"), Some("7"));
    }
}

#[cfg(test)]
mod expression_internal_tests {
    //! Item 11 of the 2026-09-16 list: which CFG nodes exist only because a
    //! short-circuit or conditional operator was expanded.

    use super::*;

    fn attr<'a>(node: &'a ExportNode, key: &str) -> Option<&'a str> {
        node.attrs
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
    }

    /// `(kind, source, expr_internal)` for every node of the first CFG.
    fn census(text: &str, repr: Repr) -> Vec<(String, String, String)> {
        let views = export(text, repr).into_parts().0;
        views[0]
            .nodes
            .iter()
            .map(|node| {
                let (lo, hi) = attr(node, "span").unwrap().split_once(':').unwrap();
                let source = text
                    .get(lo.parse::<usize>().unwrap()..hi.parse::<usize>().unwrap())
                    .unwrap_or("")
                    .to_owned();
                (
                    attr(node, "kind").unwrap().to_owned(),
                    source,
                    attr(node, "expr_internal")
                        .expect("every CFG node carries expr_internal")
                        .to_owned(),
                )
            })
            .collect()
    }

    #[test]
    fn the_operands_of_a_logical_and_in_an_if_are_internal_and_the_test_is_not() {
        let text = "int f(int idx, unsigned u) { if (idx <= 16 && u != 0) return 1; return 0; }";
        let nodes = census(text, Repr::Cfg);
        let internal: Vec<&str> = nodes
            .iter()
            .filter(|(_, _, flag)| flag == "true")
            .map(|(_, source, _)| source.as_str())
            .collect();
        assert_eq!(internal, ["idx <= 16", "u != 0"], "{nodes:#?}");
        let whole = nodes
            .iter()
            .find(|(_, source, _)| source == "idx <= 16 && u != 0")
            .expect("the statement-level test node");
        assert_eq!(whole.0, "cond");
        assert_eq!(whole.2, "false");
        // Entry, exit and the returns are statement-level.
        assert!(nodes
            .iter()
            .filter(|(kind, _, _)| kind != "cond" && kind != "stmt")
            .all(|(_, _, flag)| flag == "false"));
    }

    #[test]
    fn the_arms_of_a_conditional_initializer_are_internal_and_the_declaration_is_not() {
        let text = "int f(int n) { int idx = n < 0 ? -n : n; return idx; }";
        let nodes = census(text, Repr::Cfg);
        let internal: Vec<(&str, &str)> = nodes
            .iter()
            .filter(|(_, _, flag)| flag == "true")
            .map(|(kind, source, _)| (kind.as_str(), source.as_str()))
            .collect();
        // The consumer's "four nodes": the test, the two arms, and the
        // operator's own join, which sits ahead of the declaration's node.
        assert_eq!(
            internal,
            [
                ("cond", "n < 0"),
                ("stmt", "-n"),
                ("stmt", "n"),
                ("stmt", "n < 0 ? -n : n")
            ],
            "{nodes:#?}"
        );
        let declaration = nodes
            .iter()
            .find(|(_, source, _)| source == "int idx = n < 0 ? -n : n;")
            .expect("the declaration's own node");
        assert_eq!(declaration.2, "false");
    }

    #[test]
    fn a_nested_operator_join_inside_a_larger_expression_is_internal() {
        // `a && b` here is not the statement's terminal, so its join node is
        // one more node inside the statement.
        let text = "int f(int a, int b) { int x = (a && b) + 1; return x; }";
        let nodes = census(text, Repr::Cfg);
        let internal: Vec<&str> = nodes
            .iter()
            .filter(|(_, _, flag)| flag == "true")
            .map(|(_, source, _)| source.as_str())
            .collect();
        assert_eq!(internal, ["a", "b", "a && b"], "{nodes:#?}");
    }

    #[test]
    fn a_function_without_short_circuits_has_no_internal_node() {
        let text = "int f(int a) { if (a) return 1; return a + 1; }";
        for repr in [Repr::Cfg, Repr::Cdg, Repr::Pdg] {
            let nodes = census(text, repr);
            assert!(!nodes.is_empty());
            assert!(
                nodes.iter().all(|(_, _, flag)| flag == "false"),
                "{}: {nodes:#?}",
                repr.name()
            );
        }
    }

    #[test]
    fn the_flag_is_the_same_on_every_view_over_the_cfg_node_set() {
        let text = "int f(int idx, unsigned u) { if (idx <= 16 && u != 0) return 1; return 0; }";
        let cfg = census(text, Repr::Cfg);
        assert_eq!(census(text, Repr::Cdg), cfg);
        assert_eq!(
            census(text, Repr::Pdg)
                .iter()
                .map(|(_, source, flag)| (source.clone(), flag.clone()))
                .collect::<Vec<_>>(),
            cfg.iter()
                .map(|(_, source, flag)| (source.clone(), flag.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_function_cfg_lists_the_same_nodes_the_export_flags() {
        let text = "int f(int a, int b, int c) { return a && b && c; }";
        let unit = AnalysisUnit::new(text);
        let function = &unit.functions()[0];
        // `a`, `b`, `c` and the chain's join; the `return` node is not.
        assert_eq!(
            function.expression_internal.len(),
            4,
            "{:?}",
            function.expression_internal
        );
        assert!(function.expression_internal.windows(2).all(|w| w[0] < w[1]));
        let view = &export_unit(&unit, Repr::Cfg)[0];
        for node in &view.nodes {
            let listed = function.expression_internal.contains(&NodeId::new(node.id));
            assert_eq!(
                attr(node, "expr_internal"),
                Some(if listed { "true" } else { "false" })
            );
        }
    }
}

#[cfg(test)]
mod ordering_contract_tests {
    //! Item 14 of the 2026-09-16 list: the order the AST export is written
    //! in is a contract (docs/reference/source-python.md, "Ordering").

    use super::*;

    const SOURCE: &str = "/* \u{2014} */\nint f(int a, unsigned b, int c)\n{\n  int t[4];\n  if (a /* x */ + b - c > 0 && t[1]) { t[0] = a ? b : c; }\n  return (a << 2) | c;\n}\n";

    fn span_of(node: &ExportNode) -> (u32, u32) {
        let span = node
            .attrs
            .iter()
            .find(|(key, _)| key == "span")
            .map(|(_, value)| value.as_str())
            .expect("span");
        let (lo, hi) = span.split_once(':').unwrap();
        (lo.parse().unwrap(), hi.parse().unwrap())
    }

    #[test]
    fn ast_ids_are_preorder_and_ascend_with_span() {
        let view = export(SOURCE, Repr::Ast).into_parts().0.remove(0);
        for (index, node) in view.nodes.iter().enumerate() {
            assert_eq!(node.id, index as u32, "ids are dense and in list order");
        }
        for pair in view.nodes.windows(2) {
            assert!(
                span_of(&pair[0]).0 <= span_of(&pair[1]).0,
                "{:?} precedes {:?}",
                pair[0],
                pair[1]
            );
        }
        // Preorder: every edge points forward, and a parent's span covers
        // its children's.
        for edge in &view.edges {
            assert!(edge.src < edge.dst, "{edge:?}");
            let parent = span_of(&view.nodes[edge.src as usize]);
            let child = span_of(&view.nodes[edge.dst as usize]);
            assert!(parent.0 <= child.0 && child.1 <= parent.1, "{edge:?}");
        }
    }

    #[test]
    fn ast_edges_are_grouped_by_parent_and_children_are_in_source_order() {
        let view = export(SOURCE, Repr::Ast).into_parts().0.remove(0);
        // Grouped by ascending parent: the parent id never decreases, and
        // once a parent's group ends it does not resume.
        let mut last_parent: Option<u32> = None;
        let mut closed: Vec<u32> = Vec::new();
        for edge in &view.edges {
            if last_parent != Some(edge.src) {
                assert!(!closed.contains(&edge.src), "parent {} resumed", edge.src);
                if let Some(previous) = last_parent {
                    assert!(previous < edge.src, "parent order regressed at {edge:?}");
                    closed.push(previous);
                }
                last_parent = Some(edge.src);
            }
        }
        // Within a parent, children ascend by span and do not overlap.
        for pair in view.edges.windows(2) {
            if pair[0].src != pair[1].src {
                continue;
            }
            let left = span_of(&view.nodes[pair[0].dst as usize]);
            let right = span_of(&view.nodes[pair[1].dst as usize]);
            assert!(
                left.1 <= right.0,
                "{:?} then {:?}: {left:?} vs {right:?}",
                pair[0],
                pair[1]
            );
        }
        // And that order is the operand order: the first binary chain reads
        // `a`, `b`, `c` left to right.
        let chain = view
            .nodes
            .iter()
            .find(|node| node.attrs.iter().any(|(k, v)| k == "ops" && v == "+,-"))
            .expect("the a + b - c chain");
        let operands: Vec<&str> = view
            .edges
            .iter()
            .filter(|edge| edge.src == chain.id)
            .map(|edge| {
                let (lo, hi) = span_of(&view.nodes[edge.dst as usize]);
                &SOURCE[lo as usize..hi as usize]
            })
            .collect();
        assert_eq!(operands, ["a", "b", "c"]);
    }

    #[test]
    fn cfg_nodes_start_with_entry_and_exit_and_edges_are_grouped_by_source() {
        let view = export(SOURCE, Repr::Cfg).into_parts().0.remove(0);
        let kind = |node: &ExportNode| {
            node.attrs
                .iter()
                .find(|(k, _)| k == "kind")
                .map(|(_, v)| v.clone())
                .unwrap()
        };
        assert_eq!(kind(&view.nodes[0]), "entry");
        assert_eq!(kind(&view.nodes[1]), "exit");
        for pair in view.edges.windows(2) {
            assert!(
                pair[0].src <= pair[1].src,
                "{:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn the_same_input_serializes_to_the_same_bytes() {
        use crate::syntax::graph_export::{write, Format};
        for repr in Repr::ALL {
            let first = export(SOURCE, repr).into_parts().0;
            let second = export(SOURCE, repr).into_parts().0;
            for (a, b) in first.iter().zip(&second) {
                assert_eq!(write(a, Format::Json), write(b, Format::Json));
            }
        }
    }
}

#[cfg(test)]
mod loop_metadata_tests {
    //! Item 12 of `docs/improvement-list-2026-09-16.md`: the loop header
    //! carries what a bounded unroller needs, on the AST loop statement and
    //! on the CFG `loop_header` node alike, and refuses to guess.

    use super::*;
    use crate::syntax::graph_export::ExportNode;

    fn attr<'a>(node: &'a ExportNode, key: &str) -> Option<&'a str> {
        node.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The loop attributes of every AST loop statement and every CFG
    /// `loop_header` of the first function, in source order, as
    /// `(bound_kind, induction, step, bound_value, bound_expr)`.
    fn loops(text: &str, repr: Repr) -> Vec<Vec<(String, String)>> {
        let unit = AnalysisUnit::new(text);
        let view = &export_unit(&unit, repr)[0];
        view.nodes
            .iter()
            .filter(|node| match repr {
                Repr::Ast => matches!(
                    attr(node, "tag"),
                    Some("for_stmt" | "while_stmt" | "do_while_stmt")
                ),
                _ => attr(node, "kind") == Some("loop_header"),
            })
            .map(|node| {
                node.attrs
                    .iter()
                    .filter(|(k, _)| {
                        matches!(
                            k.as_str(),
                            "loop_kind"
                                | "bound_kind"
                                | "bound_expr"
                                | "induction"
                                | "step"
                                | "init_value"
                                | "bound_value"
                        )
                    })
                    .cloned()
                    .collect()
            })
            .collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn a_literal_bound_for_loop_is_constant_sixteen_step_plus_one() {
        let text = "int f(void){ int s = 0, i; for (i = 0; i < 16; i++) s += i; return s; }";
        let expected = pairs(&[
            ("loop_kind", "for"),
            ("bound_kind", "constant"),
            ("bound_expr", "i < 16"),
            ("induction", "i"),
            ("step", "+1"),
            ("init_value", "0"),
            ("bound_value", "16"),
        ]);
        assert_eq!(loops(text, Repr::Ast), vec![expected.clone()]);
        assert_eq!(loops(text, Repr::Cfg), vec![expected]);
    }

    #[test]
    fn a_parameter_bound_the_body_leaves_alone_is_parameter() {
        let text = "int f(int n){ int s = 0, i; for (i = 0; i <= n; i++) s += i; return s; }";
        let expected = pairs(&[
            ("loop_kind", "for"),
            ("bound_kind", "parameter"),
            ("bound_expr", "i <= n"),
            ("induction", "i"),
            ("step", "+1"),
            ("init_value", "0"),
        ]);
        assert_eq!(loops(text, Repr::Ast), vec![expected.clone()]);
        assert_eq!(loops(text, Repr::Cfg), vec![expected]);
    }

    #[test]
    fn a_local_bound_is_not_a_parameter() {
        // Same shape, but `n` is a local: the resolver says so, and the
        // classification is `runtime`, not a guess from the name.
        let text = "int f(void){ int n = 3, s = 0, i; for (i = 0; i <= n; i++) s += i; return s; }";
        let [only] = &loops(text, Repr::Ast)[..] else {
            panic!("one loop")
        };
        assert!(
            only.contains(&("bound_kind".to_owned(), "runtime".to_owned())),
            "{only:?}"
        );
    }

    #[test]
    fn a_while_loop_names_its_induction_and_step_but_is_runtime() {
        let text = "unsigned f(unsigned u){ while (u > 0) { u--; } return u; }";
        let expected = pairs(&[
            ("loop_kind", "while"),
            ("bound_kind", "runtime"),
            ("bound_expr", "u > 0"),
            ("induction", "u"),
            ("step", "-1"),
        ]);
        assert_eq!(loops(text, Repr::Ast), vec![expected.clone()]);
        assert_eq!(loops(text, Repr::Cfg), vec![expected]);
    }

    #[test]
    fn a_body_that_assigns_the_bound_variable_is_runtime() {
        let text = "int f(int n){ int s = 0, i; for (i = 0; i <= n; i++) { s += i; n = n - 1; } return s; }";
        let expected = pairs(&[
            ("loop_kind", "for"),
            ("bound_kind", "runtime"),
            ("bound_expr", "i <= n"),
            ("induction", "i"),
            ("step", "+1"),
            ("init_value", "0"),
        ]);
        assert_eq!(loops(text, Repr::Ast), vec![expected.clone()]);
        assert_eq!(loops(text, Repr::Cfg), vec![expected]);
    }

    #[test]
    fn the_cdg_and_pdg_carry_the_same_loop_attributes_as_the_cfg() {
        let text = "int f(int n){ int s = 0, i; for (i = 0; i < 16; i++) s += i; \
                    do { n--; } while (n > 0); for (;;) { break; } return s; }";
        let cfg = loops(text, Repr::Cfg);
        assert_eq!(cfg.len(), 3, "{cfg:?}");
        assert_eq!(loops(text, Repr::Cdg), cfg);
        assert_eq!(loops(text, Repr::Pdg), cfg);
        assert_eq!(loops(text, Repr::Ast), cfg);
        assert_eq!(
            cfg[2],
            pairs(&[("loop_kind", "for"), ("bound_kind", "none")])
        );
    }

    #[test]
    fn the_tree_only_builder_classifies_without_parameters() {
        // No resolver: a literal bound is still constant, a name bound is
        // runtime rather than a guess.
        let text = "int f(int n){ int i; for (i = 0; i < 16; i++) {} for (i = 0; i < n; i++) {} return i; }";
        let tree = crate::csource::parse::parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let function = &tree.functions(text)[0];
        let view = ast_view(&function.name, &tree, function.node, &spans, text);
        let kinds: Vec<&str> = view
            .nodes
            .iter()
            .filter(|node| attr(node, "tag") == Some("for_stmt"))
            .map(|node| attr(node, "bound_kind").unwrap())
            .collect();
        assert_eq!(kinds, vec!["constant", "runtime"]);
    }
}
