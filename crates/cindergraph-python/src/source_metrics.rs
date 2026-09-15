//! Python bindings for the C source metrics.
//!
//! [`cindergraph::csource::metrics`] measures a single piece of C: how big it is, how
//! branchy, how deeply nested, what it calls, and Halstead's token figures.
//! This exposes that to Python, and the boundary is the one
//! [`crate::python_bindings::source_cfg`] and
//! [`crate::python_bindings::metrics`] already draw --- **Rust hands back plain
//! dicts, lists, ints, floats and strings, and nothing Python-shaped is ever
//! passed in**. The ergonomic wrappers live in `python/glaurung/source.py`,
//! where they cost nothing to change and cannot make the extension unimportable
//! in an environment missing some library.
//!
//! Plain data rather than a class hierarchy is also what the consumers want.
//! Three of the four use cases this exists for --- a JSON report, a feature
//! matrix for a corpus, a threshold gate in CI --- want a dict they can
//! serialize or a row they can stack; only interactive exploration wants
//! attributes, and that one is cheap to build in Python on top of a dict.
//!
//! Two invariants carry over from [`cindergraph::csource::metrics`] and are
//! load-bearing here.
//!
//! * **Nothing raises on account of the input.** Parsing is total
//!   (`REQ-SYN-2`), so any byte sequence yields a report; a file the parser
//!   only partly recovered yields the functions it did recover alongside the
//!   diagnostics explaining the rest. A caller cannot tell "this file has no
//!   functions" from "this file failed" if the second one throws.
//! * **Determinism.** Every collection here comes from a `Vec` or a
//!   `BTreeMap`, so no hash iteration order reaches Python and two runs over
//!   the same text produce byte-identical output.

use std::collections::BTreeMap;

use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use cindergraph::csource::metrics::{self, FunctionMetrics, SourceReport};
use cindergraph::csource::parse::parse;
use cindergraph::csource::semantic::{AnalysisOptions, AnalysisUnit, InputDialect};
use cindergraph::dataflow::ExternalCallPolicy;
use cindergraph::syntax::cfg::Cfg;
use cindergraph::syntax::diag::Diagnostics;

fn input_dialect(name: Option<&str>) -> PyResult<InputDialect> {
    match name.unwrap_or("ordinary") {
        "ordinary" => Ok(InputDialect::Ordinary),
        "preprocessed" => Ok(InputDialect::Preprocessed),
        "decompiled" => Ok(InputDialect::Decompiled),
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "unknown dialect {other:?}; expected ordinary, preprocessed, or decompiled"
        ))),
    }
}

fn external_call_policy(name: Option<&str>) -> PyResult<ExternalCallPolicy> {
    match name.unwrap_or("unknown") {
        "unknown" => Ok(ExternalCallPolicy::Unknown),
        "taint_return" => Ok(ExternalCallPolicy::TaintReturn),
        "assume_pure_no_flow" => Ok(ExternalCallPolicy::AssumePureNoFlow),
        other => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "unknown external-call policy {other:?}; expected unknown, taint_return, or assume_pure_no_flow"
        ))),
    }
}

fn diagnostics_list<'py>(
    py: Python<'py>,
    diagnostics: &Diagnostics,
    text: &str,
) -> PyResult<Bound<'py, PyList>> {
    let reported = PyList::empty(py);
    for diagnostic in diagnostics.iter() {
        let entry = PyDict::new(py);
        entry.set_item(
            "severity",
            format!("{:?}", diagnostic.severity).to_lowercase(),
        )?;
        entry.set_item("message", diagnostic.message.clone())?;
        entry.set_item("start", diagnostic.span.lo)?;
        entry.set_item("end", diagnostic.span.hi)?;
        entry.set_item("text", diagnostic.render(text))?;
        reported.append(entry)?;
    }
    Ok(reported)
}

/// Build the `{"lines", "tokens", "bytes", "functions", "diagnostics"}` dict.
fn report_dict<'py>(
    py: Python<'py>,
    report: &SourceReport,
    diagnostics: &Diagnostics,
    text: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);

    let lines = PyDict::new(py);
    lines.set_item("lines", report.lines.lines)?;
    lines.set_item("code_lines", report.lines.code_lines)?;
    lines.set_item("blank_lines", report.lines.blank_lines)?;
    lines.set_item("other_lines", report.lines.other_lines)?;
    out.set_item("lines", lines)?;
    out.set_item("tokens", report.tokens)?;
    out.set_item("bytes", report.bytes)?;

    let functions = PyList::empty(py);
    for function in &report.functions {
        functions.append(function_dict(py, function)?)?;
    }
    out.set_item("functions", functions)?;

    out.set_item("diagnostics", diagnostics_list(py, diagnostics, text)?)?;
    Ok(out)
}

/// One function's measurement as a dict.
fn function_dict<'py>(py: Python<'py>, f: &FunctionMetrics) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("name", f.name.clone())?;
    out.set_item("start", f.span.lo)?;
    out.set_item("end", f.span.hi)?;
    out.set_item("has_body", f.has_body)?;
    out.set_item("parameters", f.parameters)?;
    out.set_item("short_circuits", f.short_circuits)?;
    out.set_item("unreachable_statements", f.unreachable_statements)?;

    let size = PyDict::new(py);
    size.set_item("first_line", f.size.first_line)?;
    size.set_item("last_line", f.size.last_line)?;
    size.set_item("lines", f.size.lines)?;
    size.set_item("code_lines", f.size.code_lines)?;
    size.set_item("tokens", f.size.tokens)?;
    size.set_item("bytes", f.size.bytes)?;
    out.set_item("size", size)?;

    let graph = PyDict::new(py);
    graph.set_item("nodes", f.graph.nodes)?;
    graph.set_item("edges", f.graph.edges)?;
    graph.set_item("reachable_nodes", f.graph.reachable_nodes)?;
    graph.set_item("unreachable_nodes", f.graph.unreachable_nodes)?;
    graph.set_item("dead_end_nodes", f.graph.dead_end_nodes)?;
    graph.set_item("cyclomatic", f.graph.cyclomatic)?;
    graph.set_item("decision_points", f.graph.decision_points)?;
    graph.set_item("back_edges", f.graph.back_edges)?;
    graph.set_item("loops", f.graph.loops)?;
    let node_kinds = PyDict::new(py);
    for (kind, count) in &f.graph.node_kinds {
        node_kinds.set_item(kind.name(), count)?;
    }
    graph.set_item("node_kinds", node_kinds)?;
    let edge_kinds = PyDict::new(py);
    for (kind, count) in &f.graph.edge_kinds {
        edge_kinds.set_item(kind.name(), count)?;
    }
    graph.set_item("edge_kinds", edge_kinds)?;
    out.set_item("graph", graph)?;

    let shape = PyDict::new(py);
    shape.set_item("max_nesting", f.shape.max_nesting)?;
    shape.set_item("max_loop_depth", f.shape.max_loop_depth)?;
    shape.set_item("cognitive", f.shape.cognitive)?;
    shape.set_item("calls", f.shape.calls)?;
    shape.set_item("callees", f.shape.callees.clone())?;
    shape.set_item("statements", f.shape.statements)?;
    shape.set_item("ast_nodes", f.shape.nodes)?;
    shape.set_item("truncated", f.shape.truncated)?;
    let tags = PyDict::new(py);
    for (tag, count) in &f.shape.tag_counts {
        tags.set_item(*tag, count)?;
    }
    shape.set_item("tag_counts", tags)?;
    out.set_item("shape", shape)?;

    let halstead = PyDict::new(py);
    halstead.set_item("distinct_operators", f.halstead.distinct_operators)?;
    halstead.set_item("distinct_operands", f.halstead.distinct_operands)?;
    halstead.set_item("total_operators", f.halstead.total_operators)?;
    halstead.set_item("total_operands", f.halstead.total_operands)?;
    halstead.set_item("vocabulary", f.halstead.vocabulary())?;
    halstead.set_item("length", f.halstead.length())?;
    halstead.set_item("volume", f.halstead.volume())?;
    halstead.set_item("difficulty", f.halstead.difficulty())?;
    halstead.set_item("effort", f.halstead.effort())?;
    out.set_item("halstead", halstead)?;

    Ok(out)
}

