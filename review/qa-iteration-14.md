# QA iteration 14: parameter-count uncertainty

The early out-of-range branch in `reaches` bypassed summary completeness.
Recovered malformed input therefore returned `no` for any index beyond the
recovered signature even though the summary was incomplete. More concretely,
duplicate definitions with one and two parameters yielded different answers
for index 1 depending on declaration order: `unknown` versus `no`.

Out-of-range indices now return `no` only for complete summaries; otherwise
they return `unknown`. Valid complete source retains the previous behavior.
This is intentionally conservative for incomplete memory/call analyses too:
the current summary has only one completeness flag and does not separately
certify its parameter count. Splitting signature certainty from body coverage
could recover precision later without losing this protection.

Four new regressions failed before the change: three index boundaries
(1, 10, u32::MAX) on recovered input and the duplicate-definition order case.
The same index boundaries are checked against complete source to retain `no`.
The reference API contract is updated. Negative and above-u32 Python inputs
retain the native argument-conversion behavior; this does not change that API.

Validation on a rebuilt release ABI3 extension:

- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  916 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 575 passed, one ignored.
- Ruff, ty and Rust formatting checks passed.

Baseline: `edf2777` plus pending iterations 6–13. Nothing published or pushed;
no Joern/DecBench execution. General C soundness and independent signature
certainty remain outside this increment's evidence.
