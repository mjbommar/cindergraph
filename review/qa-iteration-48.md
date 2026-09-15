# QA iteration 48: seeded initializer provenance

Extended the initializer-order regression with 128 seeded programs. Each has
24 initializers selecting a previously bound variable, constant zero, a known
identity call, or a known argument-discarding call. A separate Boolean tracker
computes whether the selected return value depends on parameter x. Both a
single comma-separated declaration and individual declarations must match this
oracle, have no unresolved uses, and produce identical complete summaries.

This covers 256 declaration layouts. The oracle is exact for this limited
copy/constant/identity/drop fragment, not for arbitrary arithmetic, aliases or
C side effects. All cases passed; no new engine defect was found.

Expanded `tests/fixtures/initializer_order.c` with a mixed identity/drop call
chain, then compiled using `gcc -std=c11 -Wall -Wextra -Werror` at both `-O0`
and `-O1` and ran both executables. Both exited zero; binaries remain at
`/home/mjbommar/.cache/cindergraph/initializer-calls.SC9xCX/o0` and `o1`.
This compiler check covers the fixture file, not all generated programs.

Validation on `edf2777` plus pending QA changes:

- `uv run --no-sync pytest python/tests/test_initializer_order.py -q`: 131 passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,925 passed, ten skipped, three optional Joern cases deselected.
- Ruff check/format and ty check on the expanded regression, and diff whitespace:
  passed.

Used iteration 47's release extension. No Rust changes, rebuild or Rust-suite
rerun; no benchmark comparison, publication, commit, remote CI or external
evaluator. Changes local/uncommitted; broader goal active.
