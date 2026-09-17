//! Ownership and identity for one immutable C analysis snapshot.
//!
//! This is the first semantic-foundation boundary. It prevents consumers from
//! pairing a tree, spans, diagnostics, or CFGs produced from different source
//! buffers and gives derived results a stable snapshot identity.

use std::sync::OnceLock;

use crate::csource::cfg::{function_cfgs, FunctionCfg};
use crate::csource::dataflow::{summarize_with_policy, DataFlow, ExternalCallPolicy, Summaries};
use crate::csource::eval::EvaluationPlan;
use crate::csource::facts::{
    resolve_facts, ExternalFacts, FactSource, FunctionFacts, FunctionShape, ParameterShape,
};
use crate::csource::normalize::Dialect;
use crate::csource::parse::{parse, Tree};
use crate::syntax::diag::{Diagnostic, Diagnostics};
use crate::syntax::ids::Span;

pub(crate) mod declarations;
pub(crate) mod expr_types;
pub(crate) mod types;

use declarations::{resolve_function, FunctionResolution, TranslationUnitSymbols};
use types::{resolve_types, FunctionTypes};

/// Revision of the semantic result contract.
///
/// Increment when IDs or the meaning of the semantic result changes. This is
/// independent of the package version so cached analysis can reject an
/// incompatible result during development.
pub const ANALYSIS_REVISION: u32 = 5;

/// Source preparation applied before parsing an analysis snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputDialect {
    /// Parse the supplied text exactly as provided.
    #[default]
    Ordinary,
    /// Strip compiler system-header regions from a preprocessed translation unit.
    Preprocessed,
    /// Normalize supported decompiler-emitted C spellings.
    Decompiled,
}

impl InputDialect {
    /// Stable spelling used by Python and serialized result metadata.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Ordinary => "ordinary",
            Self::Preprocessed => "preprocessed",
            Self::Decompiled => "decompiled",
        }
    }

    fn prepare(self, source: &str) -> String {
        match self {
            Self::Ordinary => source.to_owned(),
            Self::Preprocessed => Dialect::Preprocessed(source).normalize(),
            Self::Decompiled => Dialect::Decompiled(source).normalize(),
        }
    }
}

/// Explicit policies used to construct one immutable analysis snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AnalysisOptions {
    /// Input normalization policy. It changes source bytes and all coordinates.
    pub dialect: InputDialect,
    /// Model for named direct callees whose body is outside this snapshot.
    pub external_calls: ExternalCallPolicy,
}

/// Identity of the exact source bytes used by an [`AnalysisUnit`].
///
/// This is a deterministic mismatch guard, not a cryptographic digest. The
/// owning unit still retains the source itself; callers must not use this as a
/// security boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceUnitId {
    /// Source length in bytes.
    pub len: u64,
    /// Two independently seeded byte hashes.
    pub hash: [u64; 2],
}

impl SourceUnitId {
    /// Derive an identity from exact UTF-8 source bytes.
    pub fn of(source: &str) -> Self {
        const OFFSET_A: u64 = 0xcbf2_9ce4_8422_2325;
        const OFFSET_B: u64 = 0x8422_2325_cbf2_9ce4;
        const PRIME_A: u64 = 0x0000_0100_0000_01b3;
        const PRIME_B: u64 = 0x0000_0100_0000_01e7;
        let mut hash = [OFFSET_A, OFFSET_B];
        for byte in source.as_bytes() {
            hash[0] = (hash[0] ^ u64::from(*byte)).wrapping_mul(PRIME_A);
            hash[1] = (hash[1] ^ u64::from(*byte)).wrapping_mul(PRIME_B);
        }
        Self {
            len: source.len() as u64,
            hash,
        }
    }

    /// Stable lowercase text for serialized APIs.
    pub fn name(self) -> String {
        format!("{:016x}{:016x}-{}", self.hash[0], self.hash[1], self.len)
    }
}

/// Dense function identity within one immutable analysis snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FunctionId(pub u32);

impl FunctionId {
    /// Sentinel used only by the low-level single-function compatibility API.
    pub const UNKNOWN: Self = Self(u32::MAX);

    /// Whether this ID lacks an owning unit/function table.
    pub const fn is_unknown(self) -> bool {
        self.0 == u32::MAX
    }
}

