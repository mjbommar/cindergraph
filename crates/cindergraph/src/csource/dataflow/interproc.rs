//! Dependence that crosses a call, by summary rather than by inlining.
//!
//! The intraprocedural analysis in [`super::solve`] stops at the call: a call
//! records argument uses but does not by itself model callee effects. This
//! module adds parameter-to-return summaries and cross-call reachability.
//! Caller-visible memory effects remain outside that model.
//!
//! # Summaries, not inlining
//!
//! For each function, compute which parameters flow to the return value.
//! Caller-visible pointee outputs are not yet modeled. A caller then *applies* the
//! summary instead of re-analysing the callee.
//!
//! Inlining is easier to write and does not terminate on recursion. Summaries
//! do, because the lattice is finite --- a summary is a set of (parameter,
//! destination) pairs and there are finitely many --- so iterating over the
//! call graph to a fixed point converges without recursive inlining. The caller
//! worklist can revisit functions many times as dependencies propagate. A total
//! evaluation budget bounds the work; exhaustion marks summaries incomplete.
//!
//! # Where this refuses, and why refusing is the point
//!
//! Sources of uncertainty include:
//!
//! * **An indirect call.** `p(x)` names no callee, and
//!   [`super::super::metrics`]'s call graph deliberately contributes no edge
//!   for one. A summary cannot be applied to a function that was not named.
//! * **A callee this unit does not define.** `memcpy(dst, src, n)` is an edge
//!   to a name whose body is elsewhere. What it does with its arguments is not
//!   knowable from one translation unit, and guessing "it propagates" or "it
//!   does not" would be a claim rather than an analysis. A curated table of
//!   libc effects is the obvious next increment and deliberately not smuggled
//!   in here.
//! * **A work bound exceeded.** Bounded rather than
//!   unbounded, so a pathological call graph costs an `Unknown` and not a
//!   hang.
//! * **Recovery, unresolved bindings or memory coverage.** Incomplete local
//!   analysis propagates to callers. Duplicate source names cannot select a
//!   parameter identity, even for a source-equals-sink query.
//!
//! A known positive path may still yield [`Flow::Yes`] in an incomplete
//! analysis. [`Flow::No`] requires the explored summaries to be complete;
//! unsupported semantic cases not detected by those flags remain limitations.
//!
//! A caller that treats `Unknown` as `No` gets an unsound answer; the type
//! exists so that mistake has to be written down.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use super::model::Binding;
use super::DataFlow;
use crate::csource::semantic::{FunctionId, SourceUnitId};

/// Whether a value reaches somewhere.
///
/// Three-valued on purpose. A reachability analysis that answers only yes or
/// no has to lie about the cases it cannot see, and on decompiler output ---
/// full of indirect calls and functions whose bodies are elsewhere --- those
/// are not rare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Flow {
    /// A path was found.
    Yes,
    /// No path exists, and every step of the search was decidable.
    No,
    /// The search met something it could not resolve: an indirect call, a
    /// callee defined elsewhere, or a bound.
    Unknown,
}

impl Flow {
    /// This verdict's stable name, for a serialized field.
    pub const fn name(self) -> &'static str {
        match self {
            Flow::Yes => "yes",
            Flow::No => "no",
            Flow::Unknown => "unknown",
        }
    }

    /// The weaker of two verdicts, for combining independent searches.
    ///
    /// `Yes` wins over everything --- one path is enough. `Unknown` beats `No`,
    /// because a search that could not see everywhere has not proven absence.
    pub fn or(self, other: Flow) -> Flow {
        match (self, other) {
            (Flow::Yes, _) | (_, Flow::Yes) => Flow::Yes,
            (Flow::Unknown, _) | (_, Flow::Unknown) => Flow::Unknown,
            _ => Flow::No,
        }
    }
}

/// How summaries model a named direct callee with no body in this snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExternalCallPolicy {
    /// Preserve uncertainty; the safe default.
    #[default]
    Unknown,
    /// Conservatively let every argument influence the return value while
    /// retaining incomplete coverage for other effects.
    TaintReturn,
    /// Explicitly assume the callee is pure and its return is independent of
    /// its arguments. This may produce complete negative answers.
    AssumePureNoFlow,
}

impl ExternalCallPolicy {
    /// Stable spelling used by public configuration and result metadata.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::TaintReturn => "taint_return",
            Self::AssumePureNoFlow => "assume_pure_no_flow",
        }
    }
}

/// Why a reachability query could not prove either presence or absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReachabilityUncertainty {
    /// The source name has more than one recovered definition.
    AmbiguousSource,
    /// The source function was not defined in this snapshot.
    MissingSource,
    /// The sink function identity was not defined in this snapshot.
    MissingSink,
    /// Recovered arity was insufficient and the source summary was incomplete.
    IncompleteArity,
    /// A traversed call target has no summary in this snapshot.
    MissingCallee,
    /// A traversed call name has more than one possible body.
    AmbiguousCallee,
    /// A traversed summary has incomplete semantic coverage.
    IncompleteSummary,
}

impl ReachabilityUncertainty {
    /// Stable serialized spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::AmbiguousSource => "ambiguous_source",
            Self::MissingSource => "missing_source",
            Self::MissingSink => "missing_sink",
            Self::IncompleteArity => "incomplete_arity",
            Self::MissingCallee => "missing_callee",
            Self::AmbiguousCallee => "ambiguous_callee",
            Self::IncompleteSummary => "incomplete_summary",
        }
    }
}

/// One function/parameter state on an interprocedural may-path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReachabilityStep {
    /// Exact function identity when the name resolved uniquely.
    pub function_id: Option<FunctionId>,
    /// Function whose formal parameter carries the value at this step.
    pub function: String,
    /// Zero-based formal parameter position.
    pub parameter: u32,
}

/// Structured result of an interprocedural reachability query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reachability {
    /// Three-valued compatibility verdict.
    pub verdict: Flow,
    /// Deterministic source-to-sink may-path when one was found.
    pub path: Vec<ReachabilityStep>,
    /// States explored before a negative or unknown result.
    pub explored: Vec<ReachabilityStep>,
    /// Stable reasons qualifying an unknown result.
    pub uncertainty: Vec<ReachabilityUncertainty>,
}

