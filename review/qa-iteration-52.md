# QA iteration 52: fail-closed partial-publication recovery

Audited the tag workflow as a state machine rather than only as a happy-path
build. It had a material recovery defect: if crates.io publication succeeded
and the later PyPI job failed, a workflow rerun would fail while attempting to
publish the immutable crate version again, preventing PyPI recovery.

Added `tools/publish_crate.py`. The protected crates.io job now packages the
core and checks the registry before publishing. A missing version is
published; an existing version is skipped only when its crates.io SHA-256
checksum matches the freshly packaged `.crate`; a same-version checksum
mismatch fails closed. The ordinary workflow still cannot publish on manual
dispatch, and the registry token remains scoped to the protected publication
job. Unit and workflow-contract tests cover all three decisions.

The exact-name read-only registry query returned no `cindergraph` 0.1.0 on
2026-09-14. The publication script itself was deliberately not run end to end,
because doing so when the version is absent would publish it.

## Reproducibility and artifact evidence

Two consecutive Cargo package builds produced the same crate SHA-256. This is
the invariant used by partial-publication recovery; it does not imply that all
build products are generally reproducible. Maturin wheel archives changed hash
after rebuilding despite unchanged compiled source, so no wheel reproducibility
claim is made.

The release documentation now explains the guarded rerun behavior. Because it
is shipped in the sdist, the Python candidates were rebuilt and tested again:

| Artifact | SHA-256 |
| --- | --- |
| `target/package/cindergraph-0.1.0.crate` | `e7fff927be7a966b4a30b1def6164ec2c97bbc4a6d5c0a999f8505db4ab7beca` |
| `target/qa-release-55/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | `1f23fbb3da20d4b5f43647adbdb235e5f0d63a44cfaf3d15db23d20e1c283f44` |
| `target/qa-release-55/cindergraph-0.1.0.tar.gz` | `8bdadc34d18d27889ceb12c10e8b0d9c244494d97663c219b63f647db88f3de6` |

Twine accepted both Python artifacts. The exact wheel installed without
dependencies and passed the isolated ownership smoke test on CPython 3.12.13,
3.13.12 and 3.14.3. The exact sdist rebuilt through PEP 517, installed without
runtime dependencies and passed the same smoke test on CPython 3.12.13.
Environments remain under `/home/mjbommar/.cache/cindergraph/qa55.HAPnPN`.

## Current checks

- Complete Python and design-contract suite: 2,946 passed, ten skipped and
  three optional Joern cases deselected.
- Focused publish/release/documentation contracts: passed.
- Ruff on the publication script and its tests: passed.
- Actionlint with the two established runner-label exceptions: passed.
- Diff whitespace: passed.
- Rust code is unchanged from iteration 51, where 582 tests passed and one was
  ignored, with formatting, Clippy and strict rustdoc green.

## Remaining boundary

No tag, commit, push, protected environment, credential, registry upload or
remote platform run was created. Checksum equality proves byte identity of the
crate archive only; it does not establish authorship or approval. The dirty
worktree and semantic limitations recorded in iterations 49 and 51 remain.
