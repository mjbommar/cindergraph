# Cindergraph

Cindergraph is a native, tolerant C source-analysis library written in Rust with
native Rust and Python APIs. It parses ordinary and decompiler-shaped C,
measures functions, builds control and dependence graphs, follows supported
parameter flows across calls, and exports deterministic graph formats.

The project was extracted from
[Glaurung](https://github.com/mjbommar/glaurung), where this analysis first
lived. The extraction copied the selected implementation with history; later
Cindergraph QA repaired and extended it. Glaurung still carries its original
copy and does not yet consume this crate, so fixes do not currently flow back
automatically. See [Relationship to Glaurung](https://github.com/mjbommar/cindergraph/blob/main/docs/architecture/glaurung.md)
for the exact boundary and migration status.

Cindergraph is a standalone analysis engine: no JVM, Java service, Graphviz
process or C compiler is required at runtime.

> **Pre-alpha and not yet published.** The public APIs and serialized schemas
> can still change. Build from this checkout for now; the `pip install` and
> `cargo add` commands will become valid only after the corresponding registry
> releases are independently verified.

## Why Cindergraph?

- **One native engine, two APIs.** The pure Rust crate owns parsing and
  analysis; a thin PyO3 extension exposes the same engine to Python.
- **Useful partial results.** Malformed or decompiler-shaped input can produce
  recovered functions alongside diagnostics instead of losing the whole file.
- **Explicit uncertainty.** Recovery, summary and memory-completeness signals
  distinguish supported negative results from gaps in the model.
- **Deterministic output.** Stable ordering makes JSON, DOT, GraphML, Mermaid
  and text exports suitable for review and regression testing.
- **Analysis rather than orchestration.** Core use has no Python runtime
  dependencies and does not start an external analysis server.

## Python quick start

After [building the checkout](https://github.com/mjbommar/cindergraph/blob/main/docs/install.md):

```python
import cindergraph as cg

code = """
int clamp(int value, int maximum) {
    if (value > maximum) return maximum;
    return value;
}
"""

report = cg.analyze(code)
assert not report.diagnostics

function = report.functions[0]
print(function.name, function.cyclomatic, function.cognitive)
# clamp 2 1
```

Metrics retain source spans and line information:

```python
print(function.first_line, function.last_line, function.code_lines)
# 2 5 4

for hotspot in report.hotspots(by="cyclomatic", limit=5):
    print(hotspot.name, hotspot.cyclomatic)
# clamp 2
```

Inspect dataflow and completeness before interpreting missing edges:

```python
flow = cg.data_flow("int f(int x){int y=x;return y;}")[0]
assert flow["recovery_free"]
assert flow["effects_complete"]
assert flow["memory_complete"]
assert [edge["variable"] for edge in flow["edges"]] == ["x", "y"]

summary = cg.call_summaries("int f(int x){return x;}")[0]
assert summary["complete"]
assert summary["flows"] == [
    {"parameter": 0, "sink": "return", "sink_parameter": None}
]

claim = cg.query_reaches("int f(int x){return x;}", "f", 0, "f")
assert claim.claim == "found_may_path"
assert claim.path == (cg.ReachabilityStep(function_id=0, function="f", parameter=0),)
```

Export a graph without installing a graph library:

```python
name, document = cg.export_graphs(
    "int f(int x){return x ? 1 : 0;}", repr="cfg", format="json"
)[0]
assert name == "f"
assert '"directed": true' in document
```

Install the optional `graphs` extra when you specifically need NetworkX
objects. File and directory adapters for callers migrating from `pyjoern` are
documented in the [Python reference](https://github.com/mjbommar/cindergraph/blob/main/docs/reference/source-python.md).

## Rust quick start

Until crates.io publication, depend on the core by path:

```toml
[dependencies]
cindergraph = { path = "../cindergraph/crates/cindergraph" }
```

Parsing and metrics return the recovered value together with diagnostics:

```rust
use cindergraph::metrics;

let parsed = metrics::analyze("int answer(void) { return 42; }");
let (report, diagnostics) = parsed.into_parts();

assert!(diagnostics.is_empty());
assert_eq!(report.functions[0].name, "answer");
assert_eq!(report.functions[0].graph.cyclomatic, 1);
```

The core re-exports `dataflow`, `export`, `metrics`, `normalize`, `parity` and
`parse`; lower-level syntax and C modules remain available for native callers.
API documentation is built with warnings denied as part of the Rust gate.
For multi-product analysis, use the owning `AnalysisUnit` session documented in
the [Rust API guide](https://github.com/mjbommar/cindergraph/blob/main/docs/reference/rust-api.md)
so CFGs, cached dataflow,
summaries and identities come from one source snapshot.

Python has the same persistent ownership model:

```python
session = cg.AnalysisSession("int id(int x) { return x; }")
cfg = session.control_flow_graphs()[0]
flow = session.data_flow()[0]
summary = session.call_summaries()[0]
assert cfg["source_id"] == flow["source_id"] == summary["source_id"]
assert session.backward_slice("id", cfg["cfg"]["exit"], function_id=0)
name, graph = session.export_graphs(repr="pdg", format="json")[0]
assert name == "id" and '"directed": true' in graph

native = session.native_graphs(repr="pdg")[0]
assert native.node_count == len(native.nodes())
assert native.edge_count == len(native.edge_list())
```

Use the one-shot functions for one result and `AnalysisSession` when requesting
several products from the same text. The latter parses once, lazily caches
dataflow and summaries in Rust, and reuses them for slicing and graph export.
Pass `dialect="decompiled"` or `dialect="preprocessed"` to make source
preparation an explicit part of the snapshot; `session.source` is the exact
normalized text whose byte offsets every result addresses.

Named calls without a body default to the sound `external_calls="unknown"`
policy. A caller may explicitly select `"taint_return"` to propagate every
argument into the return while retaining incomplete coverage, or
`"assume_pure_no_flow"` when an external contract really guarantees purity and
an argument-independent return. The last policy can turn unknowns into negative
answers and must not be used as a generic precision switch. Every structured
session result records the selected policy.

For large Python-side graphs, `native_graphs()` returns read-only Rust-backed
topology without serializing or constructing Python graph dictionaries.
Successor, predecessor, degree, ancestor, and descendant queries remain native.
Call `graph.to_networkx()` only when a downstream API specifically needs
NetworkX; it remains an optional interoperability dependency.

## Analysis surface

| Surface | What it provides |
| --- | --- |
| Parsing | Tokens, spans, recovered syntax tree and diagnostics |
| Metrics | Physical size, CFG complexity, syntax nesting, calls and Halstead measures |
| General CFG | Statement-oriented control flow used by metrics and graph export |
| Dataflow | Lexically scoped definitions, uses, reaching edges and dead-store observations |
| Dependence | DDG, CDG and combined PDG exports plus backward slicing |
| Call summaries | Bounded parameter-to-return/callee-parameter propagation for known direct calls |
| Normalization | Explicit ordinary, preprocessed and decompiler-input preparation |
| Serialization | JSON, DOT, GraphML, Mermaid and a readable text form |
| Parity CFG | A narrow comparison graph for stored offline evaluation fixtures |

General CFGs and parity CFGs are intentionally different. Metrics, slicing and
ordinary export use the general CFG. The parity graph preserves a particular
comparison shape and is not a richer representation.

## Boundaries you should know first

Cindergraph is not a C compiler, a code property graph, a Joern distribution,
or a drop-in implementation of arbitrary Joern queries. It does not resolve
headers, execute a preprocessor, infer ABI layout or model all C aliasing and
side effects.

In particular:

- parser diagnostics can accompany useful partial results;
- `semantic_issues` reports structured coverage qualifications;
  `recovery_free`, `effects_complete`, summary `complete` and `memory_complete`
  remain compatibility views—none is a proof of general C soundness;
- global effects, unknown pointees, projected-memory reaching edges and VLA
  size dependencies remain incomplete; field/element identities are exposed
  as explicit regions but do not yet constitute an alias-completeness proof;
- duplicate definitions are preserved by list-shaped reports and rejected by
  name-keyed APIs where identity would otherwise be ambiguous;
- compatibility adapters implement only the explicitly documented `pyjoern`
  subset and reject unsupported AST, DDG and option semantics.

Read [Python source analysis](https://github.com/mjbommar/cindergraph/blob/main/docs/reference/source-python.md)
for precise API contracts and
[support and evidence](https://github.com/mjbommar/cindergraph/blob/main/docs/support-and-evidence.md) before using
a negative analysis result as an assurance claim.

## Documentation

- [Documentation index](https://github.com/mjbommar/cindergraph/blob/main/docs/README.md)
- [Design and product roadmap](https://github.com/mjbommar/cindergraph/blob/main/docs/ROADMAP.md)
- [Installation and local artifact testing](https://github.com/mjbommar/cindergraph/blob/main/docs/install.md)
- [Release operator checklist](https://github.com/mjbommar/cindergraph/blob/main/docs/releasing.md)
- [Changelog](https://github.com/mjbommar/cindergraph/blob/main/CHANGELOG.md)
- [Python analysis reference](https://github.com/mjbommar/cindergraph/blob/main/docs/reference/source-python.md)
- [Metric definitions](https://github.com/mjbommar/cindergraph/blob/main/docs/reference/source-metrics.md)
- [Support and evidence matrix](https://github.com/mjbommar/cindergraph/blob/main/docs/support-and-evidence.md)
- [Relationship to Glaurung](https://github.com/mjbommar/cindergraph/blob/main/docs/architecture/glaurung.md)
- [Chronological QA records](https://github.com/mjbommar/cindergraph/blob/main/review/README.md)

The reference examples and local documentation links are tested. QA records
identify their source baseline and are historical evidence, not automatically
current release claims.

## Development

The repository uses Rust 1.88, Maturin, PyO3, `uv`, Ruff and ty. The short local
gate is:

```bash
export TMPDIR="$PWD/target/tmp"
mkdir -p "$TMPDIR"
uv sync --locked --dev
uv run --no-sync maturin develop --release
cargo +1.88.0 test --workspace --all-features
uv run --no-sync pytest python/tests/ review/test_design_contracts.py
```

See [Install and build](https://github.com/mjbommar/cindergraph/blob/main/docs/install.md)
for the complete gate and isolated
wheel/sdist smoke workflows. Tests that invoke external Joern tooling are
optional and deselected by default.

## Release status

Release automation is configured for Linux x86-64/AArch64, macOS x86-64/arm64
and Windows x86-64 wheels plus an sdist. An earlier pending snapshot has local
manylinux 2.17 audit and smoke evidence; the current pending tree has a
normalized, relocatable local manylinux 2.34 candidate but is not a clean
release snapshot. The remote platform matrix has not been claimed green.
Nothing in this README asserts that either registry package has been published.

## License and provenance

Licensed under
[Apache-2.0](https://github.com/mjbommar/cindergraph/blob/main/LICENSE). See
[NOTICE](https://github.com/mjbommar/cindergraph/blob/main/NOTICE) for
extraction provenance. Distribution artifacts must carry both files.