/// Parsed and graph-ready state for one exact source snapshot.
///
/// Higher semantic products are initialized on first use so syntax and CFG
/// consumers do not pay for evaluation or dataflow they never request.
#[derive(Debug, Clone)]
pub struct AnalysisUnit {
    options: AnalysisOptions,
    source_id: SourceUnitId,
    source: String,
    tree: Tree,
    token_spans: Vec<Span>,
    symbols: TranslationUnitSymbols,
    functions: Vec<FunctionCfg>,
    resolutions: Vec<FunctionResolution>,
    types: Vec<FunctionTypes>,
    facts: Vec<FunctionFacts>,
    refused_api_facts: Vec<Diagnostic>,
    evaluations: OnceLock<Vec<EvaluationPlan>>,
    diagnostics: Diagnostics,
    dataflows: OnceLock<Vec<DataFlow>>,
    summaries: OnceLock<Summaries>,
}

impl AnalysisUnit {
    /// Parse `source` once and build its executable function CFGs once.
    pub fn new(source: impl Into<String>) -> Self {
        Self::with_options(source, AnalysisOptions::default())
    }

    /// Prepare and analyze `source` using explicit snapshot policies.
    pub fn with_options(source: impl Into<String>, options: AnalysisOptions) -> Self {
        Self::with_facts(source, options, &ExternalFacts::default())
    }

    /// [`AnalysisUnit::with_options`] plus the external facts a caller
    /// attaches through the API.
    ///
    /// Facts written as comments in `source` are read on every construction;
    /// `facts` is the second front door for a caller that has them from
    /// elsewhere. Both are resolved against each function's parameter list
    /// here, and every fact that cannot be attached is a diagnostic on the
    /// unit (see [`crate::csource::facts`]). The API wins over a comment for
    /// the same key.
    pub fn with_facts(
        source: impl Into<String>,
        options: AnalysisOptions,
        facts: &ExternalFacts,
    ) -> Self {
        let source = options.dialect.prepare(&source.into());
        let source_id = SourceUnitId::of(&source);
        let (tree, mut diagnostics) = parse(&source).into_parts();
        let token_spans = tree.token_spans(&source);
        let symbols = TranslationUnitSymbols::collect(&tree, &source, &token_spans);
        let (functions, cfg_diagnostics) = function_cfgs(&tree, &source).into_parts();
        let resolutions = functions
            .iter()
            .map(|function| {
                resolve_function(
                    &tree,
                    &source,
                    &token_spans,
                    function.node,
                    function.span,
                    function.name_span,
                    &symbols,
                )
            })
            .collect();
        let types = functions
            .iter()
            .zip(&resolutions)
            .map(|(function, resolution)| {
                resolve_types(
                    &tree,
                    &source,
                    &token_spans,
                    function.node,
                    resolution,
                    &symbols,
                    function.span.lo,
                )
            })
            .collect();
        for diagnostic in cfg_diagnostics.iter() {
            diagnostics.push(diagnostic.clone());
        }
        let shapes: Vec<FunctionShape> = functions
            .iter()
            .zip(&resolutions)
            .zip(&types)
            .map(|((function, resolution), function_types)| {
                let typer = expr_types::ExpressionTyper {
                    tree: &tree,
                    text: &source,
                    token_spans: &token_spans,
                    resolution,
                    types: function_types,
                    symbols: &symbols,
                };
                let parameters = typer
                    .parameters(function.node, function.span.lo)
                    .into_iter()
                    .filter_map(|(node, parameter)| {
                        Some(ParameterShape {
                            name: parameter.name?,
                            declaration: parameter.declaration?,
                            node,
                            pointer: (!parameter.adjusted.is_unknown())
                                .then(|| parameter.adjusted.pointer_depth() > 0),
                        })
                    })
                    .collect();
                FunctionShape {
                    name: function.name.clone(),
                    node: function.node,
                    name_span: function.name_span,
                    parameters,
                }
            })
            .collect();
        let resolution = resolve_facts(
            &source,
            &tree,
            &token_spans,
            &shapes,
            facts,
            &mut diagnostics,
        );
        let refused_api_facts = resolution
            .refused
            .iter()
            .filter(|(source, _)| *source == FactSource::Api)
            .map(|(_, diagnostic)| diagnostic.clone())
            .collect();
        let facts = resolution.functions;
        Self {
            options,
            source_id,
            source,
            tree,
            token_spans,
            symbols,
            functions,
            resolutions,
            types,
            facts,
            refused_api_facts,
            evaluations: OnceLock::new(),
            diagnostics,
            dataflows: OnceLock::new(),
            summaries: OnceLock::new(),
        }
    }

