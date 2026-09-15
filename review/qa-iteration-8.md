# QA iteration 8: recovery uncertainty in summaries

Reproduced false completeness on three malformed inputs: an unterminated body,
a malformed expression and a missing semicolon. Each emitted diagnostics from
`analyze` but produced `complete=True` and an absent-sink `reaches` answer of
`no`. Five new regression cases failed before the change.

`DataFlow::recovery_free` now carries the absence of parser/CFG diagnostics into
summaries. Both warning and error diagnostics make it false. Completeness
requires this flag as well as memory completeness and resolved callees.
Python dataflow dictionaries expose the flag. The low-level `analyze_function`
entry point has no diagnostic context, so defaults it to false; the full-unit
entry point supplies the actual status.

This is deliberately unit-wide: parser recovery can affect declaration
boundaries and name resolution outside one diagnostic span. It trades some
precision for honest uncertainty; a later localized scheme needs evidence
that recovery effects are contained. Positive may-flow witnesses still win
over incompleteness, as before. Invalid parameter indices still return `no`;
that API choice is unchanged. This does not address missing diagnostics,
interprocedural memory effects or full C soundness.

Local validation on the rebuilt release ABI3 extension:

- `uv run pytest python/tests/ review/test_design_contracts.py -q`:
  503 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 575 passed, one ignored.
- `uv run python tools/gen_native_stub.py --check`: passed; dictionary keys
  currently do not appear as precise generated stub types.

No Joern/DecBench run or publication occurred. Baseline was `edf2777` plus
pending iterations 6 and 7; changes remain local. No performance claim is made.
