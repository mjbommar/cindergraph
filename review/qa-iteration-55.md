# QA iteration 55: PyO3 security upgrade and dependency policy

Independent release checks found two RustSec advisories in the locked PyO3
0.26.0 binding dependency:

- `RUSTSEC-2026-0176`, an out-of-bounds read in list/tuple iterator `nth` and
  `nth_back` implementations;
- `RUSTSEC-2026-0177`, a missing `Sync` bound on `PyCFunction::new_closure`.

Both advisories identify PyO3 0.29.0 as the fixed floor. The workspace now
requires PyO3 0.29 and resolves 0.29.2. The binding code compiled without API
changes. The upgrade also removed `autocfg`, `indoc`, `memoffset`,
`rustversion`, and `unindent` from the lockfile, reducing it from 33 to 28
packages in the advisory scan.

The current RustSec database contained 1,245 advisories when checked on
2026-09-14. `cargo audit --deny warnings` now passes with no vulnerability
finding. A pinned `rustsec/audit-check` action runs on ordinary CI and release
validation so the lockfile is re-evaluated as the database changes.

Added `deny.toml` with a fail-closed dependency policy:

- no ignored advisories;
- only Apache-2.0, Apache-2.0 WITH LLVM-exception, MIT, Unicode-3.0 and
  Unlicense are accepted for the current graph;
- duplicate crate versions and wildcard requirements are denied;
- only crates.io registry dependencies are accepted, with unknown registries
  and Git sources denied.

`cargo deny check` passes all advisory, ban, licence and source checks. A
pinned Cargo Deny action enforces the same policy in CI and release validation.
These are current database and lockfile results, not a claim that dependencies
can never receive a later advisory.

## Gates after the upgrade

- Rust 1.88 formatting, Clippy with warnings denied, workspace tests,
  no-default-features core check and strict rustdoc: passed.
- Rust workspace: 582 passed and one ignored.
- Rebuilt release extension plus complete Python/design suite: 2,954 passed,
  ten skipped and three optional Joern cases deselected.
- Ruff, ty, generated stub, actionlint and diff whitespace: passed.
- Cargo publication dry-run passed and stopped before upload.

## Current exact artifacts

The wheel and sdist were each packaged twice with the release epoch and were
byte-identical across their pair. The exact candidates then passed the custom
distribution checker, Twine and independent `check-wheel-contents`; auditwheel
confirmed the Linux wheel's manylinux 2.17 symbol boundary.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | — | `5a314bc1ab9ceb9846752d89998d91a06c59019703e90f17a6c8291556551472` |
| `target/qa-manylinux61/cindergraph-0.1.0-cp312-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | 1,222,614 | `95eb228de42e7221c8bde3f16284da705a2a5fcfe1de863900768fece0eff268` |
| `target/qa-release61/cindergraph-0.1.0.tar.gz` | 645,410 | `a1492542030cafcbcdac9d0bb225892f8dbd9dea6a650c63b637db179a49a346` |

The exact ABI3 wheel installed without dependencies and passed the ownership
smoke test on CPython 3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt via
PEP 517, installed without runtime dependencies and passed on CPython 3.12.13.
The exact unpacked crate passed 576 library tests, one README integration test
and two doctests, with one ignored library test; its LICENSE and NOTICE match
the repository. Environments remain under
`/home/mjbommar/.cache/cindergraph/qa61.NXEgF9`.

No remote job, clean release commit, tag, registry upload or publication
credential was created. Platform evidence outside Linux x86-64 remains absent.
