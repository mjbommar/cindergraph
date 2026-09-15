# QA iteration 75: fail closed after pointer arithmetic

Ad hoc read-modify-write probes found a completeness error in local points-to
tracking. After `p = p + 1`, the flow-insensitive target set correctly retained
the original local as a *possible* target, but the analysis still reported
`memory_complete: true`. A later `*p = value` was therefore presented as a
complete local-memory result even though pointer arithmetic had changed the
concrete address. Declarations such as `int *q = p + n` had the same defect.

A red regression reproduced both forms before the repair. Pointer definitions
whose expression contains `+`, `-`, `++` or `--` now taint that pointer as
unknown. Known targets remain in the may-alias set, preserving conservative
edges, but `memory_complete` and interprocedural summary completeness become
false. Unknownness continues to propagate through later pointer copies.

The classifier scans only tokens whose monotone source starts lie inside the
definition expression. Two `partition_point` searches avoid a whole-token-buffer
scan per pointer definition and preserve iteration 73's wide-copy scaling.
Harmless pointer copies, parentheses and casts do not contain an arithmetic
token and remain complete.

An exploratory population generated 2,000 pointer-arithmetic assignments and
declarations using nested parentheses, both operators, known addresses and
pointer copies; every result failed closed. A matched 500-program copy/cast
control population remained complete. The permanent Rust test covers direct
assignment, derived-pointer declaration and postfix increment propagation;
Python covers three public-API forms. The installed-artifact smoke procedure
now checks the direct arithmetic case.

## Performance check

Seven benchmark invocations compared the exact QA74 release wheel with the
current release editable extension under the same CPython 3.14.3 interpreter.
For a 512-link pointer-copy chain, median-of-medians results were:

| Operation | QA74 | QA75 | Difference |
| --- | ---: | ---: | ---: |
| `analyze` | 0.683771 ms | 0.672883 ms | -1.6% |
| `data_flow` | 2.627780 ms | 2.624711 ms | -0.1% |
| `call_summaries` | 1.979618 ms | 1.995080 ms | +0.8% |

The small mixed movements are treated as noise, not as a performance win or
regression. They establish that the local token classifier did not restore the
obvious quadratic scan caught during implementation review.

## Validation and refreshed artifacts

- The full Python/design population passed 3,495 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 605 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint, RustSec, Cargo
  Deny and `git diff --check` passed.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 603,360 | `d60fd2acaa95125e6675876b7688c0cc96ae278fcfaa06d145d019c187943180` |
| `target/qa75-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,218,564 | `bec5443789270f866ca95e447d0c37aa26170d7086ca0a90251273d1633d8fbf` |
| `target/qa75-a/cindergraph-0.1.0.tar.gz` | 657,506 | `8bfcaee20051c01288e68af4247ba1d49c007d29f0d1581bd80af9e12090dc0e` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
