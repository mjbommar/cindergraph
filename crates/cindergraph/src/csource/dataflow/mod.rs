//! Reaching definitions and the data-dependence graph over C source.
//!
//! Where a value is written, where it is read, and which write each read can
//! see. That question is the third graph a source front end owes its callers,
//! after the syntax tree and the control-flow graph, and it is what turns
//! "these two functions have the same shape" into "this value flows here".
//!
//! # What this computes
//!
//! A classic forward monotone dataflow analysis over [`crate::csource::cfg`],
//! the general graph:
//!
//! * `GEN(n)` is the set of definitions the node writes;
//! * `KILL(n)` is every other definition of the same variable;
//! * `IN(n)` is the union of the `OUT` of the predecessors;
//! * `OUT(n) = GEN(n) | (IN(n) - KILL(n))`.
//!
//! Iterated to a fixed point, then one edge per (definition, use) pair where
//! the definition reaches the node the use is on.
//!
//! # Why this is not a copy of Joern's DDG
//!
//! Measured head to head on one 17-line file, both front ends in one CPython
//! 3.12 process (2026-09-05):
//!
//! | | nodes | edges | edges naming a variable |
//! |---|---:|---:|---:|
//! | Joern / Eclipse CDT | 13 | 23 | **0** |
//! | this module | 11 | 8 | **8** |
//!
//! Two differences, and neither is a shortfall.
//!
//! **Ours are labelled.** pyjoern's `Function.ddg` returns every edge with an
//! empty attribute dict, so a consumer of *that* API cannot tell which value
//! an edge is about. Every edge here names its variable, and both endpoints
//! carry the spelling, the kind of write and the byte range.
//!
//! To be exact about the comparison: `joern-export --repr ddg` writes DOT that
//! *does* carry a `DDG: <name>` label, so the information exists upstream and
//! it is pyjoern's lifted view that drops it. The claim is about the library
//! API a Python caller actually uses, not about Joern being unable to compute
//! it.
//!
//! **Ours are variable dependences, not block adjacency.** Joern's graph is
//! over CFG blocks: of its 23 edges, 9 leave `FUNCTION_START` and 5 enter
//! `FUNCTION_END`, which say that a block is reachable rather than that a
//! value flows. Audited edge by edge, all 8 of ours are real
//! definition-to-use pairs and none of Joern's 23 is one we lost.
//!
//! # Scoping, which is the part that is actually hard
//!
//! A definition is not a name, it is a name *in a scope*. `int x` inside a
//! block is a different variable from the `x` outside it, and treating the two
//! as one produces edges that do not exist. Every declaration therefore binds
//! into a scope stack that opens at `{` and closes at `}`, and a use resolves
//! to the innermost binding visible at its offset. A name with no visible
//! binding receives a dense unresolved-name ID after lexical resolution.
//! Repeated uses of one spelling share an ID; different globals and local
//! shadows remain distinct. Unresolved names do not imply a recovered type.
//!
//! This is a local lexical binding model over recovered declarations, not a
//! full C symbol table: included declarations, typedef expansion and global
//! linkage resolution are not provided by this pass.
//!
//! # Coverage and limitations
//!
//! * **Local points-to sets.** Address-taking and pointer copies identify
//!   possible local targets. Indirect assignments add weak definitions of
//!   these targets; they do not kill other possible definitions. This is
//!   flow-insensitive and can include spurious dependences.
//! * **Incomplete memory coverage.** Fields and array elements have explicit
//!   abstract regions, operation-owned accesses, and possible reaching-write
//!   edges. Known union members overlap explicitly; unknown pointees, byte
//!   layout and full aggregate aliasing remain unsupported.
//!   `DataFlow::memory_complete` exposes these gaps, so missing edges are not
//!   proven independence.
//! * **Bounded interprocedural summaries.** [`summarize`] propagates parameter
//!   dependence and formal-pointee read/write effects through known direct
//!   calls. Complete effects are instantiated on caller regions as weak writes
//!   or reads; incomplete, indirect, external-unknown, and ambiguous callees
//!   retain conservative clobbers. Global effects are not yet modeled.
//!   Address-taking is recorded as a potential local definition, not a proof
//!   that the pointed-to value is overwritten.
//! * **No constant folding**, for the same reason
//!   [`crate::csource::metrics`]'s unreachable count is a lower bound.
//!
//! The fixed point is computed over the recovered CFG and extracted events,
//! not over all C execution semantics. Recovery, alias approximations and
//! unsupported expressions can introduce or omit dependencies. In particular,
//! VLA-size provenance is incomplete and is not fully reflected in summary
//! completeness. A complete summary is not a general C soundness certificate.
//!
//! # How the defect counts were calibrated
//!
//! The two numbers this produces --- dead stores and unresolved uses --- are
//! only worth reading if ordinary code scores near zero, so each was measured
//! against `tests/decompiler_fixtures/src` (196 files, 900 functions) and the
//! analysis fixed until it did. Every step was a real defect, and the corpus
//! test in `tests.rs` fails if any of them returns:
//!
//! | reported | cause |
//! |---|---|
//! | 897 dead | a bare `int x;` counted as a store |
//! | 460 dead | a write to a global counted, though it escapes the function |
//! | 27 dead | `++a[i]` counted as a write to `a` |
//! | **12 dead** | what hand-written C actually contains |
//!
//! Unresolved uses moved 1,970 to 962 over the same period: type names in
//! `sizeof(T)` and cast position were being counted as reads of undefined
//! variables, and callee names were too.
//!
//! Measured against our own decompiler's output for ten of those fixtures,
//! the same analysis reports **4.5% of writes dead against 0.0% for the
//! source** --- 33 writes the recovered code performs and never reads. The
//! execution differential passes every one of them, which is the point: this
//! sees a defect class that testing the return value cannot.