/// Measure one translation unit of C.
///
/// Returns the whole report as nested plain data. Total on every input: a file
/// that is not C at all yields zero functions and the diagnostics saying so,
/// never an exception.
#[pyfunction]
#[pyo3(name = "analyze")]
pub fn analyze_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyDict>> {
    // Pure Rust with no Python object access, and a whole-tree caller runs this
    // over thousands of files, so it has no business holding the GIL.
    let (report, diagnostics) = py.detach(|| metrics::analyze(text).into_parts());
    report_dict(py, &report, &diagnostics, text)
}

/// The functions a file defines, without measuring any of them.
///
/// One parse and no graph construction, for the common case of listing what is
/// in a file before deciding what to measure.
#[pyfunction]
#[pyo3(name = "functions")]
pub fn functions_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyList>> {
    let found = py.detach(|| {
        let tree = parse(text).into_parts().0;
        let index = metrics::LineIndex::new(text);
        tree.functions(text)
            .into_iter()
            .map(|f| {
                (
                    f.name,
                    f.span.lo,
                    f.span.hi,
                    index.line(f.span.lo),
                    index.line(f.span.hi.saturating_sub(1).max(f.span.lo)),
                    f.body.is_some(),
                )
            })
            .collect::<Vec<_>>()
    });

    let out = PyList::empty(py);
    for (name, start, end, first_line, last_line, has_body) in found {
        let entry = PyDict::new(py);
        entry.set_item("name", name)?;
        entry.set_item("start", start)?;
        entry.set_item("end", end)?;
        entry.set_item("first_line", first_line)?;
        entry.set_item("last_line", last_line)?;
        entry.set_item("has_body", has_body)?;
        out.append(entry)?;
    }
    Ok(out)
}

/// Serialize one general CFG.
fn cfg_dict<'py>(py: Python<'py>, cfg: &Cfg) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    let nodes = PyList::empty(py);
    for (index, node) in cfg.nodes().iter().enumerate() {
        let entry = PyDict::new(py);
        entry.set_item("id", index as u32)?;
        entry.set_item("kind", node.kind().name())?;
        entry.set_item("start", node.span().lo)?;
        entry.set_item("end", node.span().hi)?;
        nodes.append(entry)?;
    }
    out.set_item("nodes", nodes)?;

    let edges = PyList::empty(py);
    for edge in cfg.edges() {
        let entry = PyDict::new(py);
        entry.set_item("src", edge.src.raw())?;
        entry.set_item("dst", edge.dst.raw())?;
        entry.set_item("kind", edge.kind.name())?;
        entry.set_item("back", edge.is_back)?;
        edges.append(entry)?;
    }
    out.set_item("edges", edges)?;
    let dispatches = PyList::empty(py);
    for info in cfg.indirect_dispatches() {
        let entry = PyDict::new(py);
        entry.set_item("node", info.node.raw())?;
        entry.set_item(
            "targets",
            cfg.successors(info.node)
                .map(|target| target.raw())
                .collect::<Vec<_>>(),
        )?;
        entry.set_item("precision", info.precision.name())?;
        entry.set_item("may_be_invalid", info.may_be_invalid)?;
        entry.set_item(
            "reasons",
            info.reasons
                .iter()
                .map(|reason| reason.name())
                .collect::<Vec<_>>(),
        )?;
        dispatches.append(entry)?;
    }
    out.set_item("indirect_dispatches", dispatches)?;
    out.set_item("entry", cfg.entry().raw())?;
    out.set_item("exit", cfg.exit().raw())?;
    Ok(out)
}

/// Every function's **general** control-flow graph: the graph a person would
/// draw.
///
/// This is not
/// [`crate::python_bindings::source_cfg::parity_cfgs_py`], and the difference
/// matters. That one reproduces another tool's artifacts so a similarity metric
/// can be compared against it --- coalesced expression chains, a function-end
/// node deleted when it stayed a singleton, entry and exit as flags. This one
/// has real successors, real join points, real loop back edges, and typed
/// nodes and edges. Use this to look at control flow; use that one only to
/// reproduce a score.
///
/// A list rather than a name-keyed dict: two definitions in one file can carry
/// the same name after recovery, and a dict would silently drop one.
#[pyfunction]
#[pyo3(name = "control_flow_graphs")]
pub fn control_flow_graphs_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyList>> {
    let unit = py.detach(|| AnalysisUnit::new(text));
    control_flow_graphs_dict(py, &unit)
}

fn control_flow_graphs_dict<'py>(
    py: Python<'py>,
    unit: &AnalysisUnit,
) -> PyResult<Bound<'py, PyList>> {
    use cindergraph::csource::semantic::ANALYSIS_REVISION;

    let out = PyList::empty(py);
    for (index, graph) in unit.functions().iter().enumerate() {
        let entry = PyDict::new(py);
        entry.set_item("name", graph.name.clone())?;
        entry.set_item("source_id", unit.source_id().name())?;
        entry.set_item("input_dialect", unit.options().dialect.name())?;
        entry.set_item("external_call_policy", unit.options().external_calls.name())?;
        entry.set_item("function_id", index as u32)?;
        entry.set_item("analysis_revision", ANALYSIS_REVISION)?;
        entry.set_item("graph_kind", "executable_cfg")?;
        entry.set_item("start", graph.span.lo)?;
        entry.set_item("end", graph.span.hi)?;
        entry.set_item("short_circuits", graph.short_circuits)?;
        entry.set_item("cfg", cfg_dict(py, &graph.cfg)?)?;
        out.append(entry)?;
    }
    Ok(out)
}

/// The names of the numbers [`features_py`] returns, in the order it returns
/// them.
///
/// A fixed, ordered, documented vector is the point: a caller stacking rows for
/// a hundred thousand functions needs the column meaning to be stable across
/// releases and identical across files.
pub const FEATURE_NAMES: &[&str] = &[
    // size
    "lines",
    "code_lines",
    "tokens",
    "bytes",
    "parameters",
    // graph
    "cfg_nodes",
    "cfg_edges",
    "reachable_nodes",
    "dead_end_nodes",
    "cyclomatic",
    "decision_points",
    "back_edges",
    "loops",
    // shape
    "max_nesting",
    "max_loop_depth",
    "cognitive",
    "calls",
    "statements",
    "ast_nodes",
    "short_circuits",
    "unreachable_statements",
    // halstead
    "halstead_distinct_operators",
    "halstead_distinct_operands",
    "halstead_total_operators",
    "halstead_total_operands",
    "halstead_vocabulary",
    "halstead_length",
    "halstead_volume",
    "halstead_difficulty",
    "halstead_effort",
    // node-kind census, in `NodeKind` discriminant order
    "nodes_entry",
    "nodes_exit",
    "nodes_stmt",
    "nodes_cond",
    "nodes_loop_header",
    "nodes_switch",
    "nodes_indirect_dispatch",
    "nodes_case",
    "nodes_label",
    "nodes_goto",
    "nodes_break",
    "nodes_continue",
    "nodes_return",
    "nodes_diverge",
];

/// The node-kind census columns, in the order [`FEATURE_NAMES`] lists them.
const KIND_COLUMNS: &[&str] = &[
    "entry",
    "exit",
    "stmt",
    "cond",
    "loop_header",
    "switch",
    "indirect_dispatch",
    "case",
    "label",
    "goto",
    "break",
    "continue",
    "return",
    "diverge",
];