/// Where a value that entered a function can end up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Sink {
    /// The function's return value.
    Return,
    /// Through the pointer passed as parameter `n`, which the callee may write.
    /// Reserved for future memory summaries; not currently emitted.
    Parameter(u32),
}

/// One component of a caller-visible path below a formal pointee.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemoryEffectPath {
    /// A named record member.
    Field(String),
    /// The conservative summary of all array elements.
    Elements,
}

/// Direction of one caller-visible memory effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ParameterMemoryEffectKind {
    Read,
    Write,
}

impl ParameterMemoryEffectKind {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// A read or write below one formal pointer's incoming value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ParameterMemoryEffect {
    pub parameter: u32,
    pub path: Vec<MemoryEffectPath>,
    pub kind: ParameterMemoryEffectKind,
    pub precision: super::MemoryAccessPrecision,
}

/// What one function does with the values passed to it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    /// Exact source snapshot from which this summary was derived.
    pub source_id: SourceUnitId,
    /// Owning function, or `None` when duplicate names make it ambiguous.
    pub function_id: Option<FunctionId>,
    /// Semantic result contract revision.
    pub analysis_revision: u32,
    /// The function's name.
    pub name: String,
    /// How many parameters it declares. For ambiguous duplicate definitions,
    /// the maximum recovered arity is reported, not a selected signature.
    pub parameters: u32,
    /// `(parameter index, sink)` pairs: this parameter's value can reach that
    /// sink.
    pub flows: Vec<(u32, Sink)>,
    /// Caller-visible reads and writes rooted in formal pointees.
    pub memory_effects: Vec<ParameterMemoryEffect>,
    /// Whether the listed memory effects completely cover this body.
    pub memory_effects_complete: bool,
    /// Whether the body contained something this analysis could not resolve,
    /// so a caller applying this summary inherits an `Unknown` rather than a
    /// clean `No`.
    pub complete: bool,
}

fn direct_memory_effects(flow: &DataFlow) -> Vec<ParameterMemoryEffect> {
    let mut effects = Vec::new();
    for access in &flow.memory_accesses {
        let mut current = access.region;
        let mut path = Vec::new();
        let parameter = loop {
            match flow
                .memory_regions
                .get(current.0 as usize)
                .map(|region| &region.kind)
            {
                Some(super::MemoryRegionKind::ParameterPointee { parameter, .. }) => {
                    break Some(*parameter);
                }
                Some(super::MemoryRegionKind::Field { base, member, .. }) => {
                    path.push(MemoryEffectPath::Field(member.clone()));
                    current = *base;
                }
                Some(super::MemoryRegionKind::Elements { base }) => {
                    path.push(MemoryEffectPath::Elements);
                    current = *base;
                }
                _ => break None,
            }
        };
        let Some(parameter) = parameter else { continue };
        path.reverse();
        effects.push(ParameterMemoryEffect {
            parameter,
            path,
            kind: match access.kind {
                super::MemoryAccessKind::Read => ParameterMemoryEffectKind::Read,
                super::MemoryAccessKind::Write => ParameterMemoryEffectKind::Write,
            },
            precision: access.precision,
        });
    }
    effects.sort();
    effects.dedup();
    effects
}

fn composed_memory_effects(
    flow: &DataFlow,
    known: &BTreeMap<String, Summary>,
) -> (Vec<ParameterMemoryEffect>, bool) {
    let mut effects = direct_memory_effects(flow);
    let modeled_call_spans = flow
        .call_memory_arguments
        .iter()
        .map(|argument| argument.call_span)
        .collect::<BTreeSet<_>>();
    let mut complete = !flow.semantic_issues.iter().any(|issue| {
        issue.kind == super::SemanticIssueKind::UnknownMemoryEffect
            && issue
                .span
                .is_none_or(|span| !modeled_call_spans.contains(&span))
    });

    for call in &flow.calls {
        let arguments = flow
            .call_memory_arguments
            .iter()
            .filter(|argument| argument.call_span == call.span)
            .collect::<Vec<_>>();
        if arguments.is_empty() {
            continue;
        }
        let Some(callee) = call.callee.as_ref().and_then(|name| known.get(name)) else {
            complete = false;
            continue;
        };
        if !callee.memory_effects_complete {
            complete = false;
        }
        for callee_effect in &callee.memory_effects {
            let matching = arguments
                .iter()
                .filter(|argument| argument.argument == callee_effect.parameter)
                .copied()
                .collect::<Vec<_>>();
            if matching.is_empty() {
                complete = false;
                continue;
            }
            for argument in matching {
                if !argument.complete {
                    complete = false;
                }
                for &parameter in &argument.parameter_origins {
                    effects.push(ParameterMemoryEffect {
                        parameter,
                        path: callee_effect.path.clone(),
                        kind: callee_effect.kind,
                        precision: super::MemoryAccessPrecision::MayAlias,
                    });
                }
            }
        }
    }
    effects.sort();
    effects.dedup();
    (effects, complete)
}

impl Summary {
    /// Whether parameter `index` reaches `sink`.
    pub fn flows_to(&self, index: u32, sink: Sink) -> bool {
        self.flows.contains(&(index, sink))
    }
}

/// Every function's summary, keyed by name, at a fixed point.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summaries {
    by_name: BTreeMap<String, Summary>,
    by_id: BTreeMap<FunctionId, Summary>,
    external_by_name: BTreeMap<String, Summary>,
    calls: BTreeMap<String, Vec<(u32, String, u32)>>,
    calls_by_id: BTreeMap<FunctionId, Vec<(u32, String, u32)>>,
    ambiguous: BTreeSet<String>,
}

impl Summaries {
    /// The summary of `name`, when this unit defines it.
    pub fn get(&self, name: &str) -> Option<&Summary> {
        self.by_name.get(name)
    }

    /// Look up either a source-defined function or an external model.
    ///
    /// External models are deliberately not returned by [`Self::iter`] or
    /// counted by [`Self::len`]: they are assumptions attached to this
    /// snapshot, not functions recovered from its source.
    pub(crate) fn lookup(&self, name: &str) -> Option<&Summary> {
        self.by_name
            .get(name)
            .or_else(|| self.external_by_name.get(name))
    }