pub mod events;
pub mod interproc;
mod memory;
mod memory_solve;
pub mod model;
mod provenance;
mod regions;
pub mod solve;
pub mod types;

use crate::csource::semantic::declarations::{
    resolve_function, FunctionResolution, TranslationUnitSymbols,
};
use crate::csource::semantic::types::{resolve_types, FunctionTypes};

#[cfg(test)]
mod tests;

pub use interproc::{
    reaches_by_id_detailed, reaches_detailed, summarize, summarize_with_policy, ExternalCallPolicy,
    Flow, MemoryEffectPath, ParameterMemoryEffect, ParameterMemoryEffectKind, Reachability,
    ReachabilityStep, ReachabilityUncertainty, Sink, Summaries, Summary,
};
pub use model::{
    Binding, CType, CallMemoryArgument, CallRecord, CoverageDimension, DataFlow, DefKind,
    Definition, FlowEdge, MemoryAccess, MemoryAccessKind, MemoryAccessPrecision, MemoryDefinition,
    MemoryDefinitionKind, MemoryFlowEdge, MemoryOverlapKind, MemoryRegion, MemoryRegionId,
    MemoryRegionKind, MemoryRegionOverlap, MemoryUse, SemanticIssue, SemanticIssueKind, Use,
};

use crate::csource::cfg::FunctionCfg;
use crate::csource::eval::EvaluationPlan;
use crate::csource::parse::Tree;
use crate::csource::semantic::{AnalysisUnit, FunctionId, SourceUnitId, ANALYSIS_REVISION};
use crate::syntax::diag::Parsed;
use crate::syntax::ids::Span;

/// Analyze every function in one translation unit.
///
/// Total on every input (`REQ-SYN-2`): a file that is not C yields no
/// functions and the diagnostics saying so, and a function the parser only
/// partly recovered is analyzed over the graph it did build.
pub fn analyze(text: &str) -> Parsed<Vec<DataFlow>> {
    let unit = AnalysisUnit::new(text);
    let flows = analyze_unit(&unit);
    Parsed::new(flows, unit.diagnostics().clone())
}