/// One function's feature row.
fn feature_row(f: &FunctionMetrics) -> Vec<f64> {
    let mut row = vec![
        f64::from(f.size.lines),
        f64::from(f.size.code_lines),
        f64::from(f.size.tokens),
        f64::from(f.size.bytes),
        f64::from(f.parameters),
        f64::from(f.graph.nodes),
        f64::from(f.graph.edges),
        f64::from(f.graph.reachable_nodes),
        f64::from(f.graph.dead_end_nodes),
        f64::from(f.graph.cyclomatic),
        f64::from(f.graph.decision_points),
        f64::from(f.graph.back_edges),
        f64::from(f.graph.loops),
        f64::from(f.shape.max_nesting),
        f64::from(f.shape.max_loop_depth),
        f64::from(f.shape.cognitive),
        f64::from(f.shape.calls),
        f64::from(f.shape.statements),
        f64::from(f.shape.nodes),
        f64::from(f.short_circuits),
        f64::from(f.unreachable_statements),
        f64::from(f.halstead.distinct_operators),
        f64::from(f.halstead.distinct_operands),
        f64::from(f.halstead.total_operators),
        f64::from(f.halstead.total_operands),
        f64::from(f.halstead.vocabulary()),
        f64::from(f.halstead.length()),
        f.halstead.volume(),
        f.halstead.difficulty(),
        f.halstead.effort(),
    ];
    for column in KIND_COLUMNS {
        let count = f
            .graph
            .node_kinds
            .iter()
            .find(|(kind, _)| kind.name() == *column)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        row.push(f64::from(count));
    }
    row
}

/// The feature-vector column names.
#[pyfunction]
#[pyo3(name = "feature_names")]
pub fn feature_names_py() -> Vec<&'static str> {
    FEATURE_NAMES.to_vec()
}

/// One fixed-width numeric row per function, for a corpus-scale consumer.
///
/// Returns `[(name, [f64; len(feature_names())]), ...]` in source order --- the
/// same data [`analyze_py`] reports, flattened.
///
/// **It exists for the stable column vector, not for speed.** Measured over
/// `tests/decompiler_fixtures/src` (196 files, 900 functions, 0.78 MB) with a
/// `maturin develop --release` build, best of five:
///
/// ```text
/// analyze   43.8 ms
/// features  41.3 ms     0.94x
/// ```
///
/// Parsing and graph construction dominate; building the nested dicts is 6% of
/// the run, not most of it. What this buys is a row whose meaning is fixed by
/// [`FEATURE_NAMES`] and does not move when the report's dict schema gains a
/// key.
#[pyfunction]
#[pyo3(name = "features")]
pub fn features_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyList>> {
    let rows = py.detach(|| {
        let report = metrics::analyze(text).into_parts().0;
        report
            .functions
            .iter()
            .map(|f| (f.name.clone(), feature_row(f)))
            .collect::<Vec<_>>()
    });
    let out = PyList::empty(py);
    for (name, row) in rows {
        out.append((name, row))?;
    }
    Ok(out)
}

/// Apply the one text normalization pass a dialect is allowed to go through.
///
/// `dialect` is `"preprocessed"` for a gcc-preprocessed translation unit, or
/// `"decompiled"` for a decompiler backend's output. Anything else is a
/// `ValueError` rather than a silent pass-through: normalizing the wrong side
/// reshapes what the parser sees, and a typo that quietly did nothing would be
/// invisible in every number downstream.
///
/// Exposed as its own function rather than as a flag on [`analyze_py`] because
/// normalization **rewrites the text**, so every byte offset a report carries
/// refers to the normalized string and not to the caller's original. Making
/// that a separate step means the caller holds the string its offsets describe.
#[pyfunction]
#[pyo3(name = "normalize")]
pub fn normalize_py(py: Python<'_>, text: &str, dialect: &str) -> PyResult<String> {
    use cindergraph::csource::normalize::Dialect;
    let dialect = match dialect {
        "preprocessed" => Dialect::Preprocessed(text),
        "decompiled" => Dialect::Decompiled(text),
        other => {
            return Err(pyo3::exceptions::PyValueError::new_err(format!(
                "unknown dialect {other:?}; expected \"preprocessed\" or \"decompiled\""
            )))
        }
    };
    Ok(py.detach(|| dialect.normalize()))
}

/// Serialize each function's graph in one of four wire formats.
///
/// The replacement for `joern-export --repr {ast,cfg} --format {dot,graphml,
/// ...}`, minus the three representations that need a data-dependence
/// analysis this front end does not do. `cdg`, `ddg` and `pdg` raise rather
/// than returning a control-flow graph under another name.
///
/// `repr` is `"cfg"` (the general control-flow graph, never the Joern-parity
/// one) or `"ast"`. `format` is `"dot"`, `"graphml"`, `"json"` or `"mermaid"`.
///
/// Returns one `(function name, serialized graph)` pair per function, in
/// source order. A list rather than a dict, because two definitions in one
/// file can carry the same name after recovery.
#[pyfunction]
#[pyo3(name = "export_graphs")]
pub fn export_graphs_py<'py>(
    py: Python<'py>,
    text: &str,
    repr: &str,
    format: &str,
) -> PyResult<Bound<'py, PyList>> {
    let unit = py.detach(|| AnalysisUnit::new(text));
    export_graphs_dict(py, &unit, repr, format)
}

fn export_graphs_dict<'py>(
    py: Python<'py>,
    unit: &AnalysisUnit,
    repr: &str,
    format: &str,
) -> PyResult<Bound<'py, PyList>> {
    use cindergraph::csource::export::{export_unit, Repr};
    use cindergraph::syntax::graph_export::{write, Format};

    let repr_value = Repr::parse(repr).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "unknown repr {repr:?}; expected one of {:?}",
            Repr::ALL.map(|r| r.name())
        ))
    })?;
    let format_value = Format::parse(format).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "unknown format {format:?}; expected one of {:?}",
            Format::ALL.map(|f| f.name())
        ))
    })?;

    let rendered = py.detach(|| {
        export_unit(unit, repr_value)
            .iter()
            .map(|view| (view.name.clone(), write(view, format_value)))
            .collect::<Vec<(String, String)>>()
    });

    let out = PyList::empty(py);
    for (name, body) in rendered {
        out.append((name, body))?;
    }
    Ok(out)
}

/// Read-only, Rust-backed exported graph for Python-scale traversal.
#[pyclass(name = "NativeGraph", module = "cindergraph._native.source", frozen)]
struct PyNativeGraph {
    view: cindergraph::syntax::graph_export::GraphView,
    adjacency: CompactAdjacency,
    source_id: String,
    function_id: u32,
    representation: String,
    graph_kind: String,
    input_dialect: String,
    external_call_policy: String,
}

/// Immutable forward and reverse adjacency over dense exported node IDs.
///
/// Exported graphs assign IDs in `0..node_count`. Two offset arrays and two
/// contiguous neighbor arrays avoid the tree node and per-node `Vec`
/// allocations of the original Python-facing graph while retaining edge
/// insertion order and parallel-edge multiplicity.
struct CompactAdjacency {
    successor_offsets: Vec<usize>,
    successors: Vec<u32>,
    predecessor_offsets: Vec<usize>,
    predecessors: Vec<u32>,
}

