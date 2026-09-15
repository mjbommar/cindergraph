# Rust analysis sessions

`AnalysisUnit` owns one immutable source snapshot and is the preferred Rust API
when a caller needs more than one analysis product. It parses and builds CFGs
once, retains stable source/function identities, and lazily caches dataflow and
interprocedural summaries.

```rust
use cindergraph::csource::semantic::{AnalysisUnit, FunctionId};

let unit = AnalysisUnit::new("int id(int x) { return x; }");
assert!(unit.diagnostics().is_empty());

let function = unit.function(FunctionId(0)).expect("function 0");
assert_eq!(function.name(), "id");
assert_eq!(function.cfg().name, "id");
assert_eq!(function.dataflow().function_id, function.id());

let summary = unit.summaries().get("id").expect("id summary");
assert!(summary.complete);
```

Source preparation is an explicit snapshot option:

```rust
use cindergraph::csource::semantic::{AnalysisOptions, InputDialect};

let unit = AnalysisUnit::with_options(
    "int id(int x @ eax) { return x; }",
    AnalysisOptions {
        dialect: InputDialect::Decompiled,
        ..AnalysisOptions::default()
    },
);
assert_eq!(unit.options().dialect, InputDialect::Decompiled);
assert!(!unit.source().contains("@ eax"));
```

`unit.source()` is the prepared source whose bytes define `source_id` and every
span. The default is `InputDialect::Ordinary`, which preserves the supplied
text exactly. Preprocessed and decompiled inputs each receive one appropriate
normalization pass before parsing; they are not interchangeable modes.

`AnalysisOptions::external_calls` controls named direct callees without a body.
Its default, `ExternalCallPolicy::Unknown`, cannot certify effects or negative
flows. `TaintReturn` propagates every argument into the return but remains
incomplete. `AssumePureNoFlow` is a caller-supplied contract asserting no
caller-visible effects and an argument-independent return; because it can
justify negative answers, use it only when that contract is known independently.
External models participate in the fixed point but are not exposed as invented
source functions through `Summaries::iter`, `len`, or `get`.

`AnalysisUnit::analysis_functions()` iterates typed function views in source
order. `AnalysisUnit::function(id)` returns `None` for an ID outside this exact
snapshot. A `FunctionId` is meaningful only together with its `SourceUnitId`;
it is not a cross-file or persistent database identifier.

The older `parse`, `function_cfgs`, `dataflow::analyze`, and export convenience
functions remain useful for one-shot work. Calling several of them separately
may parse or analyze the same text more than once. Use a session when CFG,
dataflow, summaries, or identity-bearing queries must agree on one snapshot.
Python callers have the equivalent `cindergraph.AnalysisSession`; see the
[Python source guide](source-python.md#reuse-one-analysis-snapshot).

`cindergraph::csource::export::export_unit(&unit, repr)` produces AST, CFG,
DDG, CDG or PDG views from the owned snapshot. Dependence exports reuse
`unit.dataflows()`; they do not silently construct a second parse or CFG set.

For interprocedural reachability, `reaches_detailed(unit.summaries(), ...)`
returns `Reachability` rather than collapsing evidence into `Flow`. Its
`path` records formal-parameter states for a found may-path; `uncertainty`
explains an unknown; and an empty uncertainty set is required for `Flow::No`.
The older `reaches()` remains the three-valued compatibility projection.
`reaches_by_id_detailed()` accepts exact source and sink `FunctionId` values.
`Summaries::get_by_id()` and `FunctionAnalysis::summary()` retain separate
facts for duplicate names; `Summaries::get(name)` remains only the explicitly
ambiguous compatibility view.

Diagnostics describe tolerant recovery. A constructed unit is not necessarily
valid C, and an empty diagnostic list is not a proof that every semantic
dimension is complete. Check each flow's structured `semantic_issues` and
coverage dimensions before treating an absent edge as conclusive.
