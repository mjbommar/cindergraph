# Benchmark records

Benchmark reports in this directory are dated evidence snapshots, not timeless
capability claims. Each report identifies its input population, dependency
versions, commands, denominator and limitations. Machine-readable results live
under [`data/`](data/).

Future cross-tool robustness measurements are governed by the
[C source-front-end robustness comparison plan](robustness-comparison-plan.md).
Its dependency-free manifest/result validator, reproducible 525-specimen
initial population, and Cindergraph, Clang, and Tree-sitter adapters are now
implemented. The plan is still methodology, not a completed cross-tool
benchmark record.

## Current comparisons

- [IOCCC source CFG comparison, 2026-09-15](ioccc-cfg-comparison-2026-09-15.md)
  compares Cindergraph and Joern on 15 cross-era obfuscated-C winners, with
  independently adjudicated function sets, identical-input CFG comparison,
  diagnostics, provenance, crashes, and wall-clock provider time.
- [Cindergraph and Joern for DecBench-adjacent CFG work, 2026-09-15](joern-decbench-2026-09-15.md)
  compares function recovery, DecBench VJ-GED, a stronger graph-isomorphism
  check, decompiler-dialect tolerance, and end-to-end provider time.
- [Joern CFG difference roadmap, 2026-09-15](joern-difference-roadmap-2026-09-15.md)
  assigns every original mismatch to a root-cause class, records completed
  semantic repairs, and links the checked proof or correction that closes each
  graph.
- [Post-remediation machine-readable comparison](data/joern-complete-2026-09-15.json)
  measures 894 exact graphs over the unchanged 930-function population.
- [Difference-closure audit](data/joern-closure-audit-2026-09-15.json) maps all
  36 current nonzero graphs, with no omissions or stale entries, to their
  checked proof artifacts.

The comparison is deliberately narrow. It does not claim that Cindergraph
implements Joern's code-property graph, AST, data-dependence graph, query
language, language coverage, or security-analysis ecosystem.