    /// The summary of one exact function in this source snapshot.
    pub fn get_by_id(&self, id: FunctionId) -> Option<&Summary> {
        self.by_id.get(&id)
    }

    /// Whether `name` has more than one recovered definition.
    pub fn is_ambiguous(&self, name: &str) -> bool {
        self.ambiguous.contains(name)
    }

    /// Every source-defined summary, in function-ID order.
    pub fn iter(&self) -> impl Iterator<Item = &Summary> {
        self.by_id.values()
    }

    /// How many functions are summarized.
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    /// Whether nothing was summarized.
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

/// Maximum function evaluations per input function, across the worklist.
///
/// A summary only ever grows --- a flow is added, never removed --- so the
/// iteration is monotone over a finite lattice and terminates on its own. This
/// bounds the pathological case rather than the normal one, and a run that hits
/// it marks every summary incomplete rather than reporting a clean answer from
/// a half-finished analysis.
const EVALUATIONS_PER_FUNCTION: usize = 16;

/// Compute a summary for every function in `flows`, to a fixed point.
///
/// `flows` is the per-function analysis [`super::analyze`] already produced;
/// this adds nothing to it and only reads.
pub fn summarize(flows: &[DataFlow]) -> Summaries {
    summarize_with_policy(flows, ExternalCallPolicy::Unknown)
}

/// Compute summaries with an explicit policy for undefined direct callees.
pub fn summarize_with_policy(flows: &[DataFlow], external_calls: ExternalCallPolicy) -> Summaries {
    let provenance: Vec<_> = flows
        .iter()
        .map(super::provenance::TraceIndex::new)
        .collect();
    let mut by_name: BTreeMap<String, Summary> = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    let mut active_intrinsics = BTreeMap::new();
    let source_names: BTreeSet<String> = flows.iter().map(|flow| flow.name.clone()).collect();
    for flow in flows {
        let parameters = parameter_count(flow).max(
            by_name
                .get(&flow.name)
                .map_or(0, |summary| summary.parameters),
        );
        if by_name.contains_key(&flow.name) {
            ambiguous.insert(flow.name.clone());
        }
        by_name.insert(
            flow.name.clone(),
            Summary {
                source_id: flow.source_id,
                function_id: Some(flow.function_id),
                analysis_revision: flow.analysis_revision,
                name: flow.name.clone(),
                parameters,
                flows: Vec::new(),
                memory_effects: direct_memory_effects(flow),
                memory_effects_complete: flow.memory_complete,
                complete: true,
            },
        );
    }
    // Name-based queries cannot choose between recovered duplicate definitions.
    // Do not combine facts from different bodies into one positive answer.
    for name in &ambiguous {
        let summary = by_name.get_mut(name).expect("inserted above");
        summary.complete = false;
        summary.function_id = None;
    }
    for (name, parameters, returns_first) in known_builtins() {
        if !by_name.contains_key(*name) {
            active_intrinsics.insert((*name).to_owned(), *parameters);
            by_name.insert(
                (*name).to_owned(),
                Summary {
                    source_id: SourceUnitId::default(),
                    function_id: None,
                    analysis_revision: crate::csource::semantic::ANALYSIS_REVISION,
                    name: (*name).to_owned(),
                    parameters: *parameters,
                    flows: returns_first
                        .then_some((0, Sink::Return))
                        .into_iter()
                        .collect(),
                    memory_effects: Vec::new(),
                    memory_effects_complete: true,
                    complete: true,
                },
            );
        }
    }
    if external_calls != ExternalCallPolicy::Unknown {
        let mut externals: BTreeMap<String, u32> = BTreeMap::new();
        for flow in flows {
            for call in &flow.calls {
                let Some(name) = &call.callee else { continue };
                if !by_name.contains_key(name) {
                    let arity = call.arguments.len() as u32;
                    externals
                        .entry(name.clone())
                        .and_modify(|known| *known = (*known).max(arity))
                        .or_insert(arity);
                }
            }
        }
        for (name, parameters) in externals {
            let (flows, complete) = match external_calls {
                ExternalCallPolicy::TaintReturn => (
                    (0..parameters).map(|index| (index, Sink::Return)).collect(),
                    false,
                ),
                ExternalCallPolicy::AssumePureNoFlow => (Vec::new(), true),
                ExternalCallPolicy::Unknown => unreachable!(),
            };
            by_name.insert(
                name.clone(),
                Summary {
                    source_id: SourceUnitId::default(),
                    function_id: None,
                    analysis_revision: crate::csource::semantic::ANALYSIS_REVISION,
                    name,
                    parameters,
                    flows,
                    memory_effects: Vec::new(),
                    memory_effects_complete: complete,
                    complete,
                },
            );
        }
    }

    let mut callers: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for (index, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            if let Some(callee) = call.callee.as_deref() {
                callers.entry(callee).or_default().insert(index);
            }
        }
    }
    let mut pending: VecDeque<usize> = (0..flows.len()).collect();
    let mut queued = vec![true; flows.len()];
    let mut evaluations = 0usize;
    while let Some(index) = pending.pop_front() {
        queued[index] = false;
        let flow = &flows[index];
        if ambiguous.contains(&flow.name) {
            continue;
        }
        evaluations += 1;
        let mut changed = false;
        let (memory_effects, memory_effects_complete) = composed_memory_effects(flow, &by_name);
        let (found, complete) = local_flows(
            flow,
            &provenance[index],
            &by_name,
            &active_intrinsics,
            memory_effects_complete,
        );
        let Some(summary) = by_name.get_mut(&flow.name) else {
            continue;
        };
        for pair in found {
            if !summary.flows.contains(&pair) {
                summary.flows.push(pair);
                changed = true;
            }
        }
        for effect in memory_effects {
            if !summary.memory_effects.contains(&effect) {
                summary.memory_effects.push(effect);
                changed = true;
            }
        }
        if summary.memory_effects_complete != memory_effects_complete {
            summary.memory_effects_complete = memory_effects_complete;
            changed = true;
        }
        if summary.complete != complete {
            summary.complete = complete;
            changed = true;
        }
        if changed {
            for &caller in callers.get(flow.name.as_str()).into_iter().flatten() {
                if !queued[caller] {
                    queued[caller] = true;
                    pending.push_back(caller);
                }
            }
        }
        if !pending.is_empty()
            && evaluations >= flows.len().saturating_mul(EVALUATIONS_PER_FUNCTION)
        {
            // Did not converge in the bound. Every summary becomes incomplete,
            // so a caller gets `Unknown` rather than a confident answer from a
            // half-finished fixed point.
            for summary in by_name.values_mut() {
                summary.complete = false;
            }
            break;
        }
    }

