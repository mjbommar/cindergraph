# QA iteration 50: public documentation and release candidates

Replaced the placeholder README with the public landing page for the Rust and
Python packages. It now leads with the actual analysis surface, runnable quick
starts, uncertainty boundaries, documentation routes, development commands,
release status and provenance. All README links are absolute so they remain
valid when the same Markdown is rendered by crates.io or PyPI.

Added maintained documentation for installation, support/evidence and release
operation, plus a documentation index, QA index and `CHANGELOG.md`. Public
Python examples now include the README and execute under pytest. The Rust quick
start has a matching packaged integration test. Link and release-metadata tests
guard the maintained routes, latest QA pointer, SPDX metadata, supported Python
classifiers, project URLs, changelog version, crate publish policy and
registry-safe README links.

## Registry metadata and automation

Updated Python metadata to Core Metadata 2.4 conventions:

- `License-Expression: Apache-2.0`;
- explicit `License-File: LICENSE` and `License-File: NOTICE`;
- documentation, repository, issue and changelog URLs;
- keywords and developer/research audience classifiers;
- removed the deprecated license classifier.

The Rust core now identifies docs.rs as its documentation URL and restricts
publication to crates.io. The PyO3 binding crate remains `publish = false`.
The sdist includes the complete maintained documentation and changelog.

CI now runs the design-contract suite rather than only `python/tests/`. The
release workflow gained an independent full validation job, tag/version/
changelog agreement checks, crates.io dry-run, sdist install/smoke testing, a
protected `crates-io` publication environment, and sequencing that publishes
PyPI artifacts only after the core crate succeeds. Workflow dispatch builds
candidates but cannot publish. The protected environments, Cargo token and
PyPI trusted-publisher registration still require human configuration and were
not inferred from YAML.

This follows current Cargo guidance to dry-run and inspect packages before
publication and current PyPA guidance for SPDX license expressions,
license-file metadata and project URLs. Publication remains irreversible and
was not attempted.

Fresh exact-name API checks on 2026-09-14 returned HTTP 404 from both PyPI and
crates.io for `cindergraph`. This means no project was returned at that moment;
it is not a reservation or trademark clearance.

## Exact artifact evidence

Baseline: commit `edf2777` plus all pending QA changes through this iteration.
This is not a clean release commit.

`cargo +1.88.0 publish -p cindergraph --dry-run --locked --allow-dirty` passed
and aborted before upload as intended. The resulting crate contains 301 files,
including the README integration test; it is 582.9 KiB compressed. LICENSE and
NOTICE were verified byte-for-byte. Tests run from Cargo's unpacked package,
with an independent target directory, passed: 577 passed and one ignored.

Fresh candidates:

| Artifact | SHA-256 |
| --- | --- |
| `target/package/cindergraph-0.1.0.crate` | `b53e7e4bae7262609f9d566cbab967b78dd60b1447940d0b7799e914e8d4e5bc` |
| `target/qa-release-53/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | `fecc72fd8295f3bba88e056d6364524ab4d86e7c6e2d56f7ec5eea75de3147d0` |
| `target/qa-release-53/cindergraph-0.1.0.tar.gz` | `1b267ca572eaed43a449ccb4a4c9f02e52ba781458db1077d5f3fa0b202d0e7c` |

Twine's metadata and long-description checks passed for the wheel and sdist.
The exact wheel installed with no dependencies and passed the strict ownership
smoke test on CPython 3.12.13, 3.13.12 and 3.14.3. The exact sdist built through
isolated PEP 517 on CPython 3.12.13, installed with no runtime dependencies and
passed the same smoke test. Environments remain under
`/home/mjbommar/.cache/cindergraph/qa53.oNDZPo`.

## Current local gates

- Rust workspace: 580 passed and one ignored (576 core, one README integration
  test and three binding tests).
- Python plus review contracts: 2,941 passed, ten skipped and three optional
  Joern cases deselected.
- Rust 1.88 formatting, Clippy with warnings denied, strict rustdoc and pure
  core check: passed.
- Ruff format/check, ty, generated stub, documentation examples/links,
  release-metadata tests and diff whitespace: passed.
- Actionlint passed with only the two current runner labels ignored from the
  old local actionlint inventory, as previously source-checked in QA 15.

## What this does not prove

Only Linux x86-64 artifacts ran locally. The manylinux wheel requires glibc
2.34; no older-glibc or musllinux claim follows. The configured Linux AArch64,
macOS and Windows jobs did not run remotely. No registry name was reserved, no
protected environment or secret was configured, and no crate, wheel, tag,
commit or GitHub release was published. The worktree remains substantially
dirty and the remote `main` remains at the original extraction baseline.
Existing semantic limitations, including the iteration-49 type-recovery defect,
remain. These are release candidates for QA, not approved release artifacts.
