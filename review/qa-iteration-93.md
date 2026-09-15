# QA iteration 93: isolated source-snapshot reproduction

Date: 2026-09-15. Baseline: commit `edf2777` plus every visible pending
candidate file after generated-workspace exclusion. This remains dirty-tree
preflight evidence, not a committed or published release.

## Snapshot construction and boundary

A separate source tree was assembled under `target/` from `git archive HEAD`,
then overlaid with every modified tracked file and every untracked,
non-ignored file. It contained 860 files and occupied 8.7 MiB. It contained no
`.git`, `target`, `.venv`, or `workspace` root from the live checkout.

This tests a stricter boundary than another run in the shared worktree: local
editable-install residue, ignored Joern CPGs, prior build outputs, and Git
metadata cannot satisfy missing source or test inputs.

## Fresh snapshot gates

The snapshot created its own locked virtual environment, built a release-mode
editable extension, and compiled into its own Cargo target directory. It
passed:

```text
Rust format and strict Clippy: passed
Rust workspace: 806 passed, 1 ignored
README integration test: 1 passed
PyO3 binding tests: 3 passed
Rust doctests: 2 passed
no-default-features core check: passed
strict rustdoc: passed
Python plus design contracts: 3923 passed, 10 skipped, 3 deselected
Ruff format and lint: passed
Ty and generated-stub check: passed
RustSec audit: 28 dependencies scanned, no advisory failure
cargo-deny advisories, bans, licences and sources: passed
```

`cargo publish --dry-run --locked` succeeded from the snapshot without
`--allow-dirty`. The crate was packaged twice byte-identically and passed the
metadata, content, notice, registry-dependency and compressed-size checker.

## Artifact equivalence

With `SOURCE_DATE_EPOCH=1789389576`, derived from the real baseline commit, the
snapshot produced a normalized host wheel and sdist byte-identical to the
artifacts previously built and smoke-tested in the live checkout:

```text
host wheel  1c11c1836f7354f7d07633f8d210e8496b521023915814c947626455f265051f
sdist       efa8abd329240dbf2bcd9d4321cb4da856f53c25577e1b87b05aacd5661dde6a
```

Every packaged crate source member was also byte-identical. The snapshot crate
contained 306 files rather than the live checkout's 307 because Cargo emits
`.cargo_vcs_info.json` only when Git metadata exists. This is expected and is
why final crate identity must still be established from the clean release
commit rather than this Git-free reproduction tree.

An initial comparison used an incorrect manually supplied epoch. Its wheel
differed only in CycloneDX timestamp/UUID and the dependent RECORD entry; every
sdist member body was identical. Rebuilding with the actual commit epoch closed
that discrepancy exactly.

## Remaining boundary

The candidate is self-contained, but it is not yet an attributable Git
snapshot. The next release step is still to review and commit the 75 modified
paths and 518 visible untracked files, then rerun the authoritative package
hashes from that clean commit. Hosted non-x86 platforms and registry
administration remain external gates.
