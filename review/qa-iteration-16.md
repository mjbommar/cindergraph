# QA iteration 16: fail-closed bundled corpus loading

Four core corpus gates silently returned success if the bundled directory
could not be opened, skipped failed directory entries, or skipped unreadable
source files: dataflow consistency, binding types, call summaries and
post-dominance consistency.

They now share a test-only loader that sorts C paths deterministically and
fails on directory enumeration, entry or file-read errors, or an empty C set.
Two negative tests exercise a known non-directory (`Cargo.toml`) and a real
directory containing no C files (`src/syntax/scan`). No fixture was renamed,
deleted or replaced to produce those checks. Existing corpus count/invariant
assertions remain in each consumer.

Validation against `edf2777` plus the pending QA changes:

- `cargo +1.88.0 test --workspace --all-features -q`: 577 passed, one ignored.
- Release ABI3 extension rebuilt, then `uv run --no-sync pytest python/tests/
  review/test_design_contracts.py -q`: 916 passed, ten skipped, three optional
  Joern cases deselected.
- Rust 1.88 Clippy with warnings denied passed after fixing unused path names.
- `cargo +1.88.0 package -p cindergraph --allow-dirty --offline`: built and
  verified the packaged core, including the new test-support module.
- `cargo +1.88.0 test --manifest-path target/package/cindergraph-0.1.0/Cargo.toml
  --offline -q`: 574 passed, one ignored from the packaged source. Formatting
  and diff whitespace checks passed.

This closes only these four bundled gates. Lexer/parser/CFG helpers still
contain skip-on-missing logic shared with optional external corpora. The
metrics corpus helper also tolerates missing subdirectories and unreadable
files. Those paths need explicit bundled-versus-optional policy rather than a
blanket change that pretends absent external corpora were supplied.
No Joern/DecBench process was run. Changes remain local and uncommitted.
