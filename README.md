# Cindergraph

Cindergraph is a tolerant C source-analysis library written in Rust with Rust
and Python APIs. It parses ordinary and decompiler-shaped C,
measures functions, builds control and dependence graphs, follows supported
parameter flows across calls, and exports deterministic graph formats.

Cindergraph is a standalone analysis engine: no JVM, Java service, Graphviz
process or C compiler is required at runtime.

The current release candidate is available from GitHub. PyPI and crates.io
packages are not published yet, and APIs and serialized schemas may change
before the first stable release.

## Install

Python 3.12 or newer and a Rust 1.88 or newer toolchain are required while the
package is installed from source. Add Cindergraph to a uv-managed project:

```bash
uv add "cindergraph @ git+https://github.com/mjbommar/cindergraph.git@main"
```

To try it in a new environment without modifying an existing project:

```bash
uv venv
uv pip install "cindergraph @ git+https://github.com/mjbommar/cindergraph.git@main"
uv run python -c 'import cindergraph as cg; print(cg.analyze("int f(void){return 1;}").functions[0].name)'
```

Add the optional NetworkX adapter only when a downstream library requires
NetworkX objects:

```bash
uv add "cindergraph[graphs] @ git+https://github.com/mjbommar/cindergraph.git@main"
```

Rust projects can use the core crate directly from GitHub:

```bash
cargo add cindergraph --git https://github.com/mjbommar/cindergraph.git
```

