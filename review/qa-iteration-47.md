# QA iteration 47: per-declarator initializer effects

Initializer probes found both false negatives and false positives:
`int a=x,b=a;return b;` lost x-to-return dependence, while
`int a=0,b=x;return a;` incorrectly acquired it. Initialization writes were
delayed to the end of the enclosing Decl, so the earlier write appeared after
later initializer reads and its provenance span included unrelated initializers.

Each declaration write now takes its effect point from its own Declarator's
following Initializer, or the declarator end when no initializer exists.
Direct child relationships keep a nested initializer/declaration from being
mistaken for a sibling's extent. Corrected comments distinguishing a binding
already being in scope from its initialized value becoming available.

Three new regressions compare comma-separated declarations with separate
declarations, covering two-/three-link propagation and an independent earlier
variable. All three failed before the fix and pass afterward, without
unresolved initializer uses. The initial shadowed self-initializer probe already
resolved to the inner binding and was not a scope-resolution defect.

Added `tests/fixtures/initializer_order.c`. Compiled with
`gcc -std=c11 -Wall -Wextra -Werror` at `-O0` and `-O1` and ran both: exit zero.
Binaries retained under
`/home/mjbommar/.cache/cindergraph/initializer-order.WbNEEy/o0` and `o1`.

Validation on `edf2777` plus pending QA changes:

- `uv run --no-sync maturin develop --release`: rebuilt extension.
- `uv run --no-sync pytest python/tests/test_initializer_order.py -q`: three passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,797 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Focused Ruff check/format, `ty check python/`, and diff whitespace: passed.

No performance claim, commit, publication, remote CI or external evaluator.
VLA-size provenance and other documented limits remain open. Changes
local/uncommitted; broader goal active.