    for summary in by_name.values_mut() {
        summary.flows.sort_unstable();
        summary.flows.dedup();
        summary.memory_effects.sort();
        summary.memory_effects.dedup();
    }
    let mut calls = BTreeMap::new();
    let mut calls_by_id = BTreeMap::new();
    for (flow_index, flow) in flows.iter().enumerate() {
        // No direct call can consume a transfer. Avoid a full provenance
        // traversal per parameter just to iterate an empty destination set.
        if !flow.calls.iter().any(|call| call.callee.is_some()) {
            calls_by_id.insert(flow.function_id, Vec::new());
            if !ambiguous.contains(&flow.name) {
                calls.insert(flow.name.clone(), Vec::new());
            }
            continue;
        }
        let mut transfers = Vec::new();
        for (position, (definition_index, _parameter)) in flow
            .definitions
            .iter()
            .enumerate()
            .filter(|(_, definition)| definition.kind == super::DefKind::Parameter)
            .enumerate()
        {
            let uses =
                super::provenance::uses(flow, &provenance[flow_index], definition_index, &by_name);
            for call in &flow.calls {
                let Some(callee) = &call.callee else { continue };
                for (argument, span) in call.argument_spans.iter().enumerate() {
                    if super::provenance::expression(flow, &uses, *span, &by_name) {
                        transfers.push((position as u32, callee.clone(), argument as u32));
                    }
                }
            }
        }
        transfers.sort();
        transfers.dedup();
        calls_by_id.insert(flow.function_id, transfers.clone());
        if !ambiguous.contains(&flow.name) {
            calls.insert(flow.name.clone(), transfers);
        }
    }
    let mut by_id = BTreeMap::new();
    for (index, flow) in flows.iter().enumerate() {
        let mut summary = if !ambiguous.contains(&flow.name) {
            by_name.get(&flow.name).cloned().unwrap_or_default()
        } else {
            let (memory_effects, memory_effects_complete) = composed_memory_effects(flow, &by_name);
            let (found, complete) = local_flows(
                flow,
                &provenance[index],
                &by_name,
                &active_intrinsics,
                memory_effects_complete,
            );
            Summary {
                source_id: flow.source_id,
                function_id: Some(flow.function_id),
                analysis_revision: flow.analysis_revision,
                name: flow.name.clone(),
                parameters: parameter_count(flow),
                flows: found,
                memory_effects,
                memory_effects_complete,
                complete,
            }
        };
        summary.function_id = Some(flow.function_id);
        summary.flows.sort_unstable();
        summary.flows.dedup();
        by_id.insert(flow.function_id, summary);
    }
    // Intrinsic and external summaries are call-site knowledge, not functions
    // recovered from this source. Keep external models privately so queries
    // apply the same policy as summary construction without exposing invented
    // source functions through `iter` or `get`.
    let external_by_name = by_name
        .iter()
        .filter(|(name, _)| !source_names.contains(*name) && !active_intrinsics.contains_key(*name))
        .map(|(name, summary)| (name.clone(), summary.clone()))
        .collect();
    by_name.retain(|name, _| source_names.contains(name));
    Summaries {
        by_name,
        by_id,
        external_by_name,
        calls,
        calls_by_id,
        ambiguous,
    }
}

fn known_builtins() -> &'static [(&'static str, u32, bool)] {
    &[
        ("__builtin_constant_p", 1, false),
        ("__builtin_expect", 2, true),
        ("__builtin_expect_with_probability", 3, true),
        ("__builtin_bswap16", 1, true),
        ("__builtin_bswap32", 1, true),
        ("__builtin_bswap64", 1, true),
        ("__builtin_bswap128", 1, true),
        ("__builtin_popcount", 1, true),
        ("__builtin_popcountl", 1, true),
        ("__builtin_popcountll", 1, true),
        ("__builtin_parity", 1, true),
        ("__builtin_parityl", 1, true),
        ("__builtin_parityll", 1, true),
        ("__builtin_clz", 1, true),
        ("__builtin_clzl", 1, true),
        ("__builtin_clzll", 1, true),
        ("__builtin_ctz", 1, true),
        ("__builtin_ctzl", 1, true),
        ("__builtin_ctzll", 1, true),
        ("__builtin_ffs", 1, true),
        ("__builtin_ffsl", 1, true),
        ("__builtin_ffsll", 1, true),
    ]
}

/// How many parameters `flow`'s function declares.
fn parameter_count(flow: &DataFlow) -> u32 {
    flow.definitions
        .iter()
        .filter(|definition| definition.kind == super::DefKind::Parameter)
        .count() as u32
}