Pin a Git commit instead of `main` when a build must be reproducible. See the
[installation guide](https://github.com/mjbommar/cindergraph/blob/main/docs/install.md)
for local checkout, wheel, source-distribution, and development workflows.

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

## DecBench-adjacent CFG workflows

Cindergraph provides a separate parity projection for workflows that compare
per-function source CFG topology using entry and exit roles. It accepts a whole
translation unit and returns the compact serialized shape without requiring
NetworkX or DecBench:

```python
from cindergraph import source_cfg

cfgs = source_cfg.parity_cfgs("int abs(int x){return x < 0 ? -x : x;}")
cfg = cfgs["abs"]
assert set(cfg) == {"nodes", "edges", "entry", "exit", "degenerate"}
```

With the `graphs` extra installed,
`source_cfg.cfgs_from_decompiled(text)` returns NetworkX graphs whose node
identity and `is_entrypoint` / `is_exitpoint` attributes match the inputs read
by DecBench's VJ-GED calculation. Existing topology-only `pyjoern` callers can
move from `pyjoern.fast_cfgs_from_source` to
`cindergraph.source.fast_cfgs_from_source`; file, recursive-directory, and
direct-call-graph adapters are also available.

This surface covers tolerant function recovery, DecBench-shaped CFG
serialization, GED-ready NetworkX adaptation, and the documented subset of
file-based `pyjoern` calls. It does not implement Joern JIL nodes, Joern AST or
DDG results, a code-property graph, DecBench publishing, or arbitrary DecBench
pipelines. The parity projection is intentionally separate from the general
CFG used by metrics, slicing, dependence analysis, and ordinary graph export.

See [DecBench-adjacent workflows](https://github.com/mjbommar/cindergraph/blob/main/docs/use-cases/decbench-adjacent.md)
for installation, migration examples, preprocessing behavior, result schemas,
and limitations. The measured comparison with Joern is reported in
[Cindergraph versus Joern for DecBench-adjacent CFG extraction](https://github.com/mjbommar/cindergraph/blob/main/docs/benchmarks/joern-decbench-2026-09-15.md).

## Rust quick start

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
| Parsing | Tokens, spans, recovered syntax tree and diagnostics, including implicit-int and K&R definitions |
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

## How it compares

Cindergraph is designed for embedded, tolerant analysis of C and
decompiler-shaped C. Similar tools solve overlapping but broader or different
problems:

| Tool | Best fit | Difference from Cindergraph |
| --- | --- | --- |
| Cindergraph | In-process C recovery, metrics, CFG and dependence graphs, bounded flow summaries, and deterministic export | Focuses on one C analysis surface; does not provide a compiler or general code-property graph platform |
| Joern | Multi-language code-property graphs, a query language, and a mature security-analysis ecosystem | Broader platform and query model; requires an external JVM-based toolchain |
| Eclipse CDT | IDE-grade C/C++ parsing, indexing, bindings, and editor tooling | Richer C/C++ semantic and IDE model; not packaged as a small Rust/Python analysis library |
| Clang | Standards-oriented C/C++ compilation, diagnostics, ASTs, and compiler tooling | Provides compiler-grade semantics and build-context integration; malformed decompiler output is not its primary input |
| Tree-sitter C | Fast incremental concrete-syntax parsing and editor integration | Provides syntax trees rather than Cindergraph's metrics, CFG, dataflow, summaries, and graph exports |

The comparisons are measured for specific tasks rather than presented as
overall rankings:

- [Cindergraph versus Joern for DecBench-adjacent CFG extraction](https://github.com/mjbommar/cindergraph/blob/main/docs/benchmarks/joern-decbench-2026-09-15.md)
  reports function recovery, CFG agreement, decompiler-input recovery, and
  end-to-end provider time over a fixed corpus.
- [Cindergraph versus Joern on 15 IOCCC winners](https://github.com/mjbommar/cindergraph/blob/main/docs/benchmarks/ioccc-cfg-comparison-2026-09-15.md)
  measures obfuscated-source function recovery, diagnostics, CFG agreement,
  crashes, and provider time with identical-input comparisons.
- [Robustness comparison with Joern, Eclipse CDT, Clang, and Tree-sitter](https://github.com/mjbommar/cindergraph/blob/main/docs/benchmarks/joern-cdt-robustness-2026-09-15.md)
  reports clean input, named decompiler cases, controlled damage, and random
  damage. Function yield is distinguished from semantic correctness.
- [Benchmark index](https://github.com/mjbommar/cindergraph/blob/main/docs/benchmarks/README.md)
  links the datasets, methodology, version pins, and follow-up analyses.

## Boundaries you should know first

Cindergraph is not a C compiler, a code property graph, a Joern distribution,
or a drop-in implementation of arbitrary Joern queries. Its Rust core does not
resolve headers or execute a preprocessor, infer ABI layout or model all C
aliasing and side effects. The optional DecBench-facing Python adapter can
invoke a discovered or explicitly selected host C preprocessor for local macro
and conditional expansion; it removes includes first and returns an observable
status report, so this adds no mandatory wheel dependency.

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

Use Joern when the application needs its CPG schema, query language,
multi-language frontends, or security-analysis ecosystem. Use CDT or Clang when
complete build context, C++ support, compiler-grade types, ABI layout, or IDE
indexing is required. Use Tree-sitter when incremental concrete syntax trees
are the primary product. Cindergraph is intended for callers that need a small,
in-process C analysis engine and useful, explicitly qualified results from
imperfect source.

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
- [DecBench-adjacent CFG workflows](https://github.com/mjbommar/cindergraph/blob/main/docs/use-cases/decbench-adjacent.md)
- [Metric definitions](https://github.com/mjbommar/cindergraph/blob/main/docs/reference/source-metrics.md)
- [Support and evidence matrix](https://github.com/mjbommar/cindergraph/blob/main/docs/support-and-evidence.md)
- [Benchmarks and cross-tool comparisons](https://github.com/mjbommar/cindergraph/blob/main/docs/benchmarks/README.md)
- [Relationship to Glaurung](https://github.com/mjbommar/cindergraph/blob/main/docs/architecture/glaurung.md)

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
wheel and source-distribution smoke workflows.

## Release status

Cindergraph 0.1.0 is preparing for its first registry release. GitHub CI tests
the Rust crate, Python package, wheel, source distribution, supported CPython
versions, dependency audits, and package provenance. Until PyPI and crates.io
publication is complete, install from GitHub as shown above.

## License and provenance

Licensed under
[Apache-2.0](https://github.com/mjbommar/cindergraph/blob/main/LICENSE). See
[NOTICE](https://github.com/mjbommar/cindergraph/blob/main/NOTICE) for
extraction provenance. Distribution artifacts must carry both files.