    /// Policies that produced this snapshot.
    pub const fn options(&self) -> AnalysisOptions {
        self.options
    }

    /// Identity of the exact owned source bytes.
    pub const fn source_id(&self) -> SourceUnitId {
        self.source_id
    }

    /// Exact source text whose coordinates every owned object uses.
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Tolerant syntax tree for this snapshot.
    pub const fn tree(&self) -> &Tree {
        &self.tree
    }

    /// Exact token spans for this snapshot.
    pub fn token_spans(&self) -> &[Span] {
        &self.token_spans
    }

    /// Shared translation-unit declaration index for semantic consumers.
    pub(crate) const fn symbols(&self) -> &TranslationUnitSymbols {
        &self.symbols
    }

    /// Executable function CFGs in source order.
    pub fn functions(&self) -> &[FunctionCfg] {
        &self.functions
    }

    /// Lexical declaration resolution aligned with [`AnalysisUnit::functions`].
    pub(crate) fn resolutions(&self) -> &[FunctionResolution] {
        &self.resolutions
    }

    /// Structural declared types aligned with [`AnalysisUnit::functions`].
    pub(crate) fn types(&self) -> &[FunctionTypes] {
        &self.types
    }

    /// External facts aligned with [`AnalysisUnit::functions`]: what the
    /// comments above each function and the caller's [`ExternalFacts`] said
    /// about it and could be attached. Every fact that could not is in
    /// [`AnalysisUnit::diagnostics`].
    pub fn facts(&self) -> &[FunctionFacts] {
        &self.facts
    }

    /// The diagnostics for API-supplied facts that could not be attached ---
    /// a subset of [`AnalysisUnit::diagnostics`], kept apart because a caller
    /// with no diagnostics channel (a one-shot export) turns them into an
    /// argument error rather than returning an export that quietly lacks
    /// what it was asked to carry.
    pub fn refused_api_facts(&self) -> &[Diagnostic] {
        &self.refused_api_facts
    }

    /// An expression typer over the function at `index` in
    /// [`AnalysisUnit::functions`], or `None` when there is no such function.
    pub(crate) fn expression_typer(&self, index: usize) -> Option<expr_types::ExpressionTyper<'_>> {
        Some(expr_types::ExpressionTyper {
            tree: &self.tree,
            text: &self.source,
            token_spans: &self.token_spans,
            resolution: self.resolutions.get(index)?,
            types: self.types.get(index)?,
            symbols: &self.symbols,
        })
    }

    /// Executable semantic operations aligned with the function table.
    pub(crate) fn evaluations(&self) -> &[EvaluationPlan] {
        self.evaluations.get_or_init(|| {
            self.functions
                .iter()
                .zip(&self.resolutions)
                .zip(&self.types)
                .map(|((function, resolution), types)| {
                    EvaluationPlan::build(
                        &self.tree,
                        &self.source,
                        &self.token_spans,
                        function,
                        resolution,
                        types,
                    )
                })
                .collect()
        })
    }

    /// Merged parser and CFG diagnostics.
    pub const fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    /// Dataflow results computed once and retained by this source snapshot.
    pub fn dataflows(&self) -> &[DataFlow] {
        self.dataflows
            .get_or_init(|| crate::csource::dataflow::analyze_unit(self))
    }

    /// Interprocedural summaries computed once from this unit's cached flows.
    pub fn summaries(&self) -> &Summaries {
        self.summaries
            .get_or_init(|| summarize_with_policy(self.dataflows(), self.options.external_calls))
    }

    /// A typed view of `id` tying its CFG and dataflow to this snapshot.
    pub fn function(&self, id: FunctionId) -> Option<FunctionAnalysis<'_>> {
        self.functions
            .get(id.0 as usize)
            .map(|_| FunctionAnalysis { unit: self, id })
    }

    /// Typed function views in stable source order.
    pub fn analysis_functions(&self) -> impl ExactSizeIterator<Item = FunctionAnalysis<'_>> {
        (0..self.functions.len()).map(|index| FunctionAnalysis {
            unit: self,
            id: FunctionId(index as u32),
        })
    }
}

