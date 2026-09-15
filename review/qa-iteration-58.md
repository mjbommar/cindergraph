# QA iteration 58: reproducible and pinned release automation

This pass treated the GitHub workflows as executable supply-chain code rather
than documentation. Two publication weaknesses were repaired.

First, release wheels were already rebuilt and compared byte-for-byte, but the
sdist was built only once. The sdist job now invokes the same pinned Maturin
action twice, writes the second archive to `dist-rebuilt`, and requires equal,
non-empty filename-to-SHA-256 maps before installing or uploading the first
archive. A metadata regression checks that the two builds and fail-closed
comparison remain present.

Second, several actions still used mutable major-version tags or branches.
Every action in both workflows is now referenced by a full 40-character commit
hash, with the human-readable version retained only as a comment. A regression
enumerates every `uses:` entry and rejects any non-commit reference.

## Current-source verification

The action hashes were resolved from the corresponding upstream Git refs on
2026-09-14. The configured `ubuntu-24.04-arm` and `macos-15-intel` matrix labels
were checked against the current
[GitHub-hosted runners reference](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)
and [runner-images inventory](https://github.com/actions/runner-images); both
are valid hosted labels. The checkout's older actionlint 1.7.4 predates those
labels and produced false warnings. A checksum-verified actionlint 1.7.12 binary
from the [official release](https://github.com/rhysd/actionlint/releases/tag/v1.7.12)
accepted both complete workflow files with no findings.

Focused repository validation passed:

- 27 release-metadata, documentation-link and distribution-checker tests;
- Ruff on the modified metadata test;
- `git diff --check`;
- actionlint 1.7.12 over CI and release workflows.

The final complete Python/design rerun passed 3,030 tests, skipped ten and
deselected three optional Joern cases. Ruff and ty passed across their full
repository scopes, and Rust formatting remained clean. The complete Rust gates
remain the post-production-code QA 57 run: 589 passed and one external-corpus
test ignored.

The production Rust/Python analysis code did not change after the complete QA
57 source gates. Package candidates are refreshed below because
`docs/releasing.md` is intentionally included in the Python sdist.

Two fixed-epoch local builds produced identical maps, and the first copies
passed the custom distribution checker, Twine and check-wheel-contents:

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/qa58-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,226,203 | `ce8f022b12abb69b76ea1645e6df64acd625e904e5c7c094f9f0f56d6d657665` |
| `target/qa58-a/cindergraph-0.1.0.tar.gz` | 647,053 | `d91a3d72a754e869e5642cb8e6a50c182b7fe98b753e0de51d2425c19b7f266e` |

The unchanged wheel reproduces QA 57 byte-for-byte. The sdist hash changed as
expected because the shipped release guide changed. These are host-built local
candidates; they do not replace the remote manylinux 2.17/platform matrix.

## Boundary

Pinned hashes make execution reviewable; they do not establish that an
upstream action is benign, that GitHub-hosted runners are hermetic, or that the
remote matrix has run. Updating an action now requires an explicit reviewed
hash change. No workflow dispatch, tag, registry operation, commit or push was
performed.
