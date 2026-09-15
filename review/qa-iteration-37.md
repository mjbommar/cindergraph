# QA iteration 37: parenthesized dereference chains in sizeof

Follow-up probes showed that harmless parentheses reintroduced false reads:
sizeof(*p) was handled, while sizeof(*(p)) was not. Known-depth chains such
as sizeof(**p) also remained falsely dependent on the pointer value.

Extended the binding-sensitive classifier with an iterative walk through
parentheses and unary dereferences. It counts dereference depth, resolves the
terminal NameRef by its exact use span, and requires sufficient declared pointer
depth with a fixed-size resulting type and no array declarator. It does not
strip arbitrary unary operators or reinterpret typedef-hidden array types.

Five new expression variants failed before the change and pass afterward. A
shadowed pointer control additionally verifies that only the outer parameter's
evaluated read survives. The focused sizeof suite now has 37 passing cases.

Expanded the existing compiler fixture with sizeof(*((*(x)))) for an int**,
called with NULL. GCC `-std=c11 -Wall -Wextra -Werror` at both `-O0` and `-O1`
compiled and ran the fixture successfully (exit zero). Binaries are retained at
`/home/mjbommar/.cache/cindergraph/sizeof-chain.jwAdyE/o0` and `o1`.

Validation on `edf2777` plus pending QA changes:

- `uv run --no-sync maturin develop --release`: rebuilt extension.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,560 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- Focused Ruff check/format, `ty check python/`, Rustfmt and diff whitespace:
  passed.

Reference updated. VLA size provenance, array/member operands and unresolved
types remain open. No performance claim, commit, publication, remote CI or
external evaluator. Changes local/uncommitted; broader goal remains active.
