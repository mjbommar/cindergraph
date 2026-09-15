# QA iteration 36: sizeof of known scalar/pointer pointees

Probes reproduced a false parameter-to-return dependency for
`int f(int x){int y=x;int *p=&y;return sizeof(*p);}` with complete=True.
A pointer parameter operand additionally recorded a false memory read and
incomplete memory coverage. Added five regressions for built-in scalar,
qualified scalar, pointer pointees and the local points-to example. All five
failed before the change and pass afterward.

Unevaluated-region classification now runs after lexical event binding and
write promotion. For the direct shape sizeof(*p), it looks up that exact
NameRef's binding and declared type. A non-array declaration with a built-in
scalar pointee or a further pointer layer proves the operand fixed-size.
Opaque typedef pointees and array declarators are not treated as fixed scalars.
The existing region filter then removes both dataflow and memory-projection
events. No rendered-name lookup or global declaration-name table was added.

Expanded `tests/fixtures/sizeof_local.c` with a pointee-size function exercised
using both NULL and a valid address. GCC compiled and ran it successfully at
both optimization levels:

```bash
gcc -std=c11 -Wall -Wextra -Werror -O0 tests/fixtures/sizeof_local.c -o CACHE/o0
gcc -std=c11 -Wall -Wextra -Werror -O1 tests/fixtures/sizeof_local.c -o CACHE/o1
```

CACHE is `/home/mjbommar/.cache/cindergraph/sizeof-pointee.GBBGoH`; both runs
exited zero. This confirms the fixture's non-evaluation, not general VLA rules.

Validation on `edf2777` plus pending QA changes:

- Release extension rebuilt with `uv run --no-sync maturin develop --release`.
- `uv run --no-sync pytest python/tests/test_sizeof_local.py -q`: 31 passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,554 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Rustfmt, focused Ruff check/format, `ty check python/`, and diff whitespace:
  passed.

Updated the reference with the precise new supported shape. Complex dereference,
array/member and VLA size provenance remain open; completeness does not detect
all of those gaps. No performance claim, publication, commit, remote CI or
external evaluator. Changes local/uncommitted; broader goal active.