/// Analyze every function while preserving one owning source/parse/CFG unit.
pub fn analyze_unit(unit: &AnalysisUnit) -> Vec<DataFlow> {
    let recovery_spans = unit
        .diagnostics()
        .iter()
        .map(|diagnostic| diagnostic.span)
        .collect::<Vec<_>>();
    let mut flows = unit
        .functions()
        .iter()
        .zip(unit.resolutions())
        .zip(unit.types())
        .zip(unit.evaluations())
        .enumerate()
        .map(
            |(index, (((function, resolution), structural_types), evaluation))| {
                let mut flow = analyze_function_with_context(FunctionAnalysisInput {
                    tree: unit.tree(),
                    text: unit.source(),
                    token_spans: unit.token_spans(),
                    function,
                    resolution,
                    structural_types,
                    evaluation,
                    symbols: unit.symbols(),
                    source_id: unit.source_id(),
                    function_id: FunctionId(index as u32),
                });
                // Recovery can change declaration boundaries and name resolution,
                // so span overlap alone cannot safely localize its effects.
                flow.replace_recovery_issues(
                    (!recovery_spans.is_empty())
                        .then_some(model::SemanticIssueKind::RecoveredSyntax),
                    &recovery_spans,
                );
                flow
            },
        )
        .collect::<Vec<_>>();
    let summaries = summarize_with_policy(&flows, unit.options().external_calls);
    memory::refine_known_calls(&mut flows, unit.functions(), &summaries);
    flows
}

/// Analyze one function whose graph is already built.
///
/// Diagnostic context is absent here, so `recovery_free` stays false. Use
/// [`analyze`] to retain the translation unit's recovery status.
pub fn analyze_function(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    function: &FunctionCfg,
) -> DataFlow {
    let typedefs = TranslationUnitSymbols::collect(tree, text, token_spans);
    let resolution = resolve_function(
        tree,
        text,
        token_spans,
        function.node,
        function.span,
        function.name_span,
        &typedefs,
    );
    let structural_types = resolve_types(
        tree,
        text,
        token_spans,
        function.node,
        &resolution,
        &typedefs,
        function.span.lo,
    );
    let evaluation = EvaluationPlan::build(
        tree,
        text,
        token_spans,
        function,
        &resolution,
        &structural_types,
    );
    analyze_function_with_context(FunctionAnalysisInput {
        tree,
        text,
        token_spans,
        function,
        resolution: &resolution,
        structural_types: &structural_types,
        evaluation: &evaluation,
        symbols: &typedefs,
        source_id: SourceUnitId::of(text),
        function_id: FunctionId::UNKNOWN,
    })
}

struct FunctionAnalysisInput<'a> {
    tree: &'a Tree,
    text: &'a str,
    token_spans: &'a [Span],
    function: &'a FunctionCfg,
    resolution: &'a FunctionResolution,
    structural_types: &'a FunctionTypes,
    evaluation: &'a EvaluationPlan,
    symbols: &'a TranslationUnitSymbols,
    source_id: SourceUnitId,
    function_id: FunctionId,
}