impl CompactAdjacency {
    fn new(view: &cindergraph::syntax::graph_export::GraphView) -> Self {
        let node_count = view.nodes.len();
        debug_assert!(
            view.nodes
                .iter()
                .enumerate()
                .all(|(index, node)| node.id as usize == index),
            "exported graph node IDs must be dense"
        );

        let mut successor_counts = vec![0; node_count];
        let mut predecessor_counts = vec![0; node_count];
        for edge in &view.edges {
            if let (Some(out), Some(in_)) = (
                successor_counts.get_mut(edge.src as usize),
                predecessor_counts.get_mut(edge.dst as usize),
            ) {
                *out += 1;
                *in_ += 1;
            }
        }

        let successor_offsets = Self::offsets(&successor_counts);
        let predecessor_offsets = Self::offsets(&predecessor_counts);
        let mut successors = vec![0; *successor_offsets.last().unwrap_or(&0)];
        let mut predecessors = vec![0; *predecessor_offsets.last().unwrap_or(&0)];
        let mut successor_cursors = successor_offsets[..node_count].to_vec();
        let mut predecessor_cursors = predecessor_offsets[..node_count].to_vec();
        for edge in &view.edges {
            let (Some(successor_cursor), Some(predecessor_cursor)) = (
                successor_cursors.get_mut(edge.src as usize),
                predecessor_cursors.get_mut(edge.dst as usize),
            ) else {
                continue;
            };
            successors[*successor_cursor] = edge.dst;
            *successor_cursor += 1;
            predecessors[*predecessor_cursor] = edge.src;
            *predecessor_cursor += 1;
        }
        Self {
            successor_offsets,
            successors,
            predecessor_offsets,
            predecessors,
        }
    }

    fn offsets(counts: &[usize]) -> Vec<usize> {
        let mut offsets = Vec::with_capacity(counts.len() + 1);
        offsets.push(0);
        for count in counts {
            offsets.push(offsets.last().copied().unwrap_or(0) + count);
        }
        offsets
    }

    fn contains(&self, node: u32) -> bool {
        (node as usize) + 1 < self.successor_offsets.len()
    }

    fn neighbors(&self, node: u32, reverse: bool) -> Option<&[u32]> {
        let index = node as usize;
        let (offsets, neighbors) = if reverse {
            (&self.predecessor_offsets, &self.predecessors)
        } else {
            (&self.successor_offsets, &self.successors)
        };
        Some(&neighbors[*offsets.get(index)?..*offsets.get(index + 1)?])
    }
}

impl PyNativeGraph {
    fn new(
        view: cindergraph::syntax::graph_export::GraphView,
        unit: &AnalysisUnit,
        function_id: u32,
        representation: &str,
        graph_kind: &str,
    ) -> Self {
        let adjacency = CompactAdjacency::new(&view);
        Self {
            view,
            adjacency,
            source_id: unit.source_id().name(),
            function_id,
            representation: representation.to_owned(),
            graph_kind: graph_kind.to_owned(),
            input_dialect: unit.options().dialect.name().to_owned(),
            external_call_policy: unit.options().external_calls.name().to_owned(),
        }
    }

    fn require_node(&self, id: u32) -> PyResult<()> {
        if self.adjacency.contains(id) {
            Ok(())
        } else {
            Err(pyo3::exceptions::PyKeyError::new_err(id))
        }
    }

    fn closure(&self, start: u32, reverse: bool) -> PyResult<Vec<u32>> {
        self.require_node(start)?;
        let mut seen = vec![false; self.view.nodes.len()];
        let mut pending = vec![start];
        while let Some(node) = pending.pop() {
            for &next in self
                .adjacency
                .neighbors(node, reverse)
                .unwrap_or_default()
                .iter()
                .rev()
            {
                if next != start && !seen[next as usize] {
                    seen[next as usize] = true;
                    pending.push(next);
                }
            }
        }
        Ok(seen
            .into_iter()
            .enumerate()
            .filter_map(|(node, reached)| reached.then_some(node as u32))
            .collect())
    }
}

#[pymethods]
impl PyNativeGraph {
    #[getter]
    fn name(&self) -> &str {
        &self.view.name
    }

    #[getter]
    fn source_id(&self) -> &str {
        &self.source_id
    }

    #[getter]
    fn function_id(&self) -> u32 {
        self.function_id
    }

    #[getter]
    fn representation(&self) -> &str {
        &self.representation
    }

    #[getter]
    fn graph_kind(&self) -> &str {
        &self.graph_kind
    }

    #[getter]
    fn input_dialect(&self) -> &str {
        &self.input_dialect
    }

    #[getter]
    fn external_call_policy(&self) -> &str {
        &self.external_call_policy
    }

    #[getter]
    fn analysis_revision(&self) -> u32 {
        cindergraph::csource::semantic::ANALYSIS_REVISION
    }

    #[getter]
    fn directed(&self) -> bool {
        true
    }

    #[getter]
    fn node_count(&self) -> usize {
        self.view.nodes.len()
    }

    #[getter]
    fn edge_count(&self) -> usize {
        self.view.edges.len()
    }

    fn nodes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        for node in &self.view.nodes {
            let item = PyDict::new(py);
            item.set_item("id", node.id)?;
            item.set_item("label", &node.label)?;
            item.set_item(
                "attributes",
                node.attrs.iter().cloned().collect::<BTreeMap<_, _>>(),
            )?;
            out.append(item)?;
        }
        Ok(out)
    }

    fn edges<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        let out = PyList::empty(py);
        for edge in &self.view.edges {
            let item = PyDict::new(py);
            item.set_item("source", edge.src)?;
            item.set_item("target", edge.dst)?;
            item.set_item("label", &edge.label)?;
            item.set_item(
                "attributes",
                edge.attrs.iter().cloned().collect::<BTreeMap<_, _>>(),
            )?;
            out.append(item)?;
        }
        Ok(out)
    }

    fn edge_list(&self) -> Vec<(u32, u32)> {
        self.view
            .edges
            .iter()
            .map(|edge| (edge.src, edge.dst))
            .collect()
    }

    fn successors(&self, node: u32) -> PyResult<Vec<u32>> {
        self.require_node(node)?;
        Ok(self
            .adjacency
            .neighbors(node, false)
            .unwrap_or_default()
            .to_vec())
    }

    fn predecessors(&self, node: u32) -> PyResult<Vec<u32>> {
        self.require_node(node)?;
        Ok(self
            .adjacency
            .neighbors(node, true)
            .unwrap_or_default()
            .to_vec())
    }

    fn out_degree(&self, node: u32) -> PyResult<usize> {
        Ok(self.successors(node)?.len())
    }

    fn in_degree(&self, node: u32) -> PyResult<usize> {
        Ok(self.predecessors(node)?.len())
    }

    fn descendants(&self, node: u32) -> PyResult<Vec<u32>> {
        self.closure(node, false)
    }

    fn ancestors(&self, node: u32) -> PyResult<Vec<u32>> {
        self.closure(node, true)
    }
}

fn native_graphs_unit(
    unit: &AnalysisUnit,
    repr: &str,
) -> PyResult<(
    Vec<cindergraph::syntax::graph_export::GraphView>,
    &'static str,
    &'static str,
)> {
    use cindergraph::csource::export::{export_unit, Repr};

    let repr = Repr::parse(repr).ok_or_else(|| {
        pyo3::exceptions::PyValueError::new_err(format!(
            "unknown repr {repr:?}; expected one of {:?}",
            Repr::ALL.map(|value| value.name())
        ))
    })?;
    Ok((export_unit(unit, repr), repr.name(), repr.graph_kind()))
}

fn native_graphs_list<'py>(
    py: Python<'py>,
    unit: &AnalysisUnit,
    repr: &str,
) -> PyResult<Bound<'py, PyList>> {
    let (views, representation, graph_kind) = py.detach(|| native_graphs_unit(unit, repr))?;
    let out = PyList::empty(py);
    for (function_id, view) in views.into_iter().enumerate() {
        out.append(Py::new(
            py,
            PyNativeGraph::new(view, unit, function_id as u32, representation, graph_kind),
        )?)?;
    }
    Ok(out)
}

/// Return Rust-backed graph views without serializing or importing NetworkX.
#[pyfunction]
#[pyo3(name = "native_graphs", signature = (text, *, repr="cfg"))]
fn native_graphs_py<'py>(py: Python<'py>, text: &str, repr: &str) -> PyResult<Bound<'py, PyList>> {
    let unit = py.detach(|| AnalysisUnit::new(text));
    native_graphs_list(py, &unit, repr)
}

