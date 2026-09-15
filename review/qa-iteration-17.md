# QA iteration 17: restore the missing auxiliary corpus

The lexer, parser, CFG and metrics gates searched for
`tests/decbench_corpus/src` relative to the Rust crate. Its 14 C sources existed
only relative to the repository root, so the larger decompiler-fixture set
masked their absence. This was missing local fixture coverage, not a request
to run the external DecBench harness.

Bundled byte-identical copies of those 14 sources beneath the Rust crate so
Cargo archives carry them. The existing repository-root paths remain intact
for Python tests. A new test compares filenames and exact bytes, preventing
the copies from silently diverging. The lexer/parser/CFG/metrics bundled gates
now each require both directories via the fail-closed loader. The metrics
gate no longer silently skips unreadable files. Optional external corpus
loading remains separate.

Measured with `cargo +1.88.0 test --workspace --all-features in_repo_corpus
-- --nocapture`: all four gates passed over 210 C files and 930 functions.
Lexer diagnostics: zero; parser errors: zero, warnings: six (existing X-macro
fixture recovery); CFG validation failures: zero. These are structural checks,
not a compiler-equivalence or semantic-soundness claim.

Workspace tests passed 577 cases with one ignored. Rebuilt the release ABI3
extension and ran `uv run --no-sync pytest python/tests/
review/test_design_contracts.py -q`: 917 passed, ten skipped, three optional
Joern cases deselected. The new copy-equivalence test passed. Rust Clippy passed
before the final loader wiring in lexer/parser/CFG; final archive gates are
recorded separately below.

Baseline: `edf2777` plus pending QA increments. No fixture was deleted, no
external evaluator was invoked and nothing was published. Copies add a small
maintenance cost, explicitly guarded by the byte-comparison test.

Final archive checks: `cargo +1.88.0 package -p cindergraph --allow-dirty
--offline` built and verified the archive, then `cargo +1.88.0 test
--manifest-path target/package/cindergraph-0.1.0/Cargo.toml --offline -q`
passed 574 core tests with one ignored. Rust formatting and Ruff/ty on the
new Python test also passed.
