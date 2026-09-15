# QA iteration 25: named sizeof operands and census verification

Baseline: `edf2777` plus pending QA changes. Continued the existing fix in
`dataflow/events.rs` that excludes reads of known non-array scalar/pointer
locals used directly in sizeof, including nested parentheses and the
unparenthesized spelling. The initial workspace gate reproduced an incorrect
expected binding name: the actual checked-in fixture declares `pointer`, not
`p`. Corrected the exact identity assertion rather than relaxing its count.
The sole newly unused binding is `sizeof_array_versus_pointer:pointer`.

Added `tests/fixtures/sizeof_local.c`, covering scalar, pointer, and nested
operands across different argument values, and a Python regression checking
its three summaries. The fixture was compiled and executed with:

```bash
gcc -std=c11 -Wall -Wextra -Werror -O0 tests/fixtures/sizeof_local.c -o CACHE/o0
gcc -std=c11 -Wall -Wextra -Werror -O1 tests/fixtures/sizeof_local.c -o CACHE/o1
```

Both executables exited zero under GCC 15.2.0. CACHE for this run was
`/home/mjbommar/.cache/cindergraph/sizeof-local.KbGHRM`.

Validation commands:

- `cargo +1.88.0 test --workspace --all-features -q`: 578 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`: passed.
- `uv run --no-sync maturin develop --release`: rebuilt/installed ABI3 extension.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,038 passed, ten skipped, three optional Joern cases deselected.
- Ruff check/format and ty check on `python/tests/test_sizeof_local.py`: passed.
- `cargo +1.88.0 fmt --all -- --check` and `git diff --check`: passed.

This is bounded support, not general unevaluated-expression semantics.
Compound operands such as `sizeof(f(x))` can still produce false calls/reads,
and VLA size dependence is not reliable. Documented these limitations in the
Python reference, including the fact that summary completeness does not detect
them. The next substantive task is expression-context handling across event
collection, call collection, and VLA bound provenance; merely suppressing name
reads cannot implement that contract.

No performance claim, publication, remote CI, or external evaluator run. All
changes remain local and uncommitted; the broader QA goal remains active.
