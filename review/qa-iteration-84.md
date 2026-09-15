# QA iteration 84: compact value lattice

Iteration 83 removed address-taking events from value flow while preserving
their public definition indices. The fixed-point storage still used the
largest public definition index as its bit width, however, so excluded escape
events continued to occupy empty words in every node's `IN` and `OUT` set.

The solver now builds a dense internal mapping for value definitions and
memory writes. Public indices remain stable in definitions, edges and Python
output; only the private bit position changes. On the 1,024-initializer
`pointer-results` workload this reduces each lattice set from 49 `u64` words
to 17 while retaining all 3,077 public events. At 512 initializers it reduces
25 words to nine, and at 128 it reduces seven to three.

The compact representation includes the zero-word case. A new permanent Rust
regression analyzes `int x; (void)&x` and verifies that its sole definition-
shaped event is `AddressTaken`, with no value edge, dead store or unused
binding. A separate 4,000-program mixed-pointer differential produced the same
serialized data-flow and call-summary digest under the exact QA83 wheel and
the final editable extension:
`1744665d41871615356427876638d80e7af177f09ce71e0a96955d419ecbd28a`.

## Performance

Seven groups of five same-interpreter calls compared the exact QA83 CPython
3.14 wheel with the release-mode editable extension. At 128 initializers,
median `data_flow` time moved from 1.314 to 1.140 ms and `call_summaries` from
1.175 to 0.979 ms. At 512 and 1,024, medians moved by less than three percent
and should be treated as measurement noise. The durable gain is the 57--65%
reduction in per-node lattice width, not a claimed broad end-to-end speed-up;
parsing, pointer constraint recovery and result construction now dominate this
workload.

## Validation and refreshed artifacts

- The full Python/design population passed 3,653 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 612 core tests, one
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
| `target/package/cindergraph-0.1.0.crate` | 606,850 | `472ad7de7932a6d6a853da612a5cb404c8405cd6b13ea41036880ee0f6465950` |
| `target/qa84-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,235,533 | `578c66ef7705fa58a64780d03ff3b7c8af2450a0eb4e626bdd6f240362e5275a` |
| `target/qa84-a/cindergraph-0.1.0.tar.gz` | 660,989 | `aaf508c5149ca7f2542cd41ed60ed4b1baa4fad80a775af631d01f13550cbbbd` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
