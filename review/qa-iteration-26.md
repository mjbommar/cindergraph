# QA iteration 26: compound sizeof event suppression

Baseline: `edf2777` plus pending QA iterations. Follow-up tests reproduced
false reads and writes inside sizeof, false reachability from an unevaluated
call, and lost provenance after `sizeof(y=0)`. One initial assertion also
incorrectly indexed a name-sorted summary list as though it were source-ordered;
the regression now selects its target function by name.

Added an outer-expression classifier for operands that cannot be VLAs in
valid C: binary, assignment, conditional, literal, terminal call/increment,
and selected unary forms. Parentheses are unwrapped iteratively. Crucially,
the classifier does not decide from a nested call: dereferencing a call result
can produce an array, whereas the call itself cannot return an array.

The resulting unevaluated regions filter definitions, uses and call records
before unresolved binding allocation and reaching-definition solving. Memory
projection uses the same regions to exclude indirect stores and loads. No
Python schema or public Rust result fields were added.

Expanded the checked-in `tests/fixtures/sizeof_local.c` with nested calls,
increments, assignment, and an otherwise invalid null-pointer store inside
sizeof. GCC 15.2.0 compiled it with `-std=c11 -Wall -Wextra -Werror` at both
`-O0` and `-O1`; both binaries exited zero. Artifacts:
`/home/mjbommar/.cache/cindergraph/sizeof-compound.3UYVWt/o0` and `o1`.

Validation on the release extension rebuilt by
`uv run --no-sync maturin develop --release`:

- `uv run --no-sync pytest python/tests/test_sizeof_local.py -q`: 22 passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,049 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 578 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Rustfmt check, Ruff check/format and ty check on the Python regression file,
  and `git diff --check`: passed (Ruff applied one formatting change).

Updated the reference to distinguish the repaired compound forms from the
remaining array/member/dereference and VLA-size gaps. CFG shape is still
syntactic; this change concerns dataflow events, not removal of CFG branches.
Summary completeness does not yet detect every unsupported sizeof case.
No benchmark comparison, full C semantic soundness claim, external evaluator,
publication or remote CI. Changes remain local and uncommitted.
