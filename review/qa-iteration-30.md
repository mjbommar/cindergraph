# QA iteration 30: reject ambiguous slice targets

Follow-up on duplicate query identity found that the Python backward-slice
binding used the first matching function definition. Same-named bodies could
therefore silently select different node universes after source reordering;
even an out-of-range error described whichever body happened to come first.

The binding now checks uniqueness before inspecting node bounds or constructing
dependence graphs. Duplicate requested names raise ValueError. Missing names
still raise KeyError; invalid graph-local nodes in unique functions still raise
IndexError. Unrelated duplicate names do not prevent slicing a unique target.
Updated the Python docstring and reference with this exception contract.

Added eight cases to `python/tests/test_query_identity.py`: two source orders
times three node values, plus unique-target and missing-target controls. Six
cases failed before the fix (four silent selections, two wrong exception
types). All pass afterward. These are recovery/identity checks, not execution
claims for invalid C with multiple definitions.

Validation on `edf2777` plus pending QA changes:

- `uv run --no-sync maturin develop --release`: rebuilt extension.
- `uv run --no-sync python tools/gen_native_stub.py --check`: passed; no
  signature changes or generated-stub delta.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,070 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Ruff check/format and ty check on the changed Python facade and test;
  Rustfmt and diff whitespace checks: passed.

No core slicing algorithm change or performance claim. No commit, publication,
remote CI or external evaluator. The implementation remains local/uncommitted,
and the broader goal remains active.
