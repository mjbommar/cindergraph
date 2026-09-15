# QA iteration 82: word-parallel reaching-definition joins

Iteration 81 removed repeated whole-definition and whole-edge scans, but the
fixed-point join still visited every definition bit separately. `Vec<bool>`
stored the lattice compactly, yet iterating it exposed one boolean at a time.
Wide recovered functions therefore paid definition-count work for every
predecessor of every CFG node on every round.

The reaching-definition lattice now uses explicit `u64` words. Predecessor
joins process 64 definition states per operation, and small checked helpers set,
clear and query individual definitions for transfers and final edge recovery.
This retains the dependency-free substrate while making its compact layout
explicit. Final use processing now borrows the selected node's live words
instead of cloning the entire live set once per use.

A permanent integration regression creates 130 definitions and verifies exact
reaching edges on both sides of the 63/64 and 127/128 word boundaries. As an
independent differential, 4,000 generated scalar programs produced byte-for-
byte identical serialized data-flow and call-summary results under the exact
QA81 wheel and the final editable extension.

On the 1,024-initializer `pointer-results` workload, the exact QA81 wheel versus
the final release editable extension measured 381.832 versus 354.583 ms for
`data_flow`, and 82.919 versus 64.678 ms for `call_summaries`. At 512, the same
measurements were 95.921 versus 93.815 ms and 21.288 versus 14.635 ms. Python
conversion of the large detailed data-flow report remains outside this native
lattice optimization. On the established 512-link pointer-copy workload,
seven same-interpreter invocations moved from 2.443117 to 2.344405 ms for
`data_flow` and from 1.788652 to 1.719634 ms for `call_summaries`.

## Validation and refreshed artifacts

- The full Python/design population passed 3,652 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 611 core tests, one
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
| `target/package/cindergraph-0.1.0.crate` | 606,198 | `f44e47dba15f6daf0fbe3405f24d06e684efa67917eef57c32bc0171b83b09b5` |
| `target/qa82-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,232,432 | `5e7663dd8a9f7f186758c42024e55effa2ccd3c690f390dc57f6f1b9cc4f0479` |
| `target/qa82-a/cindergraph-0.1.0.tar.gz` | 660,327 | `d32b6faf8b0591eb88803edcfdfc7cd41f91debe278277c03e2069a17c960dda` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
