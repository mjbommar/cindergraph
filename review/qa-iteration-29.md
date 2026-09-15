# QA iteration 29: duplicate source identity in reachability queries

Ad hoc query testing reproduced an order-dependent positive verdict for
`reaches(code, "f", 0, "f")` when code contained both a zero-parameter and a
two-parameter definition of f. The starting arity was taken from the last
definition, and the source-equals-sink shortcut returned Yes before considering
ambiguity. Reversing the definitions changed the answer to Unknown. Serialized
summary arity also changed with definition order.

`Summaries` now retains its duplicate-name set privately. A query with an
ambiguous source refuses before arity or the zero-length-path shortcut. Summary
arity is the maximum recovered count across definitions and is documented as
metadata, not a selected valid signature. Duplicate summaries remain incomplete
with no merged positive flows. Unique-source self-reachability and missing-source
uncertainty are unchanged. This is a recovery/identity contract for invalid or
concatenated source, not a claim about execution of multiply defined C functions.

Nine Python tests cover both definition orders, three parameter indices,
unique and missing source controls, and full summary equality after reordering.
Three assertions failed before the fix; all nine pass afterward.

Validation on `edf2777` plus pending QA changes:

- `uv run --no-sync maturin develop --release`: rebuilt ABI3 extension.
- `uv run --no-sync pytest python/tests/test_query_identity.py -q`: nine passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,062 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Ruff check/format and ty check on the new test; Rustfmt and diff whitespace
  checks: passed.

No performance claim, publication, commit, external evaluator or remote CI.
The source distribution tested in iteration 28 predates this change. Broader
analysis limitations, including VLA handling, remain open; goal active.