/// The `repr` and `format` names [`export_graphs_py`] accepts, as two lists.
///
/// A CLI builds its choice lists from these rather than repeating them, so a
/// format added in Rust cannot be missing from the command that offers it.
#[pyfunction]
#[pyo3(name = "export_choices")]
pub fn export_choices_py(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
    use cindergraph::csource::export::Repr;
    use cindergraph::syntax::graph_export::Format;

    let out = PyDict::new(py);
    out.set_item("repr", Repr::ALL.map(|r| r.name()).to_vec())?;
    out.set_item("format", Format::ALL.map(|f| f.name()).to_vec())?;
    Ok(out)
}

/// Reaching definitions, uses and dead stores, per function.
///
/// The structured form of `export_graphs(repr="ddg")`, for a caller who wants
/// the answer rather than a rendering of it. Each function is a dict with
/// `name`, `definitions`, `uses`, `edges`, `unresolved_uses` and
/// `dead_stores`; the two defect lists index into the first two.
#[pyfunction]
#[pyo3(name = "data_flow")]
pub fn data_flow_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyList>> {
    let unit = py.detach(|| {
        let unit = AnalysisUnit::new(text);
        let _ = unit.dataflows();
        unit
    });
    data_flow_dict(py, &unit)
}

fn data_flow_dict<'py>(py: Python<'py>, unit: &AnalysisUnit) -> PyResult<Bound<'py, PyList>> {
    let flows = unit.dataflows();
    let out = PyList::empty(py);
    for flow in flows {
        let entry = PyDict::new(py);
        entry.set_item("name", flow.name.clone())?;
        entry.set_item("source_id", flow.source_id.name())?;
        entry.set_item("input_dialect", unit.options().dialect.name())?;
        entry.set_item("external_call_policy", unit.options().external_calls.name())?;
        entry.set_item("function_id", flow.function_id.0)?;
        entry.set_item("analysis_revision", flow.analysis_revision)?;

        let definitions = PyList::empty(py);
        for definition in &flow.definitions {
            let item = PyDict::new(py);
            item.set_item("name", definition.name.clone())?;
            item.set_item("binding", definition.binding.0)?;
            item.set_item("kind", definition.kind.name())?;
            item.set_item("cfg_node", definition.node)?;
            item.set_item(
                "declared_type",
                definition.declared.as_ref().map(|ty| ty.render()),
            )?;
            item.set_item("start", definition.span.lo)?;
            item.set_item("end", definition.span.hi)?;
            definitions.append(item)?;
        }
        entry.set_item("definitions", definitions)?;
        let memory_regions = PyList::empty(py);
        for region in &flow.memory_regions {
            use cindergraph::csource::dataflow::MemoryRegionKind;
            let item = PyDict::new(py);
            item.set_item("id", region.id.0)?;
            item.set_item("kind", region.kind.name())?;
            match &region.kind {
                MemoryRegionKind::Binding { binding } => {
                    item.set_item("binding", binding.0)?;
                    item.set_item("parameter", py.None())?;
                    item.set_item("base", py.None())?;
                    item.set_item("member", py.None())?;
                }
                MemoryRegionKind::ParameterPointee { parameter, binding } => {
                    item.set_item("binding", binding.0)?;
                    item.set_item("parameter", parameter)?;
                    item.set_item("base", py.None())?;
                    item.set_item("member", py.None())?;
                    item.set_item("overlapping_members", py.None())?;
                }
                MemoryRegionKind::Field {
                    base,
                    member,
                    overlapping_members,
                } => {
                    item.set_item("binding", py.None())?;
                    item.set_item("parameter", py.None())?;
                    item.set_item("base", base.0)?;
                    item.set_item("member", member)?;
                    item.set_item("overlapping_members", overlapping_members)?;
                }
                MemoryRegionKind::Elements { base } => {
                    item.set_item("binding", py.None())?;
                    item.set_item("parameter", py.None())?;
                    item.set_item("base", base.0)?;
                    item.set_item("member", py.None())?;
                    item.set_item("overlapping_members", py.None())?;
                }
            }
            if matches!(region.kind, MemoryRegionKind::Binding { .. }) {
                item.set_item("overlapping_members", py.None())?;
            }
            memory_regions.append(item)?;
        }
        entry.set_item("memory_regions", memory_regions)?;

        let memory_overlaps = PyList::empty(py);
        for overlap in &flow.memory_overlaps {
            let item = PyDict::new(py);
            item.set_item("left", overlap.left.0)?;
            item.set_item("right", overlap.right.0)?;
            item.set_item("kind", overlap.kind.name())?;
            memory_overlaps.append(item)?;
        }
        entry.set_item("memory_overlaps", memory_overlaps)?;

        let call_memory_arguments = PyList::empty(py);
        for argument in &flow.call_memory_arguments {
            let item = PyDict::new(py);
            item.set_item("cfg_node", argument.node)?;
            item.set_item("argument", argument.argument)?;
            item.set_item(
                "targets",
                argument
                    .targets
                    .iter()
                    .map(|binding| binding.0)
                    .collect::<Vec<_>>(),
            )?;
            item.set_item("parameter_origins", &argument.parameter_origins)?;
            item.set_item("complete", argument.complete)?;
            item.set_item("start", argument.call_span.lo)?;
            item.set_item("end", argument.call_span.hi)?;
            call_memory_arguments.append(item)?;
        }
        entry.set_item("call_memory_arguments", call_memory_arguments)?;

        let memory_accesses = PyList::empty(py);
        for access in &flow.memory_accesses {
            let item = PyDict::new(py);
            item.set_item("region", access.region.0)?;
            item.set_item("kind", access.kind.name())?;
            item.set_item("precision", access.precision.name())?;
            item.set_item("cfg_node", access.node)?;
            item.set_item("start", access.span.lo)?;
            item.set_item("end", access.span.hi)?;
            item.set_item("effect_at", access.effect_at)?;
            memory_accesses.append(item)?;
        }
        entry.set_item("memory_accesses", memory_accesses)?;

        let memory_definitions = PyList::empty(py);
        for definition in &flow.memory_definitions {
            let item = PyDict::new(py);
            item.set_item("region", definition.region.0)?;
            item.set_item("kind", definition.kind.name())?;
            item.set_item("precision", definition.precision.name())?;
            item.set_item("cfg_node", definition.node)?;
            item.set_item("start", definition.span.lo)?;
            item.set_item("end", definition.span.hi)?;
            item.set_item("effect_at", definition.effect_at)?;
            memory_definitions.append(item)?;
        }
        entry.set_item("memory_definitions", memory_definitions)?;

        let memory_uses = PyList::empty(py);
        for use_ in &flow.memory_uses {
            let item = PyDict::new(py);
            item.set_item("region", use_.region.0)?;
            item.set_item("precision", use_.precision.name())?;
            item.set_item("cfg_node", use_.node)?;
            item.set_item("start", use_.span.lo)?;
            item.set_item("end", use_.span.hi)?;
            memory_uses.append(item)?;
        }
        entry.set_item("memory_uses", memory_uses)?;

        let memory_edges = PyList::empty(py);
        for edge in &flow.memory_edges {
            let item = PyDict::new(py);
            item.set_item("definition", edge.definition)?;
            item.set_item("use", edge.use_)?;
            item.set_item("definition_region", edge.definition_region.0)?;
            item.set_item("use_region", edge.use_region.0)?;
            item.set_item("overlap", edge.overlap.map(|kind| kind.name()))?;
            memory_edges.append(item)?;
        }
        entry.set_item("memory_edges", memory_edges)?;
        entry.set_item("effects_complete", flow.effects_complete)?;
        entry.set_item("memory_complete", flow.memory_complete)?;
        entry.set_item("vla_complete", flow.vla_complete)?;
        entry.set_item("control_targets_complete", flow.control_targets_complete)?;
        entry.set_item("recovery_free", flow.recovery_free)?;
        let issues = PyList::empty(py);
        for issue in &flow.semantic_issues {
            let item = PyDict::new(py);
            item.set_item("kind", issue.kind.name())?;
            item.set_item(
                "dimensions",
                issue
                    .kind
                    .dimensions()
                    .iter()
                    .map(|dimension| dimension.name())
                    .collect::<Vec<_>>(),
            )?;
            item.set_item("start", issue.span.map(|span| span.lo))?;
            item.set_item("end", issue.span.map(|span| span.hi))?;
            issues.append(item)?;
        }
        entry.set_item("semantic_issues", issues)?;

        let uses = PyList::empty(py);
        for use_ in &flow.uses {
            let item = PyDict::new(py);
            item.set_item("name", use_.name.clone())?;
            item.set_item("binding", use_.binding.0)?;
            item.set_item("cfg_node", use_.node)?;
            item.set_item("start", use_.span.lo)?;
            item.set_item("end", use_.span.hi)?;
            uses.append(item)?;
        }
        entry.set_item("uses", uses)?;

        let edges = PyList::empty(py);
        for edge in &flow.edges {
            let item = PyDict::new(py);
            item.set_item("definition", edge.def)?;
            item.set_item("use", edge.use_)?;
            item.set_item("variable", edge.name.clone())?;
            edges.append(item)?;
        }
        entry.set_item("edges", edges)?;
        entry.set_item("unresolved_uses", flow.unresolved_uses.clone())?;
        entry.set_item("dead_stores", flow.dead_stores.clone())?;

        let bindings = PyList::empty(py);
        for (index, name) in flow.names.iter().enumerate() {
            let item = PyDict::new(py);
            item.set_item(
                "is_unresolved",
                flow.unresolved_bindings
                    .iter()
                    .any(|b| b.0 as usize == index),
            )?;
            item.set_item("name", name.clone())?;
            let ty = flow.types.get(index);
            item.set_item("type", ty.filter(|t| !t.is_empty()).map(|t| t.render()))?;
            item.set_item(
                "specifiers",
                ty.map(|t| t.specifiers.clone()).unwrap_or_default(),
            )?;
            item.set_item("pointer_depth", ty.map(|t| t.pointer_depth).unwrap_or(0))?;
            item.set_item("array_rank", ty.map(|t| t.array_rank).unwrap_or(0))?;
            item.set_item("is_const", ty.map(|t| t.is_const).unwrap_or(false))?;
            item.set_item("is_volatile", ty.map(|t| t.is_volatile).unwrap_or(false))?;
            bindings.append(item)?;
        }
        entry.set_item("bindings", bindings)?;
        entry.set_item(
            "type_conflicts",
            flow.type_conflicts()
                .iter()
                .map(|b| b.0)
                .collect::<Vec<u32>>(),
        )?;
        entry.set_item(
            "unused_bindings",
            flow.unused_bindings()
                .iter()
                .map(|b| b.0)
                .collect::<Vec<u32>>(),
        )?;
        out.append(entry)?;
    }
    Ok(out)
}

