# QA iteration 86: pointer-copy worklist and definite initialization

The flow-insensitive local points-to closure revisited every copy constraint on
every growth round. Ordinary declaration-order chains happened to settle in
one pass, masking a quadratic case: reverse-ordered constraints needed one
complete pass per edge. Direct copy constraints now use a dependency worklist;
only destinations of a target set that actually grew are revisited.
Dereference-load constraints retain their conservative outer iteration because
they depend on both the source's targets and the targets of the pointers it may
name.

The adversarial workload also exposed a correctness defect. Flow-insensitive
targets learned from a later assignment could be projected through an earlier
read of an uninitialized pointer, and `memory_complete` could remain true. A
dense must-analysis now checks whether pointer storage is initialized on every
CFG path to each relevant read. It fails closed for a future-only assignment,
a one-arm conditional and a zero-trip loop, while retaining completeness for a
straight-line assignment, assignments in both conditional arms and a do-while
assignment. The must lattice contains only pointer bindings lacking parameter
or declaration initialization, so established workloads pay no unnecessary
fixed-point cost.

Rust and Python regressions pin all six control-flow cases, and the installed
artifact smoke test covers both one-arm and both-arm initialization. A
2,000-program independent path oracle matched those classifications. For
definitely initialized pointer graphs, 4,000 generated cyclic and fan-in/fan-
out programs produced byte-identical serialized data-flow and call summaries
under the exact QA85 wheel and the final extension, with digest
`a68c2994c2052face2de8ae47521e5f7cd02045a2fb879f0a7b3ded049ba70fb`.

## Performance

Seven single-call CPython 3.14 samples compared the exact QA85 and QA86 wheels
on the durable `pointer-reverse-copies` workload:

| Constraints | Operation | QA85 ms | QA86 ms | Speed-up |
| ---: | --- | ---: | ---: | ---: |
| 512 | `data_flow` | 6.199 | 2.888 | 2.15x |
| 512 | `call_summaries` | 4.906 | 2.883 | 1.70x |
| 1,024 | `data_flow` | 18.773 | 8.071 | 2.33x |
| 1,024 | `call_summaries` | 17.189 | 6.941 | 2.48x |
| 2,048 | `data_flow` | 66.812 | 22.727 | 2.94x |
| 2,048 | `call_summaries` | 63.475 | 19.540 | 3.25x |
| 4,096 | `data_flow` | 257.034 | 78.649 | 3.27x |
| 4,096 | `call_summaries` | 245.100 | 70.210 | 3.49x |

This malformed/undefined-use-shaped workload is intentionally adversarial:
QA86 now reports its memory result incomplete. It remains useful as a totality
and denial-of-service scaling probe. The regular initialized pointer-result
and repeated-access workloads remained within ordinary run-to-run variation.

## Validation and refreshed artifacts

- The full Python/design population passed 3,659 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 613 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint 1.7.12,
  RustSec, Cargo Deny and `git diff --check` passed.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 608,807 | `14cc1c13dbf3504a56d333ebdfb7344da621413c16ea8e9b9c625b94830f8675` |
| `target/qa86-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,242,816 | `a4d9eabdc2dd5c041ee5d777193f68c565ab40d3ea02013039b2442f4f4eb274` |
| `target/qa86-a/cindergraph-0.1.0.tar.gz` | 663,016 | `49aefe0aec8ae91e0430cac367aa935c42f92bf2939ff67278498e66cfbc6209` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
