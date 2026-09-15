# QA iteration 51: standalone Rust documentation and refreshed artifacts

Made the public Rust documentation usable outside the extraction repository.
The crate root now explains the supported analysis surface, API selection and
assurance boundary, with runnable metrics and CFG-export examples. Public
module documentation no longer links to absent internal design files or embeds
personal cache paths, and an obsolete claim that parity flag derivation was not
wired has been corrected. Strict rustdoc and both crate-root doctests pass.

This iteration supersedes iteration 50's artifact hashes because Rust source
documentation is part of the packaged source and the release candidates were
rebuilt after those edits. No analysis semantics were intentionally changed.

## Exact artifact evidence

Baseline: commit `edf2777` plus all pending QA changes through this iteration.
This remains a dirty, uncommitted release candidate rather than a selected
release snapshot.

`cargo +1.88.0 publish -p cindergraph --dry-run --locked --allow-dirty` passed
and aborted before upload as intended. Cargo packaged 301 files, 2.1 MiB
uncompressed and 579.6 KiB compressed. LICENSE and NOTICE in the unpacked
package match the repository files byte-for-byte.

| Artifact | SHA-256 |
| --- | --- |
| `target/package/cindergraph-0.1.0.crate` | `e7fff927be7a966b4a30b1def6164ec2c97bbc4a6d5c0a999f8505db4ab7beca` |
| `target/qa-release-54/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | `5eeb9b71fe72d2e9b743010a9cc5e19920d6a9a6815b3b751a85403979a5f621` |
| `target/qa-release-54/cindergraph-0.1.0.tar.gz` | `20d9d8795ee1258b32c570b23eb530f8cfcf50b05f0254f6ba992465c495c60e` |

Twine accepted the wheel and sdist metadata and long descriptions. The exact
wheel installed without dependencies and passed the isolated ownership smoke
test on CPython 3.12.13, 3.13.12 and 3.14.3. The exact sdist built through its
PEP 517 path, installed without runtime dependencies and passed the same smoke
test on CPython 3.12.13. The environments remain under
`/home/mjbommar/.cache/cindergraph/qa54.yi5Ay8`.

Tests from Cargo's unpacked package passed: 576 library tests, one README
integration test and two doctests passed; one library test was ignored.

## Current local gates

- Rust workspace: 582 passed and one ignored (576 core, one README integration
  test, three binding tests and two doctests).
- Python plus review contracts: 2,942 passed, ten skipped and three optional
  Joern cases deselected.
- Rust 1.88 formatting, Clippy with warnings denied, strict rustdoc, doctests
  and the no-default-features core check: passed.
- Ruff, ty, generated-stub consistency, Twine, diff whitespace and actionlint
  with the two known local runner-label exceptions: passed.
- The editable release extension was rebuilt before the complete Python and
  review suite.

## What this does not prove

Only Linux x86-64 artifacts ran locally. The wheel requires glibc 2.34; no
older-glibc or musllinux claim follows. Configured Linux AArch64, macOS and
Windows jobs have not run remotely. No registry name was reserved, protected
environment or secret was configured, release snapshot was committed, or
crate, wheel, tag, push or GitHub release was published. The known
initializer-boundary type-recovery defect from iteration 49 remains. These are
locally validated candidates, not approved release artifacts.