fn analyze_function_with_context(input: FunctionAnalysisInput<'_>) -> DataFlow {
    let FunctionAnalysisInput {
        tree,
        text,
        token_spans,
        function,
        resolution,
        structural_types,
        evaluation,
        symbols,
        source_id,
        function_id,
    } = input;
    let events = events::collect_events(
        tree,
        text,
        token_spans,
        function,
        events::SemanticContext {
            resolution,
            types: structural_types,
            evaluation,
            symbols,
        },
    );
    let bound_captures = events.bound_captures;
    let binding_by_place = events.binding_by_place;
    let mut flow = DataFlow {
        source_id,
        function_id,
        analysis_revision: ANALYSIS_REVISION,
        node_spans: function
            .cfg
            .nodes()
            .iter()
            .map(|node| node.span())
            .collect(),
        discarded_values: tree
            .arena()
            .preorder(function.node)
            .filter(|node| {
                tree.arena().tag(*node)
                    == Some(crate::csource::parse::tag::NodeTag::CommaExpr.as_u16())
            })
            .flat_map(|node| {
                let children: Vec<_> = tree.arena().children_iter(node).collect();
                let owner = tree.arena().span(node, token_spans);
                children
                    .iter()
                    .take(children.len().saturating_sub(1))
                    .filter_map(|child| Some((owner?, tree.arena().span(*child, token_spans)?)))
                    .collect::<Vec<_>>()
            })
            .collect(),
        recovery_free: false,
        effects_complete: true,
        memory_complete: true,
        vla_complete: true,
        control_targets_complete: true,
        semantic_issues: events.semantic_issues,
        unresolved_bindings: events.unresolved,
        return_spans: tree
            .arena()
            .preorder(function.node)
            .filter(|node| {
                tree.arena().tag(*node)
                    == Some(crate::csource::parse::tag::NodeTag::ReturnStmt.as_u16())
            })
            .filter_map(|node| tree.arena().span(node, token_spans))
            .collect(),
        control_edges: crate::syntax::dominance::ControlDependence::of(&function.cfg)
            .edges()
            .iter()
            .map(|edge| (edge.on, edge.node))
            .collect(),
        return_nodes: function
            .cfg
            .nodes()
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind() == crate::syntax::cfg::NodeKind::Return)
            .map(|(index, _)| index as u32)
            .collect(),
        name: function.name.clone(),
        definitions: events.definitions,
        uses: events.uses,
        types: events.types,
        names: events.names,
        calls: events.calls,
        call_memory_arguments: Vec::new(),
        memory_regions: Vec::new(),
        memory_overlaps: Vec::new(),
        memory_accesses: Vec::new(),
        memory_definitions: Vec::new(),
        memory_uses: Vec::new(),
        memory_edges: Vec::new(),
        edges: Vec::new(),
        unresolved_uses: Vec::new(),
        dead_stores: Vec::new(),
    };
    for dispatch in function.cfg.indirect_dispatches() {
        if dispatch.precision == crate::syntax::cfg::TargetPrecision::Conservative {
            flow.record_issue(
                model::SemanticIssueKind::UnresolvedControlTarget,
                function.cfg.node(dispatch.node).map(|node| node.span()),
            );
        }
    }
    memory::project_writes(
        memory::ProjectionContext {
            tree,
            text,
            spans: token_spans,
            function,
            evaluation,
            binding_by_place: &binding_by_place,
            unevaluated: &events.unevaluated,
        },
        &mut flow,
    );
    memory_solve::solve(&mut flow, &function.cfg);
    solve::solve(&mut flow, &function.cfg);
    project_bound_captures(&mut flow, bound_captures);
    flow.refresh_semantic_issues(Some(model::SemanticIssueKind::RecoveryContextUnavailable));
    flow
}

fn project_bound_captures(flow: &mut DataFlow, captures: Vec<events::BoundCapture>) {
    for capture in captures {
        let source_uses = flow
            .uses
            .iter()
            .enumerate()
            .filter(|(_, use_)| {
                use_.binding == capture.use_.binding
                    && capture.source_expression.lo <= use_.span.lo
                    && use_.span.hi <= capture.source_expression.hi
            })
            .map(|(index, _)| index as u32)
            .collect::<Vec<_>>();
        let reaching = flow
            .edges
            .iter()
            .filter(|edge| source_uses.contains(&edge.use_))
            .map(|edge| edge.def)
            .collect::<std::collections::BTreeSet<_>>();
        if reaching.is_empty() {
            // The structural slot conservatively retains identifiers that a
            // constant/type operator may leave unevaluated. Event lowering
            // emits issues for opaque evaluated bounds; absence of a reaching
            // source use here therefore means there is no captured value edge.
            continue;
        }
        let use_index = flow.uses.len() as u32;
        let name = capture.use_.name.clone();
        flow.uses.push(capture.use_);
        flow.edges
            .extend(reaching.into_iter().map(|def| model::FlowEdge {
                def,
                use_: use_index,
                name: name.clone(),
            }));
    }
}
