# QA iteration 20: cross-format corpus consistency

Added comparisons for all 210 bundled C files across AST, CFG, DDG, CDG and
PDG. Each case independently decodes JSON and GraphML and compares function
ordering/names, every node ID and attribute, and every edge endpoint/attribute.
Edges are compared as multisets so duplicate edges cannot disappear unnoticed.
XML node IDs must be unique and every edge endpoint must exist. A separate
assertion requires the expected corpus size so collection cannot silently shrink.

Command: `uv run --no-sync pytest python/tests/test_graph_format_corpus.py -q`.
Result: 1,051 passed in 3.73 seconds (1,050 representation/file pairs plus the
corpus-presence assertion) on the iteration-19 release ABI3 build. No production
change was needed. Ruff format/lint and ty passed for the new test.

This adds a serializer consistency oracle, not an independent oracle for graph
semantics: both formats share the same analysis and GraphView. XML is parsed
for well-formedness but not validated against the full GraphML schema. Existing
Unicode/control-character boundary tests separately cover XML's lossy filtering.
No Graphviz, Joern, DecBench or external graph service was invoked.

Baseline: `edf2777` plus pending QA increments. No Rust source changed this
round, so Rust gates were not rerun. Changes remain local and uncommitted.

Final `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
1,998 passed, ten skipped, three optional Joern cases deselected in 4.40 seconds.
Diff whitespace checks passed.
