# QA iteration 46: Rust crate artifact tests in CI

Refreshed the standalone Rust consumer gates rather than relying on the
workspace/PyO3 build. The core builds with no default features. Cargo packaged
300 files and successfully compiled the verified archive. Its LICENSE and
NOTICE match the repository byte-for-byte.

Cargo package verification compiles but does not run unit/corpus tests. Ran
those tests against Cargo's unpacked archive, with an independent target
directory, then added the same artifact-test gate to the Rust CI job. This
closes the gap where workspace tests could succeed while required fixtures
were absent from the published crate.

Local commands:

```bash
cargo +1.88.0 check -p cindergraph --no-default-features
cargo +1.88.0 package -p cindergraph --allow-dirty
uv run --no-sync python tools/check_crate_notices.py target/package/cindergraph-0.1.0.crate
CARGO_TARGET_DIR=/home/mjbommar/.cache/cindergraph/crate-qa46-target cargo +1.88.0 test --manifest-path target/package/cindergraph-0.1.0/Cargo.toml --locked --all-features -q
sha256sum target/package/cindergraph-0.1.0.crate
actionlint .github/workflows/ci.yml
git diff --check
```

All gates passed. Packaged-core tests: 576 passed, one ignored; this excludes
the separate Python binding crate's tests. The archive's generated lockfile
resolved regex 1.13.1 and serde_json 1.0.151, and the artifact was compiled/tested
on the declared Rust 1.88 floor. Dependency downloads/caches were available;
this is not a network-free build claim.

Crate SHA-256:
`5faa1812e6bec231680753508df516c9085b2b00a5a76792ec0832d6670c5dec`.
Baseline is `edf2777` plus pending QA through iteration 45. `--allow-dirty` is
an experimental packaging step, not an approved clean release snapshot. The
only implementation-tree change this turn is CI configuration; no Python-suite
rerun, new semantic claim, commit, publication, remote CI or external evaluator.
Artifacts retained; broader goal active.
