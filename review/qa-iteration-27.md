# QA iteration 27: indexed unevaluated-region containment

Iteration 26 added a linear region scan for every event and for nodes visited
by memory projection. For R unevaluated regions and E tested events/nodes,
this containment work was O(R * E). Replaced it with sorted region starts and
prefix-maximum ends: O(R log R) setup and O(log R) per containment query.
This describes only the containment component, not the whole analysis.

`regions.rs` checks the index against the original linear predicate for all
valid query intervals with endpoints below 16 over five region collections:
empty, nested, overlapping, adjacent, and duplicate starts. Prefix maxima
preserve individual-region containment; adjacency does not incorrectly make
a query crossing two regions count as contained. Python regression coverage
also checks nested sizeof regions at 1, 32, 128 and 512 expressions while
retaining the actual return-value read.

## Local release-profile measurements

Command: `uv run --no-sync python tools/bench_analysis.py --shape sizeof`.
Added the deterministic sizeof workload to the existing benchmark tool. Each
row uses a warmup followed by seven batches of five calls, reporting the
median batch time per call. CPython 3.14.3; the ABI3 extension was built with
`uv run --no-sync maturin develop --release` before both implementations.
Baseline was `edf2777` plus QA through iteration 26; the second implementation
adds this containment index. Both used the same generated input and wrapper.

512 sizeof expressions, one function, 6,167 input bytes:

| Operation | Linear scan, ms | Index, ms | Index repeat, ms |
|---|---:|---:|---:|
| analyze (control) | 0.759617 | 0.796554 | 0.765825 |
| data_flow | 1.354340 | 0.800923 | 0.786633 |
| call_summaries | 1.354761 | 0.796879 | 0.785357 |

This is a local workload-specific improvement, not a corpus-wide speed claim.
Small timings were noisy (the first indexed 32-expression dataflow median was
0.100806 ms, versus 0.059565 ms on repeat). No timing threshold was added to CI,
and peak memory was not measured.

## Gates

- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,053 passed, ten skipped, three optional Joern cases deselected.
- Ruff check/format on the modified Python test and benchmark; ty check on the
  Python test; Rustfmt and diff whitespace checks: passed.

No semantic scope expansion: VLA and unresolved sizeof operand handling remain
open. No publication, commit, external evaluator or remote CI. Goal active.