/// Control dependence and post-dominance, per function.
///
/// Which branch decides each statement, and how deeply each is nested in
/// decisions. The depth is computed on the graph rather than from the syntax,
/// so a `goto` out of a block or a decompiler's flattened dispatch cannot fool
/// it the way counting braces can.
#[pyfunction]
#[pyo3(name = "control_dependence")]
pub fn control_dependence_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyList>> {
    use cindergraph::csource::semantic::{AnalysisUnit, ANALYSIS_REVISION};
    use cindergraph::syntax::dominance::ControlDependence;

    let built = py.detach(|| {
        let unit = AnalysisUnit::new(text);
        unit.functions()
            .iter()
            .enumerate()
            .map(|(index, function)| {
                let cdg = ControlDependence::of(&function.cfg);
                let nodes: Vec<(u32, String, u32, Option<u32>)> = (0..function.cfg.node_count()
                    as u32)
                    .map(|id| {
                        let kind = function
                            .cfg
                            .node(cindergraph::syntax::ids::NodeId::new(id))
                            .map(|node| node.kind().name().to_string())
                            .unwrap_or_default();
                        (id, kind, cdg.depth(id), cdg.post_dominators().immediate(id))
                    })
                    .collect();
                let edges: Vec<(u32, u32, &'static str)> = cdg
                    .edges()
                    .iter()
                    .map(|edge| (edge.on, edge.node, edge.kind.name()))
                    .collect();
                let stuck: Vec<u32> = cdg.post_dominators().dead_ends().to_vec();
                (
                    function.name.clone(),
                    unit.source_id(),
                    index as u32,
                    nodes,
                    edges,
                    stuck,
                )
            })
            .collect::<Vec<_>>()
    });

    let out = PyList::empty(py);
    for (name, source_id, function_id, nodes, edges, stuck) in built {
        let entry = PyDict::new(py);
        entry.set_item("name", name)?;
        entry.set_item("source_id", source_id.name())?;
        entry.set_item("function_id", function_id)?;
        entry.set_item("analysis_revision", ANALYSIS_REVISION)?;
        entry.set_item("graph_kind", "executable_cfg")?;
        let node_list = PyList::empty(py);
        for (id, kind, depth, ipdom) in nodes {
            let item = PyDict::new(py);
            item.set_item("id", id)?;
            item.set_item("kind", kind)?;
            item.set_item("depth", depth)?;
            item.set_item("ipdom", ipdom)?;
            node_list.append(item)?;
        }
        entry.set_item("nodes", node_list)?;
        let edge_list = PyList::empty(py);
        for (on, node, kind) in edges {
            let item = PyDict::new(py);
            item.set_item("on", on)?;
            item.set_item("node", node)?;
            item.set_item("kind", kind)?;
            edge_list.append(item)?;
        }
        entry.set_item("edges", edge_list)?;
        entry.set_item("unreachable_exit", stuck)?;
        out.append(entry)?;
    }
    Ok(out)
}

/// The backward slice of one function's CFG node, over the program-dependence
/// graph.
///
/// Every node whose execution or value can affect `node`, found by walking
/// control and data dependence backwards to a fixed point.
#[pyfunction]
#[pyo3(name = "backward_slice")]
#[pyo3(signature = (text, function, node, *, source_id=None, function_id=None, graph_kind=None, analysis_revision=None))]
// The optional identity coordinates deliberately remain separate Python
// keywords; grouping them into a Rust-only options object would make the
// binding signature diverge from the documented public API.
#[allow(clippy::too_many_arguments)]
pub fn backward_slice_py(
    py: Python<'_>,
    text: &str,
    function: &str,
    node: u32,
    source_id: Option<&str>,
    function_id: Option<u32>,
    graph_kind: Option<&str>,
    analysis_revision: Option<u32>,
) -> PyResult<Vec<u32>> {
    use cindergraph::csource::semantic::AnalysisUnit;

    let unit = py.detach(|| AnalysisUnit::new(text));
    py.detach(|| {
        backward_slice_unit(
            &unit,
            function,
            node,
            source_id,
            function_id,
            graph_kind,
            analysis_revision,
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn backward_slice_unit(
    unit: &AnalysisUnit,
    function: &str,
    node: u32,
    source_id: Option<&str>,
    function_id: Option<u32>,
    graph_kind: Option<&str>,
    analysis_revision: Option<u32>,
) -> PyResult<Vec<u32>> {
    use cindergraph::csource::semantic::ANALYSIS_REVISION;
    use cindergraph::syntax::dominance::{backward_slice, ControlDependence};

    let flows = unit.dataflows();
    if source_id.is_some_and(|expected| expected != unit.source_id().name()) {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "source_id does not identify the supplied source text",
        ));
    }
    if graph_kind.is_some_and(|kind| kind != "executable_cfg") {
        return Err(pyo3::exceptions::PyValueError::new_err(
            "backward_slice requires graph_kind='executable_cfg'",
        ));
    }
    if analysis_revision.is_some_and(|revision| revision != ANALYSIS_REVISION) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "analysis_revision does not match {ANALYSIS_REVISION}"
        )));
    }
    let mut matches = unit
        .functions()
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.name == function);
    let (index, candidate) = matches.next().ok_or_else(|| {
        pyo3::exceptions::PyKeyError::new_err(format!("no function named {function:?}"))
    })?;
    if matches.next().is_some() {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "ambiguous function name {function:?}: multiple definitions"
        )));
    }
    if function_id.is_some_and(|expected| expected != index as u32) {
        return Err(pyo3::exceptions::PyValueError::new_err(format!(
            "function_id does not identify function {function:?}"
        )));
    }
    if node as usize >= candidate.cfg.node_count() {
        return Err(pyo3::exceptions::PyIndexError::new_err(format!(
            "node {node} is outside function {function:?} ({} nodes)",
            candidate.cfg.node_count()
        )));
    }
    let cdg = ControlDependence::of(&candidate.cfg);
    let flow = &flows[index];
    let mut data: Vec<(u32, u32)> = flow
        .edges
        .iter()
        .filter_map(|edge| {
            Some((
                flow.definitions.get(edge.def as usize)?.node,
                flow.uses.get(edge.use_ as usize)?.node,
            ))
        })
        .collect();
    data.extend(flow.memory_edges.iter().filter_map(|edge| {
        Some((
            flow.memory_definitions.get(edge.definition as usize)?.node,
            flow.memory_uses.get(edge.use_ as usize)?.node,
        ))
    }));
    Ok(backward_slice(&cdg, &data, node))
}