/// One round of a function's own flows, given the summaries so far.
///
/// Returns the pairs found and whether the body was fully resolvable.
///
/// The call-site rule is what makes this interprocedural. A parameter reaching
/// an argument of `g(...)` propagates **only as far as `g`'s own summary says
/// it does**: to this function's return if `g` returns it and the result is
/// returned here, and nowhere if `g` drops it. An earlier version had no
/// call-site rule at all and reported a flow whenever a parameter was read
/// anywhere before a return, which made `int b(int y) { return a(y); }` flow
/// even when `a` returns a constant --- an answer that is right by accident and
/// wrong in general.
fn local_flows(
    flow: &DataFlow,
    provenance: &super::provenance::TraceIndex,
    known: &BTreeMap<String, Summary>,
    active_intrinsics: &BTreeMap<String, u32>,
    memory_effects_complete: bool,
) -> (Vec<(u32, Sink)>, bool) {
    let mut found: Vec<(u32, Sink)> = Vec::new();
    // Unresolved bindings include globals. Their local edges remain useful,
    // but this summary model has no global input/output slots to transfer
    // effects between callees and callers. Do not certify their absence.
    let mut complete = flow.effects_complete
        && memory_effects_complete
        && flow.vla_complete
        && flow.control_targets_complete
        && flow.recovery_free
        && flow.unresolved_bindings.is_empty();

    let call_sites = flow.call_sites();
    for site in &call_sites {
        if site.callee.is_none() {
            // An indirect call: no name, so no summary can be applied.
            complete = false;
        } else if let Some(name) = &site.callee {
            let wrong_intrinsic_arity = active_intrinsics
                .get(name)
                .is_some_and(|expected| site.arguments.len() as u32 != *expected);
            if wrong_intrinsic_arity || !known.get(name).is_some_and(|summary| summary.complete) {
                // Missing and incomplete callees both leave unknown effects.
                // Completeness must propagate through recursion and call chains,
                // not just one step beyond the missing definition.
                complete = false;
            }
        }
    }

    for (index, (definition_index, parameter)) in flow
        .definitions
        .iter()
        .enumerate()
        .filter(|(_, definition)| definition.kind == super::DefKind::Parameter)
        .enumerate()
    {
        let index = index as u32;

        // Direct: the parameter itself, or a local carrying it, is returned.
        if returns_binding(flow, provenance, definition_index, &call_sites, known)
            || incoming_parameter_memory_reaches_return(flow, parameter.binding)
        {
            found.push((index, Sink::Return));
        }

        // Address-taking here refers to callee-local parameter storage, not
        // the caller's pointee. It cannot justify a Parameter output flow.
        // Real pointee effects require a separate interprocedural memory model.
    }

    found.sort_unstable();
    found.dedup();
    (found, complete)
}

fn incoming_parameter_memory_reaches_return(flow: &DataFlow, binding: Binding) -> bool {
    flow.memory_edges.iter().any(|edge| {
        let Some(definition) = flow.memory_definitions.get(edge.definition as usize) else {
            return false;
        };
        if definition.kind != super::MemoryDefinitionKind::IncomingParameter
            || flow.memory_region_root_binding(definition.region) != Some(binding)
        {
            return false;
        }
        let Some(use_) = flow.memory_uses.get(edge.use_ as usize) else {
            return false;
        };
        flow.return_spans
            .iter()
            .any(|span| span.lo <= use_.span.lo && use_.span.hi <= span.hi)
    })
}

/// Whether a value in `binding` reaches a `return` in this function.
///
/// Two ways it can: read directly by a return, or passed to a call whose
/// summary says that argument reaches the callee's return, where the call's
/// result is itself returned.
fn returns_binding(
    flow: &DataFlow,
    provenance: &super::provenance::TraceIndex,
    parameter_definition: usize,
    _call_sites: &[CallSite],
    known: &BTreeMap<String, Summary>,
) -> bool {
    super::provenance::returns(flow, provenance, parameter_definition, known)
}

/// One call in a function body, reduced to what a summary needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The callee's name, or `None` for an indirect call.
    pub callee: Option<String>,
    /// The binding passed at each argument position, in order.
    ///
    /// A position holding an expression rather than a bare name contributes
    /// [`Binding::FREE`], which no real binding equals, so it neither
    /// propagates nor blocks.
    pub arguments: Vec<Binding>,
    /// Whether the call's result is itself returned.
    pub result_is_returned: bool,
}

/// Whether a value in `source`'s parameter `index` can reach `sink`.
///
/// The query a code property graph is used for, answered across calls. Three
/// valued: `Unknown` when the search met an indirect call, a callee this unit
/// does not define, or a bound --- see the module docs on why that is a
/// separate answer from `No`.
pub fn reaches(summaries: &Summaries, source: &str, index: u32, sink: &str) -> Flow {
    reaches_detailed(summaries, source, index, sink).verdict
}

/// Structured form of [`reaches`] with path and uncertainty evidence.
pub fn reaches_detailed(
    summaries: &Summaries,
    source: &str,
    index: u32,
    sink: &str,
) -> Reachability {
    let result = |verdict, path, explored, uncertainty: BTreeSet<_>| Reachability {
        verdict,
        path,
        explored,
        uncertainty: uncertainty.into_iter().collect(),
    };
    // A name alone cannot select a parameter identity among duplicate bodies,
    // even for the zero-length source == sink path.
    if summaries.ambiguous.contains(source) {
        return result(
            Flow::Unknown,
            Vec::new(),
            Vec::new(),
            [ReachabilityUncertainty::AmbiguousSource].into(),
        );
    }
    let Some(start) = summaries.get(source) else {
        return result(
            Flow::Unknown,
            Vec::new(),
            Vec::new(),
            [ReachabilityUncertainty::MissingSource].into(),
        );
    };
    if index >= start.parameters {
        // Recovery or duplicate definitions may have lost parameters. An
        // incomplete summary's recovered arity cannot prove their absence.
        // Until signature certainty is tracked separately, conservatively
        // apply this to all incomplete summaries.
        return if start.complete {
            result(Flow::No, Vec::new(), Vec::new(), BTreeSet::new())
        } else {
            result(
                Flow::Unknown,
                Vec::new(),
                Vec::new(),
                [ReachabilityUncertainty::IncompleteArity].into(),
            )
        };
    }
    let mut seen = BTreeSet::new();
    let first = ReachabilityStep {
        function_id: start.function_id,
        function: source.to_string(),
        parameter: index,
    };
    let mut pending = vec![(first.clone(), vec![first])];
    let mut uncertainty = BTreeSet::new();
    while let Some((step, path)) = pending.pop() {
        let name = &step.function;
        let position = step.parameter;
        if !seen.insert((name.clone(), position)) {
            continue;
        }
        if name == sink {
            let explored = seen
                .iter()
                .map(|(function, parameter)| ReachabilityStep {
                    function_id: summaries
                        .lookup(function)
                        .and_then(|summary| summary.function_id),
                    function: function.clone(),
                    parameter: *parameter,
                })
                .collect();
            return result(Flow::Yes, path, explored, BTreeSet::new());
        }
        let Some(summary) = summaries.lookup(name) else {
            uncertainty.insert(ReachabilityUncertainty::MissingCallee);
            continue;
        };
        if !summary.complete {
            uncertainty.insert(ReachabilityUncertainty::IncompleteSummary);
        }
        if let Some(transfers) = summaries.calls.get(name) {
            for (parameter, callee, argument) in transfers {
                if *parameter == position {
                    let next = ReachabilityStep {
                        function_id: summaries
                            .lookup(callee)
                            .and_then(|summary| summary.function_id),
                        function: callee.clone(),
                        parameter: *argument,
                    };
                    let mut next_path = path.clone();
                    next_path.push(next.clone());
                    pending.push((next, next_path));
                }
            }
        }
    }
    let explored = seen
        .into_iter()
        .map(|(function, parameter)| ReachabilityStep {
            function_id: summaries
                .lookup(&function)
                .and_then(|summary| summary.function_id),
            function,
            parameter,
        })
        .collect();
    if uncertainty.is_empty() {
        result(Flow::No, Vec::new(), explored, uncertainty)
    } else {
        result(Flow::Unknown, Vec::new(), explored, uncertainty)
    }
}

