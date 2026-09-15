# QA iteration 6: executable reference documentation

Baseline: `edf2777` plus the pending reference/rustdoc edits. This increment
finishes those documentation edits without changing analysis semantics.

The two reference pages document source coordinates, normalization, metrics,
graph identities, diagnostics, summaries and the limits of memory analysis.
Their six Python examples execute in page order in the default Python suite;
missing pages or pages without examples fail instead of silently skipping.
Rust API links now resolve without referring to private implementation items
or old Glaurung paths. CI builds rustdoc with warnings denied.

Local validation against the rebuilt release ABI3 extension, Python 3.14:

- `RUSTDOCFLAGS='-D warnings' cargo +1.88.0 doc --workspace --no-deps`: passed.
- `TMPDIR=/home/mjbommar/.cache/cindergraph uv run maturin develop --release`:
  passed.
- `uv run pytest python/tests/ review/test_design_contracts.py -q`: 498 passed,
  ten inherited skips, three optional Joern cases deselected. This comprises
  450 default-suite tests and 48 review tests.
- `cargo +1.88.0 fmt --all -- --check`: passed.
- `cargo +1.88.0 test --workspace --all-features -q`: 575 passed, one ignored.
- Ruff lint/format and ty on the new reference-example test: passed.

These checks do not establish sound C analysis or release readiness. Remaining
work includes parser-recovery uncertainty, interprocedural memory effects,
fixed-point limits, precise generated stubs, license inclusion in the Rust
archive, platform/artifact installation tests and release-runner maintenance.
No new performance claim is made: this increment changes documentation and
test coverage only. No Joern/DecBench execution or publication occurred.