/// What each function does with the values passed to it, across calls.
///
/// Interprocedural summaries at a fixed point over the call graph: which
/// parameter reaches the return, and which reaches which other parameter. A
/// summary is marked incomplete when the body held something this analysis
/// could not resolve --- an indirect call, or a callee this translation unit
/// does not define --- so a caller inherits `unknown` rather than a clean no.
#[pyfunction]
#[pyo3(name = "call_summaries")]
pub fn call_summaries_py<'py>(py: Python<'py>, text: &str) -> PyResult<Bound<'py, PyList>> {
    let unit = py.detach(|| {
        let unit = AnalysisUnit::new(text);
        let _ = unit.summaries();
        unit
    });
    call_summaries_dict(py, &unit)
}

fn call_summaries_dict<'py>(py: Python<'py>, unit: &AnalysisUnit) -> PyResult<Bound<'py, PyList>> {
    use cindergraph::csource::dataflow::Sink;

    let summaries = unit.summaries();
    let out = PyList::empty(py);
    for summary in summaries.iter() {
        let entry = PyDict::new(py);
        entry.set_item("name", summary.name.clone())?;
        entry.set_item("source_id", summary.source_id.name())?;
        entry.set_item("input_dialect", unit.options().dialect.name())?;
        entry.set_item("external_call_policy", unit.options().external_calls.name())?;
        entry.set_item("function_id", summary.function_id.map(|id| id.0))?;
        entry.set_item("analysis_revision", summary.analysis_revision)?;
        entry.set_item("parameters", summary.parameters)?;
        entry.set_item("complete", summary.complete)?;
        entry.set_item("memory_effects_complete", summary.memory_effects_complete)?;
        let memory_effects = PyList::empty(py);
        for effect in &summary.memory_effects {
            use cindergraph::csource::dataflow::MemoryEffectPath;
            let item = PyDict::new(py);
            item.set_item("parameter", effect.parameter)?;
            item.set_item("kind", effect.kind.name())?;
            item.set_item("precision", effect.precision.name())?;
            let path = effect
                .path
                .iter()
                .map(|component| match component {
                    MemoryEffectPath::Field(member) => format!(".{member}"),
                    MemoryEffectPath::Elements => "[*]".to_owned(),
                })
                .collect::<Vec<_>>();
            item.set_item("path", path)?;
            memory_effects.append(item)?;
        }
        entry.set_item("memory_effects", memory_effects)?;
        let flows = PyList::empty(py);
        for (index, sink) in &summary.flows {
            let item = PyDict::new(py);
            item.set_item("parameter", *index)?;
            match sink {
                Sink::Return => {
                    item.set_item("sink", "return")?;
                    item.set_item("sink_parameter", py.None())?;
                }
                Sink::Parameter(other) => {
                    item.set_item("sink", "parameter")?;
                    item.set_item("sink_parameter", *other)?;
                }
            }
            flows.append(item)?;
        }
        entry.set_item("flows", flows)?;
        out.append(entry)?;
    }
    Ok(out)
}

/// Persistent Python owner of one parsed and analyzed source snapshot.
#[pyclass(
    name = "AnalysisSession",
    module = "cindergraph._native.source",
    frozen
)]
struct PyAnalysisSession {
    unit: AnalysisUnit,
}

#[pymethods]
impl PyAnalysisSession {
    #[new]
    #[pyo3(signature = (text, *, dialect=None, external_calls=None))]
    fn new(
        py: Python<'_>,
        text: String,
        dialect: Option<&str>,
        external_calls: Option<&str>,
    ) -> PyResult<Self> {
        let dialect = input_dialect(dialect)?;
        let external_calls = external_call_policy(external_calls)?;
        Ok(Self {
            unit: py.detach(|| {
                AnalysisUnit::with_options(
                    text,
                    AnalysisOptions {
                        dialect,
                        external_calls,
                    },
                )
            }),
        })
    }

    #[getter]
    fn source_id(&self) -> String {
        self.unit.source_id().name()
    }

    #[getter]
    fn source(&self) -> &str {
        self.unit.source()
    }

    #[getter]
    fn dialect(&self) -> &str {
        self.unit.options().dialect.name()
    }

    #[getter]
    fn external_calls(&self) -> &str {
        self.unit.options().external_calls.name()
    }

    #[getter]
    fn diagnostics<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        diagnostics_list(py, self.unit.diagnostics(), self.unit.source())
    }

    fn control_flow_graphs<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        control_flow_graphs_dict(py, &self.unit)
    }

    fn data_flow<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        py.detach(|| {
            let _ = self.unit.dataflows();
        });
        data_flow_dict(py, &self.unit)
    }

    fn call_summaries<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        py.detach(|| {
            let _ = self.unit.summaries();
        });
        call_summaries_dict(py, &self.unit)
    }

    #[pyo3(signature = (repr, format))]
    fn export_graphs<'py>(
        &self,
        py: Python<'py>,
        repr: &str,
        format: &str,
    ) -> PyResult<Bound<'py, PyList>> {
        export_graphs_dict(py, &self.unit, repr, format)
    }

    #[pyo3(signature = (*, repr="cfg"))]
    fn native_graphs<'py>(&self, py: Python<'py>, repr: &str) -> PyResult<Bound<'py, PyList>> {
        native_graphs_list(py, &self.unit, repr)
    }

    #[pyo3(signature = (function, node, *, function_id=None))]
    fn backward_slice(
        &self,
        py: Python<'_>,
        function: &str,
        node: u32,
        function_id: Option<u32>,
    ) -> PyResult<Vec<u32>> {
        py.detach(|| backward_slice_unit(&self.unit, function, node, None, function_id, None, None))
    }

    fn query_reaches<'py>(
        &self,
        py: Python<'py>,
        source: &str,
        parameter: u32,
        sink: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let answer = py.detach(|| {
            cindergraph::csource::dataflow::interproc::reaches_detailed(
                self.unit.summaries(),
                source,
                parameter,
                sink,
            )
        });
        reachability_dict(py, &self.unit, &answer)
    }

    fn query_reaches_by_id<'py>(
        &self,
        py: Python<'py>,
        source_function_id: u32,
        parameter: u32,
        sink_function_id: u32,
    ) -> PyResult<Bound<'py, PyDict>> {
        use cindergraph::csource::semantic::FunctionId;

        let answer = py.detach(|| {
            cindergraph::csource::dataflow::interproc::reaches_by_id_detailed(
                self.unit.summaries(),
                FunctionId(source_function_id),
                parameter,
                FunctionId(sink_function_id),
            )
        });
        reachability_dict(py, &self.unit, &answer)
    }
}

