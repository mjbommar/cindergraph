# QA iteration 54: distribution integrity and reproducible packaging

Turned manual archive inspection into a mandatory pre-upload gate.
`tools/check_python_distribution.py` uses only the Python standard library and
now checks every release wheel and sdist before installation or upload.

For wheels it rejects unsafe or duplicate paths, links and special files,
missing native/stub/typing/licence/SBOM payloads, inconsistent name, version,
SPDX, Python-floor or ABI3 metadata, incomplete RECORD membership, and every
incorrect RECORD SHA-256 or size. It also validates the CycloneDX identity and
the presence of the core and PyO3 components. For sdists it rejects unsafe or
duplicate paths, links, devices, writable members, wrong roots, missing build,
source, licence, typing or documentation inputs, and leaked `.git`, `target`
or `review` trees. Path-safety unit tests cover traversal, absolute, Windows
separator and duplicate-name cases.

Both ordinary CI and the release workflow run the checker against the exact
artifacts they subsequently install. The existing ownership smoke remains a
separate runtime check; archive validity does not substitute for import
validity.

## Reproducibility finding and control

Repeated wheels previously had different hashes even though ZIP member dates
were normalized. The generated CycloneDX document carried a fresh timestamp
and UUID. Setting `SOURCE_DATE_EPOCH` removed those variable fields and made
two host wheel archives byte-identical.

The release matrix now derives `SOURCE_DATE_EPOCH` from the release commit,
explicitly forwards it into Maturin's Linux container, creates every wheel
archive twice from the same source/build cache, and compares complete
filename-to-SHA-256 maps. Publication stops on any difference. This establishes
deterministic packaging for a fixed build graph, not independently reproduced
native compilation.

The container behavior was tested directly with Maturin 1.15.0: two
manylinux2014 builds with the same epoch had identical SHA-256
`3826f09005df8c51dbb977c0353f95a88c5a6be8fb035e38d11cf02ac822d880`.
The final wheel and sdist were then each produced twice using the current HEAD
commit timestamp (`SOURCE_DATE_EPOCH=1789389576`) and compared byte-for-byte.

## Current exact artifacts

| Artifact | SHA-256 |
| --- | --- |
| `target/package/cindergraph-0.1.0.crate` | `5a314bc1ab9ceb9846752d89998d91a06c59019703e90f17a6c8291556551472` |
| `target/qa-manylinux59/cindergraph-0.1.0-cp312-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | `24f1d4a80bc6f7f756c4408b9a2e29df0037ca2e7bd1192fe542f72fe6055632` |
| `target/qa-release60/cindergraph-0.1.0.tar.gz` | `e0cb734829b0077adf878ee82607cff800c7425f10d9fbc6af4374ea216fa534` |

The distribution checker and Twine passed both Python artifacts. The exact
wheel installed without dependencies and passed the ownership smoke on host
CPython 3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt via PEP 517,
installed without runtime dependencies and passed on CPython 3.12.13.
Wheel environments remain under
`/home/mjbommar/.cache/cindergraph/qa59.Bf5pFs`; the final sdist environment is
`/home/mjbommar/.cache/cindergraph/qa60.YOwawM`.

The complete Python and design-contract suite passed 2,953 tests, with ten
skipped and three optional Joern cases deselected. Ruff, ty, generated-stub
consistency, actionlint with the two established runner-label exceptions, and
diff whitespace also passed.

## Boundary

No clean-room binary reproducibility, remote runner behavior, registry upload,
signature, attestation, tag, commit or push is claimed. The exact wheel was
locally built only for Linux x86-64; other configured targets remain untested.
The worktree remains dirty and the semantic limitations in earlier iterations
remain unchanged.