/// One function viewed through its owning immutable analysis snapshot.
#[derive(Debug, Clone, Copy)]
pub struct FunctionAnalysis<'a> {
    unit: &'a AnalysisUnit,
    id: FunctionId,
}

impl<'a> FunctionAnalysis<'a> {
    /// Stable function identity within the owning unit.
    pub const fn id(self) -> FunctionId {
        self.id
    }

    /// Function name recovered from its declarator.
    pub fn name(self) -> &'a str {
        &self.unit.functions[self.id.0 as usize].name
    }

    /// Executable CFG for this function.
    pub fn cfg(self) -> &'a FunctionCfg {
        &self.unit.functions[self.id.0 as usize]
    }

    /// Cached dataflow result carrying the same source and function identity.
    pub fn dataflow(self) -> &'a DataFlow {
        &self.unit.dataflows()[self.id.0 as usize]
    }

    /// Cached interprocedural summary for this exact function identity.
    pub fn summary(self) -> &'a crate::csource::dataflow::Summary {
        self.unit
            .summaries()
            .get_by_id(self.id)
            .expect("every analyzed function has an identity-keyed summary")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_tracks_exact_source_and_is_deterministic() {
        let source = "int f(void) { return 0; }";
        let first = AnalysisUnit::new(source);
        let second = AnalysisUnit::new(source);
        let changed = AnalysisUnit::new("int f(void) { return 1; }");
        assert_eq!(first.source_id(), second.source_id());
        assert_eq!(first.source_id().name(), second.source_id().name());
        assert_ne!(first.source_id(), changed.source_id());
        assert_eq!(first.source(), source);
        assert_eq!(first.functions().len(), 1);
        assert_eq!(first.resolutions().len(), first.functions().len());
        assert_eq!(first.types().len(), first.functions().len());
        assert_eq!(first.evaluations().len(), first.functions().len());
    }

    #[test]
    fn evaluation_plans_are_lazy_and_cached() {
        let unit = AnalysisUnit::new("int f(int n){if(n){return n;} return 0;}");
        assert!(unit.evaluations.get().is_none());
        assert_eq!(unit.functions().len(), 1);
        assert!(unit.evaluations.get().is_none());
        let first = unit.evaluations();
        assert!(unit.evaluations.get().is_some());
        assert!(std::ptr::eq(first, unit.evaluations()));
    }

    #[test]
    fn explicit_dialect_prepares_the_owned_snapshot_before_identity() {
        let raw = "int f(int x @ eax){return x;}";
        let unit = AnalysisUnit::with_options(
            raw,
            AnalysisOptions {
                dialect: InputDialect::Decompiled,
                ..AnalysisOptions::default()
            },
        );
        let expected = Dialect::Decompiled(raw).normalize();
        assert_eq!(unit.source(), expected);
        assert_eq!(unit.source_id(), SourceUnitId::of(&expected));
        assert_eq!(unit.options().dialect, InputDialect::Decompiled);
        assert_ne!(unit.source_id(), AnalysisUnit::new(raw).source_id());
    }

    #[test]
    fn one_session_caches_flows_summaries_and_typed_function_views() {
        let unit = AnalysisUnit::new("int id(int x){return x;} int main(void){return id(1);}");
        assert!(std::ptr::eq(unit.dataflows(), unit.dataflows()));
        assert!(std::ptr::eq(unit.summaries(), unit.summaries()));
        let functions = unit.analysis_functions().collect::<Vec<_>>();
        assert_eq!(functions.len(), 2);
        assert_eq!(functions[0].name(), "id");
        assert_eq!(functions[0].id(), FunctionId(0));
        assert_eq!(functions[0].dataflow().function_id, functions[0].id());
        assert_eq!(functions[0].cfg().name, functions[0].name());
        assert_eq!(functions[0].summary().function_id, Some(functions[0].id()));
        assert!(unit.function(FunctionId(2)).is_none());
    }
}
