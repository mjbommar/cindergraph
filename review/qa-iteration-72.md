# QA iteration 72: generated pointer-write ordering coverage

The two-case regression from iteration 70 established the endpoints of local
pointer-write ordering, but it did not exercise longer mixtures. A generated
oracle now checks the safe semantic property for straight-line code where a
local pointer is known to address `x`: the concrete final write must be among
the definitions reaching the source-last return read.

The assertion intentionally allows extra reaching definitions. Indirect writes
are weak because the local points-to set is a may-alias model; demanding an
exact singleton would test a stronger and unsound contract. Each case locates
the return use and expected definition by byte span rather than assuming event
tables are in source order.

Rust exhaustively covers all 256 direct/indirect orderings across eight writes.
Python adds 200 reproducible random programs containing between one and thirteen
writes. Before committing the bounded populations, an exploratory 1,000-seed
run also passed.

## Validation and refreshed artifacts

- The focused Rust generated test passed all 256 internal cases in 0.11 seconds;
  the Python provenance module passed 602 cases in 0.68 seconds.
- The Rust workspace and exact packaged crate each passed 603 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- The full Python/design population passed 3,287 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, no-default-feature checking, Clippy, strict rustdoc,
  RustSec, Cargo Deny, Ruff, ty, generated stubs, Actionlint and
  `git diff --check` passed.
- Cargo 1.88 publish dry-run, deterministic double normalized wheel/sdist
  builds, crate notices, the custom distribution checker, Twine and
  check-wheel-contents passed. The exact sdist rebuilt and passed on CPython
  3.12.13.
- The normalized wheel is byte-identical to iteration 71, so the prior exact
  CPython 3.12.13, 3.13.12 and 3.14.3 smoke results address the same bytes.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 602,382 | `048ba34745166af7ac06881e02969f683d9b1cbb76aea93735542ce47d56e478` |
| `target/qa72-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,205,705 | `0e956ee9f08f96ab7fa0fee929d924368d99cde3479d0331c18e8b683ff6d39b` |
| `target/qa72-a/cindergraph-0.1.0.tar.gz` | 656,466 | `307d7e7377ae92726813dba875057ba59aaaad6e36b85b2f3196fa9f23831024` |

The second normalized wheel and sdist were byte-identical. The Linux wheel
remains a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