fn reachability_dict<'py>(
    py: Python<'py>,
    unit: &AnalysisUnit,
    answer: &cindergraph::csource::dataflow::interproc::Reachability,
) -> PyResult<Bound<'py, PyDict>> {
    use cindergraph::csource::dataflow::interproc::Flow;
    use cindergraph::csource::semantic::ANALYSIS_REVISION;

    let out = PyDict::new(py);
    out.set_item(
        "claim",
        match answer.verdict {
            Flow::Yes => "found_may_path",
            Flow::No => "no_may_path",
            Flow::Unknown => "unknown",
        },
    )?;
    out.set_item("verdict", answer.verdict.name())?;
    out.set_item("source_id", unit.source_id().name())?;
    out.set_item("input_dialect", unit.options().dialect.name())?;
    out.set_item("external_call_policy", unit.options().external_calls.name())?;
    out.set_item("analysis_revision", ANALYSIS_REVISION)?;
    out.set_item("coverage_complete", answer.verdict == Flow::No)?;
    let steps = |items: &[cindergraph::csource::dataflow::interproc::ReachabilityStep]| {
        items
            .iter()
            .map(|step| {
                (
                    step.function_id.map(|id| id.0),
                    step.function.clone(),
                    step.parameter,
                )
            })
            .collect::<Vec<_>>()
    };
    out.set_item("path", steps(&answer.path))?;
    out.set_item("explored", steps(&answer.explored))?;
    out.set_item(
        "uncertainty",
        answer
            .uncertainty
            .iter()
            .map(|reason| reason.name())
            .collect::<Vec<_>>(),
    )?;
    Ok(out)
}

/// Whether a value in one function's parameter can reach another function.
///
/// Returns `"yes"`, `"no"` or `"unknown"`. The third is not a failure: an
/// indirect call names no callee and a function defined in another translation
/// unit has no body here, and reporting either as `"no"` would be a claim
/// rather than an analysis.
#[pyfunction]
#[pyo3(name = "reaches")]
#[pyo3(signature = (text, source, parameter, sink))]
pub fn reaches_py(
    py: Python<'_>,
    text: &str,
    source: &str,
    parameter: u32,
    sink: &str,
) -> PyResult<String> {
    use cindergraph::csource::dataflow::{analyze, interproc::reaches, summarize};

    let verdict = py.detach(|| {
        let summaries = summarize(&analyze(text).into_parts().0);
        reaches(&summaries, source, parameter, sink)
    });
    Ok(verdict.name().to_string())
}

/// Structured reachability claim with path or explicit uncertainty evidence.
#[pyfunction]
#[pyo3(name = "query_reaches")]
#[pyo3(signature = (text, source, parameter, sink))]
pub fn query_reaches_py<'py>(
    py: Python<'py>,
    text: &str,
    source: &str,
    parameter: u32,
    sink: &str,
) -> PyResult<Bound<'py, PyDict>> {
    let (unit, answer) = py.detach(|| {
        let unit = AnalysisUnit::new(text);
        let answer = cindergraph::csource::dataflow::interproc::reaches_detailed(
            unit.summaries(),
            source,
            parameter,
            sink,
        );
        (unit, answer)
    });
    reachability_dict(py, &unit, &answer)
}

/// Identity-first reachability claim for duplicate-safe queries.
#[pyfunction]
#[pyo3(name = "query_reaches_by_id")]
#[pyo3(signature = (text, source_function_id, parameter, sink_function_id))]
pub fn query_reaches_by_id_py<'py>(
    py: Python<'py>,
    text: &str,
    source_function_id: u32,
    parameter: u32,
    sink_function_id: u32,
) -> PyResult<Bound<'py, PyDict>> {
    use cindergraph::csource::semantic::FunctionId;

    let (unit, answer) = py.detach(|| {
        let unit = AnalysisUnit::new(text);
        let answer = cindergraph::csource::dataflow::interproc::reaches_by_id_detailed(
            unit.summaries(),
            FunctionId(source_function_id),
            parameter,
            FunctionId(sink_function_id),
        );
        (unit, answer)
    });
    reachability_dict(py, &unit, &answer)
}

/// Register the `source` submodule on the extension root.
pub fn register_source_metrics_bindings(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    let sub = PyModule::new(m.py(), "source")?;
    sub.add_class::<PyNativeGraph>()?;
    sub.add_class::<PyAnalysisSession>()?;
    sub.add_function(wrap_pyfunction!(analyze_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(functions_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(control_flow_graphs_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(feature_names_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(features_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(normalize_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(export_graphs_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(native_graphs_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(export_choices_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(data_flow_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(control_dependence_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(backward_slice_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(call_summaries_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(reaches_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(query_reaches_py, &sub)?)?;
    sub.add_function(wrap_pyfunction!(query_reaches_by_id_py, &sub)?)?;
    m.add_submodule(&sub)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The column names and the row builder are two lists that must agree, and
    /// nothing but this test makes them. A mismatch would not fail anywhere:
    /// it would silently shift every column after the missing one, and a
    /// consumer stacking rows would train on scrambled features.
    #[test]
    fn the_feature_names_and_the_feature_row_are_the_same_length() {
        let report =
            metrics::analyze("int f(int a) { if (a && a) { while (a) { a--; } } return a; }")
                .into_parts()
                .0;
        let function = report.functions.first().expect("one function");
        assert_eq!(
            FEATURE_NAMES.len(),
            feature_row(function).len(),
            "FEATURE_NAMES and feature_row disagree"
        );
    }

    /// Every kind column must name a real `NodeKind`, or it silently reports 0
    /// forever.
    #[test]
    fn every_kind_column_names_a_real_node_kind() {
        use cindergraph::syntax::cfg::NodeKind;
        let known: Vec<&str> = [
            NodeKind::Entry,
            NodeKind::Exit,
            NodeKind::Stmt,
            NodeKind::Cond,
            NodeKind::LoopHeader,
            NodeKind::Switch,
            NodeKind::IndirectDispatch,
            NodeKind::Case,
            NodeKind::Label,
            NodeKind::Goto,
            NodeKind::Break,
            NodeKind::Continue,
            NodeKind::Return,
            NodeKind::Diverge,
        ]
        .iter()
        .map(|k| k.name())
        .collect();
        for column in KIND_COLUMNS {
            assert!(
                known.contains(column),
                "unknown node kind column {column:?}"
            );
        }
        assert_eq!(
            KIND_COLUMNS.len(),
            known.len(),
            "a NodeKind exists that no feature column reports"
        );
    }

    /// A feature row is the same numbers the report carries. If these two paths
    /// ever disagree, one of them is wrong and neither says so.
    #[test]
    fn a_feature_row_agrees_with_the_report_it_flattens() {
        let text = "int f(int a) { for (int i = 0; i < a; i++) { if (i) { a--; } } return a; }";
        let report = metrics::analyze(text).into_parts().0;
        let function = report.functions.first().expect("one function");
        let row = feature_row(function);
        let column = |name: &str| -> f64 {
            let index = FEATURE_NAMES
                .iter()
                .position(|n| *n == name)
                .unwrap_or_else(|| panic!("no column {name}"));
            row[index]
        };
        assert_eq!(column("cyclomatic"), f64::from(function.graph.cyclomatic));
        assert_eq!(column("cognitive"), f64::from(function.shape.cognitive));
        assert_eq!(column("max_nesting"), f64::from(function.shape.max_nesting));
        assert_eq!(column("loops"), f64::from(function.graph.loops));
        assert_eq!(
            column("nodes_loop_header"),
            f64::from(
                *function
                    .graph
                    .node_kinds
                    .iter()
                    .find(|(k, _)| k.name() == "loop_header")
                    .map(|(_, c)| c)
                    .unwrap_or(&0)
            )
        );
    }
}
