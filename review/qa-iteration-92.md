# QA iteration 92: complete local distribution preflight

Date: 2026-09-15. Baseline: commit `edf2777` plus the pending shared-tree
changes. This is local preflight evidence from a dirty tree, not a release
candidate, remote-platform result, or publication.

## Defects found and repaired

The first exact sdist failed `tools/check_python_distribution.py` because the
shipped semantic-foundation plan linked relatively to excluded `review/`
records. Those historical records remain checkout-only; the public document
now uses repository URLs. The shipped benchmark documentation also linked to
machine-readable JSON that the sdist omitted. `pyproject.toml` now includes
only `docs/benchmarks/data/*.json` alongside the Markdown documentation.

The archive link checker now accepts a directory link when an archive omits an
explicit directory entry but contains members below that prefix. A focused
regression test covers that archive representation.

The installed-wheel smoke script had two stale contracts. It recursively
counted every generated-stub method and required the old total of 15, although
analysis-session and native-graph methods brought the valid generated stub to
52 definitions. It now compares the `_Source` and `_CSource` stub methods and
parameters directly with the installed native namespaces. Its double-pointer
probes also expected the conservative result predating QA 78; they now require
the complete, parameter-to-return flow established by the current focused Rust
and Python tests.

## Exact artifact checks

The normalized local wheel and sdist passed:

```sh
python3 tools/check_python_distribution.py \
  target/release-candidate-dirty-20260915-3/cindergraph-*.whl \
  target/release-candidate-dirty-20260915-3/cindergraph-*.tar.gz
uvx --from twine==7.0.0 twine check --strict \
  target/release-candidate-dirty-20260915-3/*
```

The exact wheel was installed without dependencies under CPython 3.12 and
passed `tools/smoke_installed.py`. A separate install of its `graphs` extra
passed `tools/smoke_graphs_installed.py` with NetworkX 3.6.1. Both the core and
graphs consumer examples passed Ty against those isolated environments.

The exact sdist built and passed the installed-package smoke suite under
CPython 3.12.13, 3.13.12, and 3.14.3. A separate CPython 3.12 source install of
its `graphs` extra passed the NetworkX smoke suite.

Two sdist builds were byte-identical. Two normalized wheel builds initially
differed only in the CycloneDX timestamp and UUID because the manual command
omitted the release workflow's reproducibility epoch. Repeating the builds
with the workflow contract produced byte-identical wheels:

```sh
export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
uv run --no-sync maturin build --release --out target/release-candidate-dirty-20260915-5
uv run --no-sync maturin build --release --out target/release-candidate-dirty-20260915-6
python3 tools/normalize_wheel_sbom.py target/release-candidate-dirty-20260915-{5,6}/*.whl
```

Hashes of the reproducibility witnesses were:

```text
host wheel  1c11c1836f7354f7d07633f8d210e8496b521023915814c947626455f265051f
manylinux 2.17 wheel  d292a3a158887261897eeafa108a1da614fb4d525fcf692184eab0269f5bd245
sdist  efa8abd329240dbf2bcd9d4321cb4da856f53c25577e1b87b05aacd5661dde6a
```

The host-linked wheel correctly refused a forced manylinux 2.17 label because
it referenced newer glibc symbols. Building twice in the cached, pinned
`ghcr.io/pyo3/maturin:v1.15.0` container with separate Cargo target directories
produced byte-identical
`cp312-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64` wheels. The exact first
wheel passed structural and strict Twine validation plus isolated core and
NetworkX smoke tests under CPython 3.12.

The crate metadata/notice checker also passed the previously packaged archive;
its repeated archive hash was
`4caa6a15559eeeee3e808c7fa2db5c74790637bc28cb63c15ef5c41eacf9dae0`.

## Current-tree regression gates

```text
Ruff format: 88 files already formatted
Ruff lint: passed
Ty: passed
generated native stub: current
Python plus design contracts: 3923 passed, 10 skipped, 3 deselected
distribution-checker unit tests: 29 passed
crate-package unit tests: 19 passed
```

The Rust workspace, Clippy, rustdoc, packaged-crate tests, audit, and deny gates
passed earlier in this same release preflight. The only Rust edit since that
broad run was the Clippy-only `collapsible_else_if` repair, followed by the
full Rust and Python gates and a release extension rebuild.

Pinned release-tool validation also passed: zizmor 1.30.1 reported no workflow
findings beyond four repository suppressions, validate-pyproject 0.26 accepted
the manifest, all 11 Trove classifiers were current in the pinned
`trove-classifiers` snapshot, and pip-audit 2.10.1 found no known
vulnerabilities in the hash-locked dependency closure for all published
extras. The release-version checker rejected both `v0.1.0` while the changelog
remains `unreleased` and an independently mismatched `v9.9.9` tag.

The official PyPI and crates.io project APIs both returned HTTP 404 on
2026-09-15. GitHub reported zero configured deployment environments. Local
`main` remained five commits ahead of `origin/main` (`edf2777` versus
`28ae864`), and the newest hosted CI run covered only the latter commit. These
are current external-state checks, not proof of name reservation or registry
ownership.

A release-snapshot audit found 1,128 generated Joern workspace files visible
as untracked content. They consisted of `cpg.bin`, temporary CPG copies, and
project metadata beneath the repository-root `workspace/` directory. That
directory is now ignored without deleting the local comparison cache; the
intentional robustness corpus and checked benchmark evidence remain visible.

## Remaining release boundary

This evidence does not make the shared dirty tree publishable. The next
authoritative checkpoint must be a reviewed clean commit, followed by the same
crate/wheel/sdist gates from that commit and the configured remote five-wheel
matrix. Protected `crates-io` and `pypi` environments, first-release Cargo
credentials, PyPI trusted publishing, a release version decision, the dated
changelog, signed tag, human approvals, registry reconciliation, and
post-publication consumer checks remain outstanding.
