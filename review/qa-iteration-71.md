# QA iteration 71: relocatable wheel SBOMs

An exact source-distribution rebuild exposed a release-quality defect that the
same-checkout reproducibility gate could not see. With a fixed
`SOURCE_DATE_EPOCH`, the native extension and ordinary package files matched,
but the wheel differed because Maturin 1.15's cargo-cyclonedx SBOM embedded the
absolute checkout directory in `bom-ref` and dependency `ref` values. Besides
breaking relocation reproducibility, a published wheel would disclose its
builder's filesystem path.

This behavior follows the current upstream tools: Maturin enables a generated
Rust CycloneDX SBOM by default, and cargo-cyclonedx 0.5.9 deliberately makes
the manifest path absolute while using `SOURCE_DATE_EPOCH` only for timestamp
and serial-number reproducibility. See the [Maturin SBOM
guide](https://www.maturin.rs/sbom) and [cargo-cyclonedx 0.5.9
changelog](https://github.com/CycloneDX/cyclonedx-rust-cargo/blob/main/cargo-cyclonedx/CHANGELOG.md).

`tools/normalize_wheel_sbom.py` now replaces path-based local-package IDs with
stable `pkg:cargo/<name>@<version>` IDs, rewrites every matching dependency
reference, regenerates wheel `RECORD`, and atomically recreates the archive.
It is idempotent. CI applies it before structural checks; the release workflow
applies it to both wheel builds before comparing hashes. The distribution
checker now rejects any surviving `path+file://` SBOM value.

The operator documentation includes normalization in the required order, and
Ruff formatting/lint coverage now includes `tools/` as well as `python/`.

## Evidence

- Before normalization, checkout and exact-sdist wheel builds had identical
  native extensions but different SBOM and `RECORD` files; the wheel sizes
  differed by 16 bytes.
- After normalization, those builds were byte-identical despite different
  absolute source directories. Re-running normalization did not change the
  wheel hash.
- The normalized SBOM passed the official CycloneDX 1.5 JSON schema. Its 29
  component references and 28 dependency rows were unique, every dependency
  endpoint resolved, and no path-based identifier remained.
- The full Python/design population passed 3,087 tests, skipped ten and
  deselected three optional Joern cases.
- Focused distribution, release-metadata and documentation coverage passed 38
  tests. Ruff, ty, generated stubs, Actionlint and `git diff --check` passed.
- The custom distribution checker, Twine, check-wheel-contents and auditwheel
  accepted the normalized candidate. Its exact wheel passed isolated CPython
  3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and passed
  on 3.12.13.
- On 2026-09-14, the PyPI JSON endpoint and the crates.io crate API/index all
  returned 404 for `cindergraph`; the GitHub repository API returned 200.
  This is point-in-time availability evidence, not ownership or reservation.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 602,123 | `0a1c89b9cf42e489ea243572c3023cde5b072a7d0aa31d01ba45c0dd60cb7693` |
| `target/qa71-current-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,205,705 | `0e956ee9f08f96ab7fa0fee929d924368d99cde3479d0331c18e8b683ff6d39b` |
| `target/qa71-current-a/cindergraph-0.1.0.tar.gz` | 656,234 | `7fdb813ff584ca5fc32deec8dab8254c06b8501e9684e92d4daccc6369c0a423` |

The second normalized wheel and sdist were byte-identical.

The Linux wheel remains a local manylinux 2.34 candidate, not the release
workflow's manylinux 2.17 artifact. Baseline remains commit `edf2777` plus
pending QA changes. No commit, push, workflow dispatch, tag, registry claim or
publication was performed.
