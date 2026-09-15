# DecBench-adjacent CFG workflows

Cindergraph supports the source-C CFG work commonly performed next to
DecBench: recovering functions from ordinary or decompiler-shaped C, producing
the compact per-function CFG shape used by published source-CFG comparisons,
adapting that shape to the NetworkX inputs consumed by VJ-GED, and replacing a
small topology-oriented subset of `pyjoern` calls.

Cindergraph does not depend on DecBench or invoke Joern. It can be used as an
in-process provider in a separate evaluation harness, but it does not publish
DecBench results or implement DecBench's complete pipeline.

## Install

The serialized parity representation has no Python runtime dependency beyond
Cindergraph itself:

```bash
uv add "cindergraph @ git+https://github.com/mjbommar/cindergraph.git@main"
```

Install the `graphs` extra for NetworkX graphs and the `pyjoern` compatibility
surface:

```bash
uv add "cindergraph[graphs] @ git+https://github.com/mjbommar/cindergraph.git@main"
```

Until wheels are published, installation from GitHub builds the native
extension and requires Rust 1.88 or newer and a platform linker.

## Produce serialized parity CFGs

`parity_cfgs()` accepts one translation unit as text and returns a mapping from
function name to a compact CFG:

```python
from cindergraph import source_cfg

text = """
int classify(int x) {
    if (x < 0) return -1;
    if (x > 0) return 1;
    return 0;
}
"""

cfg = source_cfg.parity_cfgs(text)["classify"]
assert set(cfg) == {"nodes", "edges", "entry", "exit", "degenerate"}
assert cfg["entry"]
assert isinstance(cfg["exit"], list)
```

The fields are:

| Field | Meaning |
| --- | --- |
| `nodes` | Dense basic-block identifiers |
| `edges` | Directed `[source, target]` block pairs |
| `entry` | Block identifiers carrying the entry role |
| `exit` | Block identifiers carrying the exit role |
| `degenerate` | Whether the recovered function has the comparison pipeline's degenerate shape |

This representation is deterministic and JSON-compatible. NetworkX is not
constructed or imported on this path.

## Produce GED-ready NetworkX graphs

`cfgs_from_decompiled()` returns one `networkx.DiGraph` per recovered function:

```python
from cindergraph import source_cfg

graphs = source_cfg.cfgs_from_decompiled(
    "int absolute(int x){return x < 0 ? -x : x;}"
)
graph = graphs["absolute"]
assert graph.is_directed()
assert any(node.is_entrypoint for node in graph.nodes)
assert any(node.is_exitpoint for node in graph.nodes)
```

Each node has a dense `id`, `is_entrypoint`, and `is_exitpoint`. These are the
node properties read by DecBench's VJ-GED calculation. Statements, labels,
types, and source text do not participate in that metric and are therefore not
present on these parity nodes. Depending on the parity projection's control
shape, a function can have no node carrying the exit role; consumers must use
the supplied role flags rather than infer or require one exit node.

When input contains conditional-preprocessor directives, this adapter attempts
local expansion with `gcc` or `cc`, removes include directives before invoking
the preprocessor, and falls back to the original tolerant-parser input if
preprocessing is unavailable or fails. The dependency-free
`parity_cfgs()` function does not invoke a compiler.

## Migrate topology-only `pyjoern` callers

For a single decompiler-output file, change the import and keep the familiar
topology call:

```python
from cindergraph.source import fast_cfgs_from_source

graphs = fast_cfgs_from_source(
    "decompiled.c",
    is_decompilation=True,
)
```

The adapter reads invalid UTF-8 with replacement, normalizes common
decompiler-shaped syntax in memory, and returns recovered functions even when
the file also has parser diagnostics. Set `strict=True` to raise
`SourceParseError` on any diagnostic.

Use `parse_source()` for metadata and recursive `.c` / `.h` directory input:

```python
from cindergraph.source import parse_source

functions = parse_source(
    "decompiler-output/",
    no_ddg=True,
    no_ast=True,
    is_decompilation=True,
)
for (name, filename), function in functions.items():
    print(name, filename, function.start_line, function.end_line)
```

Directory results use `(function name, absolute filename)` keys so file-local
functions with the same spelling remain distinct. Duplicate definitions inside
one file are rejected by name-keyed adapters; use `analyze_path()` and its
ordered `functions` collection when both bodies must be retained.

`parse_callgraph(path, is_decompilation=True)` provides a per-file graph of
directly named calls. It does not resolve indirect calls, headers, or symbols
across translation units.

## Choose the correct CFG surface

The parity CFG exists for comparison compatibility. It is not the graph used by
the rest of Cindergraph:

| Need | API |
| --- | --- |
| Compact DecBench-shaped serialized topology | `source_cfg.parity_cfgs()` |
| NetworkX topology with entry/exit flags | `source_cfg.cfgs_from_decompiled()` |
| File-based `pyjoern` topology migration | `cindergraph.source.fast_cfgs_from_source()` |
| General CFG, AST, DDG, CDG, or PDG export | `export_graphs()` or `AnalysisSession.export_graphs()` |
| Metrics, dataflow, summaries, and slicing | `analyze()` or `AnalysisSession` |

General CFGs retain Cindergraph's analysis-oriented control structure. Parity
CFGs deliberately reproduce a narrower comparison shape. Results from the two
surfaces should not be mixed as though their nodes had the same semantics.

## Supported boundary

This use case covers:

- tolerant recovery of functions from ordinary and decompiler-shaped C;
- per-translation-unit parity CFG projection;
- deterministic serialized nodes, edges, entry roles, and exit roles;
- NetworkX adaptation for topology and VJ-GED-style consumers;
- file and recursive-directory discovery through the documented compatibility
  subset;
- direct named-call graphs; and
- diagnostics, warning mode, and strict failure mode.

It does not cover:

- Joern JIL statements or raw Joern graph nodes;
- Joern-compatible AST or DDG results;
- Joern's code-property graph, query language, dataflow engine, or language
  ecosystem;
- a general source-to-binary matching database;
- complete preprocessing, header resolution, ABI layout, or compiler semantics;
- cross-translation-unit indirect-call resolution; or
- DecBench publication, scoring orchestration, or upstream interaction.

For measured scope and results, see
[Cindergraph versus Joern for DecBench-adjacent CFG work](../benchmarks/joern-decbench-2026-09-15.md).
For the complete Python API contract, see
[Python source analysis](../reference/source-python.md).
