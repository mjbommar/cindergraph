# Native graph API and NetworkX boundary

Date: 2026-09-15. Status: first native read-only API implemented.

## Decision

Cindergraph does not replace its graph engine with NetworkX, petgraph, or
rustworkx. Its analysis graphs already use deterministic Rust-native storage;
the immutable CFG uses dense IDs and compressed adjacency for successors and
predecessors. NetworkX remains an optional compatibility and interoperability
adapter, never the analysis substrate.

Python users who need topology without Python object expansion use
`NativeGraph`, returned by `native_graphs()` or
`AnalysisSession.native_graphs()`. Conversion to NetworkX is explicit through
`NativeGraph.to_networkx()` and requires the `graphs` extra.

## Required contract

- directed topology with isolated nodes, self-loops, and parallel edges;
- stable snapshot-local node and function identities;
- deterministic node and edge ordering;
- typed labels and arbitrary string attributes;
- efficient successors, predecessors, in-degree, and out-degree;
- forward and reverse transitive traversal without Python callbacks;
- one bulk boundary for node, edge, and edge-list materialization;
- all AST, CFG, DDG, CDG, and PDG representations;
- source ID, function ID, representation, graph kind, analysis revision,
  dialect, and external call policy on every native graph;
- checked unknown-node and unknown-representation failures;
- no mandatory Python package or new Rust crate dependency.

The current API satisfies this initial contract. Graph objects own their
export view and immutable forward/reverse compressed adjacency, so they remain
valid independently of the session that created them. Exported node IDs are
dense; offset and neighbour arrays preserve deterministic insertion order and
parallel-edge multiplicity without one tree node and one allocation per graph
node. Closure traversal uses a dense visited bitmap. `nodes()` and `edges()`
allocate Python dictionaries only on request. `edge_list()` is the
lower-overhead topology interchange. A NetworkX
`DiGraph` deliberately collapses parallel edges; callers select
`to_networkx(multigraph=True)` when edge multiplicity matters.

## Nice-to-have expansion

Add capabilities only with a measured consumer:

- bounded BFS/DFS and shortest unweighted paths;
- SCCs, weak components, and cycle-aware topological ordering;
- native subgraph and edge-kind projections;
- native forward slicing and batched reachability;
- buffer-protocol or NumPy-compatible integer arrays without mandatory NumPy;
- deterministic parallel traversal across independent function graphs;
- compact persisted snapshots and function-level invalidation;
- optional rustworkx conversion when downstream demand exists;
- a NetworkX backend only if maintaining the evolving backend protocol is
  justified by multiple real consumers.

Petgraph remains a possible implementation aid for a missing algorithm, not an
automatic storage migration. Its `GraphMap` does not preserve parallel edges,
while other petgraph structures introduce a second index/ownership contract.
Rustworkx is a useful optional Python ecosystem target but is not a drop-in
NetworkX replacement and its full package would substantially expand the
dependency tree. See the official [petgraph project](https://github.com/petgraph/petgraph),
[rustworkx documentation](https://www.rustworkx.org/dev/index.html), and
[NetworkX backend contract](https://networkx.org/documentation/stable/backends.html).

## Baseline measurement

Build the release extension first, then run:

```bash
export TMPDIR="$PWD/target/tmp"
uv run maturin develop --release
uv run python tools/bench_native_graph.py --statements 20000 --repeat 7
```

On the 2026-09-15 working tree after compact adjacency landed, CPython 3.14.3,
Linux x86-64, the generated 20,003-node/20,002-edge linear CFG measured:

| Operation | Median |
| --- | ---: |
| Build native views from a warm session | 6.835 ms |
| Native descendants from the entry | 0.293 ms |
| Convert native graph to NetworkX | 83.941 ms |
| NetworkX descendants | 4.258 ms |
| Serialize node-link JSON | 18.771 ms |
| Load that JSON into NetworkX | 79.653 ms |

Both traversals reached the same 20,002 descendants. Against the immediately
preceding `BTreeMap<u32, Vec<u32>>` representation, the same command measured
10.835 ms to build views and 2.530 ms for native descendants. That is a 37%
lower construction median and an 88% lower traversal median on this linear
case. Earlier fresh-process `/usr/bin/time -v` runs at 50,000 generated
statements measured 90,008 KiB peak RSS while retaining the source, session,
and pre-compact native graph, versus 214,660 KiB after additionally
materializing NetworkX; remeasure memory before attributing a new absolute RSS
number to compact adjacency. These are local baseline measurements, not
portable performance promises. Preserve release/debug, warm/cold, topology,
node/edge count, and peak-memory context in future reports.
