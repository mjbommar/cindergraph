# QA iteration 74: parenthesized local pointer stores

Adversarial probing after iteration 73 found that the supported plain local
pointer store `*p = value` became incomplete when redundant parentheses wrapped
the lvalue: `(*p) = value`. The parser represented the assignment target as a
`paren_expr`, while memory projection required the target itself to be a
`unary_expr`. The analysis consequently omitted the concrete local write and
reported `memory_complete: false` despite having enough information.

Memory projection now strips any chain of target parentheses before classifying
the dereference. It retains the full original lvalue span for the projected
definition and still requires the dereference operand to resolve to a direct
known pointer name. This is deliberately narrow: fields, array elements,
computed pointer expressions and unresolved parameter pointers remain
incomplete.

Permanent Rust and Python regressions cover one and three target-parenthesis
layers. The Python negative population now also requires `((*p)) = x` through
an unresolved parameter pointer to stay incomplete. The installed-artifact
smoke test exercises the three-layer positive case so a stale or incorrectly
packaged native extension cannot satisfy the release checks.

An exploratory deterministic population varied zero through eight parentheses
around the pointer name and around the full dereference, plus whitespace,
newlines and comments around the assignment operator. All 2,000 known-local
programs retained complete parameter-to-return flow, while all 2,000 matched
unknown-pointer programs remained fail-closed.

## Validation and refreshed artifacts

- The focused Python pointer module passed 56 cases. The full Python/design
  population passed 3,492 tests, skipped ten and deselected three optional
  Joern cases.
- The Rust workspace and exact packaged crate each passed 604 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict rustdoc, Ruff,
  RustSec, Cargo Deny and `git diff --check` passed. The lockfile did not change
  during this iteration.
- Cargo 1.88 publish dry-run passed with `--allow-dirty`, which is required to
  package this uncommitted QA tree. An initial invocation without that explicit
  flag refused the dirty checkout and was discarded rather than counted.
- Deterministic double normalized wheel/sdist builds, crate notices, the custom
  distribution checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 602,937 | `83e474302cfb74c567602f183f1ce2d67bc4371da7243a988f4b93e8e197d2d6` |
| `target/qa74-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,218,744 | `3cb378fec7ee93726dfc9593a5251cabf08ff9b0492e1bbc4587e1fb221522f2` |
| `target/qa74-a/cindergraph-0.1.0.tar.gz` | 657,035 | `8729bdaf98f6e737ad70eb160228e40f394d544c51d01a24386d4ea92fb7ebbf` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
