# QA iteration 1

Baseline: `28ae864681d9ffd07dd43948147b01478bfe7617`.
The original design review is a historical snapshot; this records repairs.

## Changes and evidence

- Incomplete summaries now propagate to callers through the fixed point.
  Forty seeded eight-function call graphs are checked against an independent
  graph traversal oracle, including cycles and shuffled definition order.
  Twenty-one seeds failed before the repair; all forty pass afterward.
- Duplicate function definitions produce an incomplete summary with no merged
  positive flows. Two additional tests cover the ambiguous definition and a
  caller using it; both failed before the repair and pass afterward.
- General CFGs retain definition node identity and name span. Dataflow no longer
  re-enumerates the entire translation unit once per function. These are new
  public fields on `FunctionCfg`; external struct-literal construction will
  need to supply them. Generated graphs and existing consumers use the same
  source tree for those IDs.
- `export_path()` preserves CRLF bytes like `analyze_path()`. Nine cases cover
  three newline encodings and ASCII, UTF-8, and replacement-decoded headings,
  checking all five graph representations.
- Slicing rejects a node outside the selected function with `IndexError`.
  Tests distinguish a node valid in a different function and preserve the
  existing missing-function `KeyError` behavior.
- Root graph-choice exports are explicitly re-exported for static typing.

## Native build provenance

An old ignored `_native.cpython-314-x86_64-linux-gnu.so` shadowed newly rebuilt
`_native.abi3.so`. It was moved, recoverably, to
`/home/mjbommar/.cache/cindergraph/stale-native-cpython314-20260914.so`.
All final checks loaded the rebuilt ABI3 module; the build used
`TMPDIR=/home/mjbommar/.cache/cindergraph uv run maturin develop --release`.
Future checks must inspect `cindergraph._native.__file__` when changing ABI or
interpreter versions. A successful build alone did not prove the new code ran.

## Performance

`tools/bench_analysis.py` constructs a deterministic chain of small functions.
Each row is the median of seven samples, five calls per sample, after warmup.
Timings include parsing and Python conversion. Both runs used release builds
and Python 3.14.3 on the same host. Baseline came from the separately installed
review wheel (SHA-256 in the design review), not the stale extension.

| API | Functions | Baseline median ms | Repaired median ms |
| --- | ---: | ---: | ---: |
| analyze | 8 | 0.08459 | 0.08227 |
| analyze | 32 | 0.32767 | 0.33231 |
| analyze | 128 | 1.30374 | 1.29198 |
| data_flow | 8 | 0.11943 | 0.08246 |
| data_flow | 32 | 0.82916 | 0.34945 |
| data_flow | 128 | 9.03068 | 1.38934 |
| call_summaries | 8 | 0.10177 | 0.07195 |
| call_summaries | 32 | 0.76378 | 0.38595 |
| call_summaries | 128 | 8.91059 | 1.34105 |

The 128-function workload improves approximately 6.5x for dataflow and 6.6x for
summaries. These are synthetic workload results, not general speedup claims.
The chain crosses the existing 16-round summary limit, so large-chain summary
precision still requires further work. The timings do not establish correctness
of the remaining flow queries or peak memory usage.

To check output preservation, compared canonical JSON of report dictionaries,
dataflow dictionaries, and PDG JSON exports over all 196 C files / 900 functions
in `crates/cindergraph/tests/decompiler_fixtures/src`. Baseline wheel and rebuilt
extension produced the same aggregate SHA-256:
`e27421a1d733661a6ab57a60e035b0cb7c43dace07fdf503561e4ff8d5c1908a`.
This equivalence check deliberately excludes summaries whose uncertainty
semantics changed, and the CRLF file API behavior intentionally repaired.

## Final local gates and remaining work

- Rust 1.88: 572 core tests and 3 binding tests passed; 1 core test ignored.
- Python: 204 passed, 10 inherited CLI skips, 3 optional Joern tests deselected.
- Rust Clippy with warnings denied, formatting, Ruff, and ty passed.
- Review contract suite: 38 passed, **8 still fail** (previously 34/12).
- No Joern/DecBench execution or upstream interaction.

Next work remains the principal semantic repair: retain real return and call
expression identities, propagate reaching values instead of binding presence,
and make `reaches()` traverse actual call sites and argument positions. Then
address pointer/field dependence, unresolved bindings, the remaining documentation
and release gaps, and extend benchmarks to large single functions and real
corpora. The overall quality-improvement goal remains active.
