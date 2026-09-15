# QA iteration 53: manylinux policy and stripped wheels

Audited the Linux wheel below the installation layer. The host-built candidate
was valid but required glibc 2.34. The release workflow used `manylinux: auto`,
which left compatibility vulnerable to changes in runner and action defaults.
Maturin's own release-hardening guidance recommends an explicit policy, a
pinned Maturin version and its PyPI compatibility check.

The Linux x86-64 and AArch64 matrix entries now explicitly target manylinux
2.17. The Maturin action is pinned to commit
`e83996d129638aa358a18fbd1dfb82f0b0fb5d3b` (the peeled `v1` tag), Maturin is
pinned to 1.15.0, Cargo locking is required for wheel builds, and
`--compatibility pypi` must pass. The sdist command deliberately omits
`--locked`: an executable local check found that Maturin 1.15.0 does not accept
that option for `sdist`.

The Python package now sets `tool.maturin.strip = true`. This removed 949,992
bytes (24.0%) from the uncompressed native extension and 134,938 bytes (9.9%)
from the compressed host wheel. The stripped extension has no `.symtab`.
This changes packaging, not analysis semantics.

## Direct manylinux evidence

Built in the official `ghcr.io/pyo3/maturin:v1.15.0` manylinux2014 container,
resolved at
`sha256:1c897d72a79c083cdea8829662f7b28e7657cf5b0d7f44fb4231e55998cf52ed`.
Auditwheel confirmed that the result is consistent with manylinux 2.17 and
uses no GLIBC symbol newer than 2.14.

The exact wheel was installed without dependencies and passed the ownership
smoke test twice across CPython 3.12, 3.13 and 3.14:

- on the host, using CPython 3.12.13, 3.13.12 and 3.14.3;
- inside the manylinux2014 container, using CPython 3.12.14, 3.13.15 and
  3.14.7.

Current distributable identities:

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | — | `5a314bc1ab9ceb9846752d89998d91a06c59019703e90f17a6c8291556551472` |
| `target/qa-manylinux57/cindergraph-0.1.0-cp312-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | 1,225,327 | `bb91bec30ca4b843d9331084c4db6d53e00e1e5a63b6e3071de6cce277732025` |
| `target/qa-release-58/cindergraph-0.1.0.tar.gz` | 645,219 | `acfab04202370b90ad6e614cbc4e728e259d8399f2db25edff4dc2179a3908e8` |

Twine accepted the manylinux wheel and sdist. The exact sdist built, installed
without runtime dependencies and passed the smoke test on host CPython 3.12.
The unpacked exact crate passed 576 library tests, one README integration test
and two doctests, with one library test ignored; its LICENSE and NOTICE match
the repository byte-for-byte. Wheel and crate environments remain under
`/home/mjbommar/.cache/cindergraph/qa57.teWdvO`; the final sdist environment is
`/home/mjbommar/.cache/cindergraph/qa58.2noNue`.

## Current checks and boundary

- Complete Python and design-contract suite: 2,947 passed, ten skipped and
  three optional Joern cases deselected.
- Release metadata, documentation, Ruff, actionlint and diff checks: passed.
- Rust code is unchanged from iteration 51's 582 passing and one ignored test.

This is strong local evidence for the Linux x86-64 wheel only. It is not a
remote workflow run and does not validate Linux AArch64, macOS or Windows.
Nothing was published, tagged, committed or pushed, and no external release
environment was configured.