/// Identity-first reachability query that never selects a duplicate by name.
pub fn reaches_by_id_detailed(
    summaries: &Summaries,
    source: FunctionId,
    index: u32,
    sink: FunctionId,
) -> Reachability {
    let result = |verdict, path, explored, uncertainty: BTreeSet<_>| Reachability {
        verdict,
        path,
        explored,
        uncertainty: uncertainty.into_iter().collect(),
    };
    let Some(start) = summaries.get_by_id(source) else {
        return result(
            Flow::Unknown,
            Vec::new(),
            Vec::new(),
            [ReachabilityUncertainty::MissingSource].into(),
        );
    };
    if summaries.get_by_id(sink).is_none() {
        return result(
            Flow::Unknown,
            Vec::new(),
            Vec::new(),
            [ReachabilityUncertainty::MissingSink].into(),
        );
    }
    if index >= start.parameters {
        return if start.complete {
            result(Flow::No, Vec::new(), Vec::new(), BTreeSet::new())
        } else {
            result(
                Flow::Unknown,
                Vec::new(),
                Vec::new(),
                [ReachabilityUncertainty::IncompleteArity].into(),
            )
        };
    }

    let first = ReachabilityStep {
        function_id: Some(source),
        function: start.name.clone(),
        parameter: index,
    };
    let mut pending = vec![(source, index, vec![first])];
    let mut seen = BTreeSet::new();
    let mut uncertainty = BTreeSet::new();
    while let Some((function_id, parameter, path)) = pending.pop() {
        if !seen.insert((function_id, parameter)) {
            continue;
        }
        if function_id == sink {
            let explored = seen
                .iter()
                .filter_map(|(id, parameter)| {
                    Some(ReachabilityStep {
                        function_id: Some(*id),
                        function: summaries.get_by_id(*id)?.name.clone(),
                        parameter: *parameter,
                    })
                })
                .collect();
            return result(Flow::Yes, path, explored, BTreeSet::new());
        }
        let Some(summary) = summaries.get_by_id(function_id) else {
            uncertainty.insert(ReachabilityUncertainty::MissingCallee);
            continue;
        };
        if !summary.complete {
            uncertainty.insert(ReachabilityUncertainty::IncompleteSummary);
        }
        for (formal, callee, argument) in summaries
            .calls_by_id
            .get(&function_id)
            .into_iter()
            .flatten()
        {
            if *formal != parameter {
                continue;
            }
            if summaries.is_ambiguous(callee) {
                uncertainty.insert(ReachabilityUncertainty::AmbiguousCallee);
                continue;
            }
            let Some(callee_summary) = summaries.lookup(callee) else {
                uncertainty.insert(ReachabilityUncertainty::MissingCallee);
                continue;
            };
            let Some(callee_id) = callee_summary.function_id else {
                // A complete external model has no body to traverse and no
                // caller-to-source-function path. An incomplete model cannot
                // justify that negative conclusion.
                if !callee_summary.complete {
                    uncertainty.insert(ReachabilityUncertainty::IncompleteSummary);
                }
                continue;
            };
            let step = ReachabilityStep {
                function_id: Some(callee_id),
                function: callee.clone(),
                parameter: *argument,
            };
            let mut next_path = path.clone();
            next_path.push(step);
            pending.push((callee_id, *argument, next_path));
        }
    }

    let explored = seen
        .into_iter()
        .filter_map(|(id, parameter)| {
            Some(ReachabilityStep {
                function_id: Some(id),
                function: summaries.get_by_id(id)?.name.clone(),
                parameter,
            })
        })
        .collect();
    if uncertainty.is_empty() {
        result(Flow::No, Vec::new(), explored, uncertainty)
    } else {
        result(Flow::Unknown, Vec::new(), explored, uncertainty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::dataflow::analyze;

    fn summaries(text: &str) -> Summaries {
        summarize(&analyze(text).into_parts().0)
    }

    #[test]
    fn every_function_gets_a_summary() {
        let s = summaries("int a(int x) { return x; }\nint b(int y) { return a(y); }");
        assert_eq!(s.len(), 2);
        assert!(s.get("a").is_some() && s.get("b").is_some());
    }

    #[test]
    fn duplicate_names_keep_separate_function_id_summaries() {
        let s = summaries("int f(void){return 0;} int f(int x){return x;}");
        assert_eq!(s.len(), 2);
        let first = s.get_by_id(FunctionId(0)).expect("first f");
        let second = s.get_by_id(FunctionId(1)).expect("second f");
        assert_eq!(first.function_id, Some(FunctionId(0)));
        assert_eq!(first.parameters, 0);
        assert!(first.flows.is_empty());
        assert_eq!(second.function_id, Some(FunctionId(1)));
        assert_eq!(second.parameters, 1);
        assert!(second.flows_to(0, Sink::Return));
        assert!(s.is_ambiguous("f"));
        assert_eq!(s.get("f").expect("compatibility summary").function_id, None);
    }

    #[test]
    fn identity_first_query_distinguishes_duplicate_source_bodies() {
        let s =
            summaries("int f(int x){return g(x);} int f(int x){return 0;} int g(int y){return y;}");
        let first = reaches_by_id_detailed(&s, FunctionId(0), 0, FunctionId(2));
        let second = reaches_by_id_detailed(&s, FunctionId(1), 0, FunctionId(2));
        assert_eq!(first.verdict, Flow::Yes);
        assert_eq!(second.verdict, Flow::No);
        assert_eq!(first.path[0].function_id, Some(FunctionId(0)));
        assert_eq!(first.path[1].function_id, Some(FunctionId(2)));
    }

    #[test]
    fn a_summary_records_the_parameter_count() {
        let s = summaries("int f(int a, int b, int c) { return a; }");
        assert_eq!(s.get("f").expect("f").parameters, 3);
    }

    #[test]
    fn a_parameter_returned_directly_flows_to_the_return() {
        let s = summaries("int f(int a, int b) { return a; }");
        let f = s.get("f").expect("f");
        assert!(f.flows_to(0, Sink::Return), "{:?}", f.flows);
    }

    #[test]
    fn a_parameter_returned_through_a_local_still_flows() {
        let s = summaries("int f(int a) { int t = a; return t; }");
        let f = s.get("f").expect("f");
        assert!(f.flows_to(0, Sink::Return), "{:?}", f.flows);
    }

    #[test]
    fn recursion_terminates() {
        // The fixed point must converge rather than descend forever.
        let s = summaries("int f(int n) { if (n > 0) { return f(n - 1); } return n; }");
        assert!(s.get("f").is_some());
    }

    #[test]
    fn mutual_recursion_terminates() {
        let s = summaries(
            "int a(int n) { return b(n); }\nint b(int n) { if (n > 0) { return a(n - 1); } return n; }",
        );
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn summarizing_is_total_on_input_that_is_not_c() {
        for junk in ["", "\u{0}\u{1}", "int f(", "}}}"] {
            let _ = summaries(junk);
        }
    }

    #[test]
    fn summaries_are_deterministic() {
        let text = "int a(int x) { return x; }\nint b(int y) { return a(y) + y; }";
        assert_eq!(summaries(text), summaries(text));
    }

    #[test]
    fn a_flow_verdict_combines_conservatively() {
        assert_eq!(Flow::Yes.or(Flow::No), Flow::Yes);
        assert_eq!(Flow::No.or(Flow::Unknown), Flow::Unknown);
        assert_eq!(Flow::No.or(Flow::No), Flow::No);
        // Unknown must never weaken to No: a search that could not see
        // everywhere has not proven absence.
        assert_eq!(Flow::Unknown.or(Flow::No), Flow::Unknown);
    }

    #[test]
    fn a_detailed_positive_names_the_interprocedural_path() {
        let s = summaries("int sink(int z){return z;} int f(int x){return sink(x);}");
        let answer = reaches_detailed(&s, "f", 0, "sink");
        assert_eq!(answer.verdict, Flow::Yes);
        assert_eq!(
            answer.path,
            vec![
                ReachabilityStep {
                    function_id: Some(FunctionId(1)),
                    function: "f".into(),
                    parameter: 0,
                },
                ReachabilityStep {
                    function_id: Some(FunctionId(0)),
                    function: "sink".into(),
                    parameter: 0,
                },
            ]
        );
        assert!(answer.uncertainty.is_empty());
    }

    #[test]
    fn a_detailed_unknown_explains_the_missing_callee() {
        let s = summaries("int f(int x){external(x);return 0;}");
        let answer = reaches_detailed(&s, "f", 0, "sink");
        assert_eq!(answer.verdict, Flow::Unknown);
        assert!(answer
            .uncertainty
            .contains(&ReachabilityUncertainty::IncompleteSummary));
        assert!(answer
            .uncertainty
            .contains(&ReachabilityUncertainty::MissingCallee));
        assert!(answer.path.is_empty());
        assert!(!answer.explored.is_empty());
    }
}

#[cfg(test)]
mod negative_tests {
    use super::*;
    use crate::csource::dataflow::analyze;

    fn summaries(text: &str) -> Summaries {
        summarize(&analyze(text).into_parts().0)
    }

    #[test]
    fn a_parameter_that_is_not_returned_does_not_flow_to_the_return() {
        // The test that separates an analysis from a rubber stamp: `b` is
        // never read on any path to the return, so claiming it flows there
        // would make every summary useless.
        let s = summaries("int f(int a, int b) { return a; }");
        let f = s.get("f").expect("f");
        assert!(f.flows_to(0, Sink::Return), "a should flow: {:?}", f.flows);
        assert!(
            !f.flows_to(1, Sink::Return),
            "b must not flow: {:?}",
            f.flows
        );
    }

    #[test]
    fn an_unused_parameter_flows_nowhere() {
        let s = summaries("int f(int a, int unused) { return a + 1; }");
        let f = s.get("f").expect("f");
        assert!(!f.flows_to(1, Sink::Return), "{:?}", f.flows);
    }

    #[test]
    fn a_constant_return_carries_no_parameter() {
        let s = summaries("int f(int a, int b) { return 0; }");
        let f = s.get("f").expect("f");
        assert!(
            f.flows.is_empty(),
            "nothing flows to a constant: {:?}",
            f.flows
        );
    }
}

#[cfg(test)]
mod cross_call_tests {
    use super::*;
    use crate::csource::dataflow::analyze;

    fn summaries(text: &str) -> Summaries {
        summarize(&analyze(text).into_parts().0)
    }

    #[test]
    fn a_value_returned_through_a_callee_flows_to_the_caller_return() {
        // The whole point of the module: `b`'s parameter reaches its return
        // only because `a` returns what it is given.
        let s = summaries("int a(int x) { return x; }\nint b(int y) { return a(y); }");
        let b = s.get("b").expect("b");
        assert!(
            b.flows_to(0, Sink::Return),
            "y should reach b's return through a: {:?}",
            b.flows
        );
    }

    #[test]
    fn a_callee_that_drops_its_argument_does_not_propagate() {
        let s = summaries("int a(int x) { return 0; }\nint b(int y) { return a(y); }");
        let b = s.get("b").expect("b");
        assert!(
            !b.flows_to(0, Sink::Return),
            "a returns a constant, so nothing should flow: {:?}",
            b.flows
        );
    }

    #[test]
    fn external_call_policy_is_explicit_and_transitive() {
        let analyzed = analyze(concat!(
            "extern int external(int);",
            "int f(int x) { return external(x); }",
            "int g(int y) { return f(y); }",
        ))
        .into_parts()
        .0;

        let unknown = summarize_with_policy(&analyzed, ExternalCallPolicy::Unknown);
        for name in ["f", "g"] {
            let summary = unknown.get(name).expect(name);
            assert!(!summary.complete, "{name}: {summary:?}");
            assert!(!summary.flows_to(0, Sink::Return), "{name}: {summary:?}");
        }

        let tainted = summarize_with_policy(&analyzed, ExternalCallPolicy::TaintReturn);
        for name in ["f", "g"] {
            let summary = tainted.get(name).expect(name);
            assert!(!summary.complete, "{name}: {summary:?}");
            assert!(summary.flows_to(0, Sink::Return), "{name}: {summary:?}");
        }

        let pure = summarize_with_policy(&analyzed, ExternalCallPolicy::AssumePureNoFlow);
        for name in ["f", "g"] {
            let summary = pure.get(name).expect(name);
            assert!(summary.complete, "{name}: {summary:?}");
            assert!(!summary.flows_to(0, Sink::Return), "{name}: {summary:?}");
        }
        assert!(
            pure.get("external").is_none(),
            "models are not source bodies"
        );
    }

    #[test]
    fn external_policy_also_qualifies_reachability_queries() {
        let analyzed = analyze(concat!(
            "extern int external(int);",
            "int f(int x) { external(x); return 0; }",
            "int sink(int z) { return z; }",
        ))
        .into_parts()
        .0;

        let unknown = summarize_with_policy(&analyzed, ExternalCallPolicy::Unknown);
        assert_eq!(
            reaches_by_id_detailed(&unknown, FunctionId(0), 0, FunctionId(1)).verdict,
            Flow::Unknown
        );

        let tainted = summarize_with_policy(&analyzed, ExternalCallPolicy::TaintReturn);
        assert_eq!(
            reaches_by_id_detailed(&tainted, FunctionId(0), 0, FunctionId(1)).verdict,
            Flow::Unknown
        );

        let pure = summarize_with_policy(&analyzed, ExternalCallPolicy::AssumePureNoFlow);
        assert_eq!(
            reaches_by_id_detailed(&pure, FunctionId(0), 0, FunctionId(1)).verdict,
            Flow::No
        );
    }

    #[test]
    fn pure_value_builtins_have_narrow_intrinsic_summaries() {
        for expression in [
            "__builtin_expect(x, 1)",
            "__builtin_expect_with_probability(x, 1, 0.9)",
            "__builtin_bswap32(x)",
            "__builtin_popcount(x)",
            "__builtin_clz(x)",
        ] {
            let s = summaries(&format!("int f(unsigned x) {{ return {expression}; }}"));
            assert_eq!(s.len(), 1, "intrinsics are not public functions");
            let f = s.get("f").expect("f");
            assert!(f.complete, "{expression}: {f:?}");
            assert!(f.flows_to(0, Sink::Return), "{expression}: {f:?}");
        }

        for expression in [
            "__builtin_not_a_real_intrinsic(x)",
            "__builtin_add_overflow(x, 1, &out)",
            "__builtin_expect(x)",
            "__builtin_expect(x, 1, 2)",
        ] {
            let source = format!("int f(unsigned x) {{ unsigned out = 0; return {expression}; }}");
            assert!(!summaries(&source).get("f").expect("f").complete);
        }

        let overridden = summaries(concat!(
            "int __builtin_expect(int x, int expected) { return 0; }",
            "int f(int x) { return __builtin_expect(x, 1); }",
        ));
        let f = overridden.get("f").expect("f");
        assert!(f.complete, "{f:?}");
        assert!(!f.flows_to(0, Sink::Return), "source body wins: {f:?}");

        let constant_override = summaries(concat!(
            "int __builtin_constant_p(int x) { return x; }",
            "int f(int x) { return __builtin_constant_p(x); }",
        ));
        let f = constant_override.get("f").expect("f");
        assert!(f.complete, "{f:?}");
        assert!(f.flows_to(0, Sink::Return), "source body wins: {f:?}");
    }
}

#[cfg(test)]
mod corpus_tests {
    use super::*;
    use crate::csource::dataflow::analyze;

    /// Summarizing 900 real functions must terminate, agree with itself, and
    /// keep every invariant the type promises.
    #[test]
    fn the_fixture_corpus_summarizes_consistently() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/decompiler_fixtures/src");
        let mut functions = 0usize;
        let mut summarized = 0usize;
        let mut flows = 0usize;
        let mut incomplete = 0usize;

        for (path, text) in crate::test_corpus::sources(&root) {
            let analyzed = analyze(&text).into_parts().0;
            functions += analyzed.len();
            let summaries = summarize(&analyzed);
            summarized += summaries.len();
            for summary in summaries.iter() {
                flows += summary.flows.len();
                if !summary.complete {
                    incomplete += 1;
                }
                // Every flow names a parameter this function actually has.
                for (index, sink) in &summary.flows {
                    assert!(
                        *index < summary.parameters,
                        "{}: flow from parameter {index} of {}",
                        summary.name,
                        summary.parameters
                    );
                    if let Sink::Parameter(other) = sink {
                        assert!(
                            *other < summary.parameters,
                            "{}: flow to parameter {other}",
                            summary.name
                        );
                    }
                }
            }
            // Determinism, on real input rather than a two-line fixture.
            assert_eq!(summaries, summarize(&analyzed), "{}", path.display());
        }

        assert!(functions > 500, "only {functions} functions");
        assert_eq!(summarized, functions, "a function lost its summary");
        eprintln!(
            "corpus summaries: {summarized} functions, {flows} parameter flows, \
             {incomplete} incomplete"
        );
    }
}
