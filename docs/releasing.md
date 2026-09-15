# Release Cindergraph

This is the release operator checklist for the Rust crate and Python
distribution. Registry publication is effectively permanent: never publish
from a dirty checkout or from an unreviewed local artifact.

## One-time registry setup

1. Repeat the official API checks recorded in the
   [support matrix](support-and-evidence.md#registry-name-snapshot), then confirm
   `cindergraph` ownership on crates.io and PyPI immediately before the first
   release. HTTP 404 establishes only that no project is publicly visible;
   availability checks do not reserve a name.
2. Create protected GitHub environments named `crates-io` and `pypi`, each with
   required human reviewers.
3. For the bootstrap crates.io release, add a narrowly scoped token as the
   `CARGO_REGISTRY_TOKEN` secret on the `crates-io` environment. crates.io
   cannot configure trusted publishing until the crate has a first release;
   this token path is intentionally temporary.
4. Configure PyPI trusted publishing for this repository, the
   `.github/workflows/release.yml` workflow and the `pypi` environment. Do not
   add a long-lived PyPI API token.
5. Protect release tags and the `main` branch according to the repository's
   review policy.

These are external administrative operations and cannot be inferred from the
workflow file. Verify them in each registry and repository settings UI.

After the first crates.io release, configure its trusted publisher for this
repository, `.github/workflows/release.yml`, and the `crates-io` environment.
Then replace the workflow's bootstrap secret with the official
`rust-lang/crates-io-auth-action`, verify the short-lived token path, enable
crates.io's trusted-publishing-only setting, and revoke the bootstrap token.
Do not claim that migration before both registry settings and a workflow run
prove it.

## Prepare the release commit

1. Start from a clean `main` whose remote commit and CI result are known.
2. Set the shared workspace version in `Cargo.toml`; Maturin reads the Python
   distribution version from the binding crate.
3. Replace `## X.Y.Z — unreleased` in `CHANGELOG.md` with the release date.
4. Update the README's pre-release notice and installation commands only after
   the registry plan is approved. Do not claim a package is available before it
   can be independently fetched.
5. Recheck README links, package URLs, SPDX metadata, LICENSE and NOTICE.
6. Run the complete local gate from [installation](install.md).

The release version must be unused on both registries. Published versions
cannot be overwritten.

## Validate distributable artifacts

From the clean release commit:

```bash
export TMPDIR="$PWD/target/tmp"
mkdir -p "$TMPDIR"
cargo +1.88.0 publish -p cindergraph --dry-run --locked
cargo +1.88.0 package -p cindergraph --locked
cargo audit --deny warnings
cargo deny check
uvx --from zizmor==1.30.1 zizmor .github/workflows
uvx --from validate-pyproject==0.26 --with packaging==26.0 validate-pyproject pyproject.toml
uvx --from trove-classifiers==2026.6.1.19 python tools/check_trove_classifiers.py
set -o pipefail
uv export --locked --no-dev --all-extras --no-emit-project \
  --format requirements.txt | \
  uvx --from pip-audit==2.10.1 pip-audit --strict \
    --progress-spinner off --require-hashes -r /dev/stdin
uv run --no-sync maturin build --release --out target/release-candidate
python3 tools/normalize_wheel_sbom.py target/release-candidate/*.whl
uv run --no-sync maturin sdist --out target/release-candidate
uvx --from twine==7.0.0 twine check --strict target/release-candidate/*
sha256sum target/package/cindergraph-*.crate target/release-candidate/*
```

Run `cargo package` a second time without changing the tree and require the
crate archive's SHA-256 to remain identical. CI and release validation enforce
this comparison automatically. It covers deterministic Cargo archive assembly;
it is not a claim that two independent compilers emitted identical machine
code. The structural checker also enforces Cindergraph's 2 MiB compressed crate
budget, leaving deliberate headroom below crates.io's 10 MB service limit while
catching accidental corpus or generated-file growth.

The normalizer removes absolute checkout paths emitted in cargo-cyclonedx
`bom-ref` fields and regenerates wheel `RECORD`; run it before artifact hashes,
validation or upload. Inspect `cargo package --list`, the crate archive, wheel metadata and sdist
members. Test the `.crate` file's unpacked source and install the exact wheel
and sdist into clean environments using [the artifact smoke procedure](install.md).
Do not substitute an editable install for an artifact test.

Run `tools/check_python_distribution.py` over the wheel and sdist before their
runtime smoke tests. It validates archive paths and member types, wheel RECORD
hashes, metadata, licences, typing files, native payload, CycloneDX identity,
and the sdist's required build/source inputs and exclusion boundary.
The same registry metadata contract—including classifiers, project URLs,
licence files, Python floor, extras and dependencies—is parsed from both wheel
`METADATA` and sdist `PKG-INFO`; the sdist root version must agree as well.
Twine 7.0.0's strict check separately validates the registry metadata and
whether PyPI can render the README used as the long description. Keep the
version pinned in local instructions and automation so a tooling update cannot
silently change the release gate.

## Tag and automated publication

Create an annotated signed tag named exactly `vX.Y.Z` on the reviewed release
commit and push it. The release workflow:

1. checks that the tag and package version agree and that the changelog has a
   matching release heading;
2. runs Rust, Python, documentation, lint, typing and generated-stub gates;
3. builds and smoke-tests the platform wheel matrix and sdist;
4. waits for approval in the `crates-io` environment, then publishes the core;
5. downloads each Python producer into a separate directory, requires exactly
   five uniquely named wheels plus the sdist, verifies each wheel's platform
   against its producer and its embedded tags against its filename, then reruns
   structural and Twine validation over that gathered set;
6. waits for approval in the `pypi` environment, then publishes only the
   validated upload directory using trusted publishing with PyPI attestations;
7. waits for PyPI's version API and requires its complete filename and SHA-256
   set to equal the reviewed upload directory.

The PyPI job downloads only `wheels-*` and `sdist`; it never uses a wildcard
that could admit the separately preserved Rust crate artifact. The collector
then requires exactly those six producer directories before copying anything
into the upload directory.

If crates.io succeeds but a later job fails, rerunning the workflow packages
the core again, requires it to match the preserved reviewed archive, and
compares that SHA-256 checksum with the immutable registry version. An exact
match is skipped so PyPI can continue; a mismatch fails closed. Never bypass
that mismatch by changing source under the same version.

The PyPI publisher's attestations are explicitly enabled even though the
official action currently enables them by default. Post-upload reconciliation
is separate: attestations identify the trusted publisher, while the API check
proves every reviewed filename arrived with the reviewed bytes and no extra
file appeared under the immutable version.

After reconciliation, pinned `pypi-attestations` 0.0.30 downloads each of the
five wheels and the sdist through PyPI, retrieves its Integrity API provenance,
cryptographically verifies the attestation, and requires the signing Trusted
Publisher identity to belong to `https://github.com/mjbommar/cindergraph`.
Presence of a provenance URL alone is not accepted as signature verification.

Before upload, a PyPI API preflight classifies immutable state. An absent
version uses the action's normal fail-on-duplicate behavior. A complete exact
match skips upload and proceeds to verification. A partial release resumes
with `skip-existing` only when every existing filename and checksum is an exact
subset of the reviewed upload; any foreign or byte-different file fails before
new files are sent. This narrow recovery case is why the production workflow
does not enable duplicate tolerance unconditionally.

Workflow dispatch builds candidates but never publishes. A tag alone should
not bypass protected-environment review.

The workflow uses one repository-wide `cindergraph-release` concurrency group
with cancellation disabled. A second candidate or tag run waits for the active
release instead of cancelling it or racing registry state. CI uses a separate
per-ref group and may cancel an obsolete run when a newer commit arrives.

Linux release jobs use an explicit manylinux 2.17 policy rather than automatic
host detection. The Maturin action commit and Maturin 1.15.0 are pinned, and
all wheels must pass Maturin's PyPI compatibility check before smoke testing.
Local host builds can legitimately carry a newer tag and are not substitutes
for the container-built release wheels.

Every workflow action is pinned to a full commit hash. The trailing version
comments are maintenance hints, not executable references. Review upstream
changes and update the hash deliberately; do not replace a hash with a mutable
major-version tag or branch while preparing a release.

Every wheel archive is created twice from the same source and build cache with
`SOURCE_DATE_EPOCH` derived from the release commit. Publication stops unless
the two filename-to-SHA-256 maps are exactly equal. This tests deterministic
packaging, not independent clean-room compilation. The variable is explicitly
forwarded into Linux build containers; setting it only on the runner does not
reach the pinned action's container.

The sdist job likewise creates the archive twice with the commit-derived epoch
and requires identical filename-to-SHA-256 maps before installation or upload.
Only the first, independently checked archive set is uploaded; the rebuilt set
exists solely as a reproducibility witness.

The Rust crate is also packaged twice and compared byte for byte before release
jobs can start. The later publication helper compares that reviewed archive's
checksum with crates.io on a partial-release rerun. Both CI and release
validation parse the normalized packaged manifest, enforce the crate identity,
MSRV, licence, URLs, targets and registry-only dependency set, compare the
packaged README and notices with the repository, and run the exact unpacked
crate's tests.

The validated `.crate` is retained as a workflow artifact. The protected
publication job downloads and revalidates it, rebuilds from the tagged checkout
and requires byte identity with the reviewed archive before invoking Cargo.
After a new upload, it waits for crates.io to expose the immutable checksum and
requires that checksum to equal the reviewed archive. A rerun accepts an
existing version only under that same equality check.

Zizmor checks the workflows for unsafe GitHub Actions patterns in offline mode.
Release jobs disable `setup-uv` caches, and every checkout disables persisted
Git credentials. Zizmor is pinned because changes to its audit set should be
reviewed as deliberate gate changes.

The metadata gates validate the complete `pyproject.toml` schema and check
every declared Trove classifier against the pinned current registry. This
prevents a valid-looking wheel from reaching PyPI with misspelled or retired
discovery metadata.

The pip-audit gate exports every dependency reachable through a published
Python extra from the exact `uv.lock`, including hashes, while omitting the
unpublished local Cindergraph project. `--strict` then makes dependency
collection failures fatal instead of silently skipping them. Advisory results
remain point-in-time evidence and must be rerun for the release commit.

## Verify after publication

Do not stop at a successful upload step:

1. fetch the exact crate version from crates.io in a new Rust consumer and run
   the README example;
2. install `cindergraph==X.Y.Z` from PyPI in clean supported Python/platform
   environments and run `tools/smoke_installed.py` against downloaded wheels;
3. verify PyPI metadata, wheel tags, license files, stubs, `py.typed`, hashes
   and attestations;
4. verify docs.rs built the intended crate and renders public links;
5. create the GitHub release from the same tag with changelog notes and artifact
   hashes;
6. update [support and evidence](support-and-evidence.md) with the exact remote
   runs and registry URLs.
7. after the bootstrap crate release, complete the crates.io trusted-publishing
   migration above and revoke the first-release API token before another tag.

Report partial publication plainly. A crates.io success followed by a PyPI
failure is not a complete release and must not be hidden by rerunning with a
different source commit under the same version. The checksum guard permits a
rerun only from byte-identical crate contents.
