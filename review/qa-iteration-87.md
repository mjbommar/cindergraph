# QA iteration 87: discarded pointer-assignment effects

The QA86 must-initialization work was extended across switches, early returns,
gotos, short-circuit expressions, conditional expressions and same-source-order
comma expressions. Eight of nine initial probes classified correctly. The
counterexample was:

```c
int f(int x) { int y = 0; int *p; (p = &y, 0); *p = x; return y; }
```

The CFG and must-analysis correctly placed the assignment before the indirect
store. The points-to pass nevertheless removed `&y`: it treated the comma's
discarded left *value* as though side effects inside that operand were also
discarded. The pointer assignment is now interpreted relative to its enclosing
discarded region, so its address, copy and load sources remain visible.

This does not weaken the pointer-arithmetic boundary. When a pointer assignment
itself occurs in a discarded expression, its complete value-producing span is
checked conservatively for arithmetic. Thus `((p = q + 1, 1), 0)` remains
`memory_complete: false`; discarding the assignment's result cannot make an
unsupported pointer value safe. Deeply nested expressions may consequently be
classified incomplete even where a more path-sensitive expression analysis
could prove the arithmetic irrelevant. A true completeness flag remains a
bounded local-model statement, not a general C soundness certificate.

Rust, Python and exact installed-artifact regressions pin the corrected known
target and the arithmetic counterexample. A 2,000-program structured oracle
covered nested comma wrappers, future assignments, short-circuit paths and
both conditional arms without a mismatch. The original nine-case matrix also
passed after the repair, including total/partial switches and goto paths.

## Performance

Nine single-call samples on CPython 3.14 compared the exact QA86 and QA87
wheels using repeated discarded assignments `(p = &y, 0)`. QA86 produced the
incorrect incomplete verdict; QA87 produced the corrected complete verdict.

| Assignments | QA86 ms | QA87 ms |
| ---: | ---: | ---: |
| 128 | 0.575 | 0.496 |
| 512 | 3.065 | 2.815 |
| 2,048 | 25.195 | 25.247 |
| 4,096 | 89.906 | 89.627 |

The change introduces no measured regression on this targeted workload. The
shape remains visibly superlinear at large sizes and is a useful future event-
collection/profile target; this iteration does not claim to fix that scaling.

## Validation and refreshed artifacts

- The full Python/design population passed 3,663 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 614 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace
  rustdoc, Ruff, ty, generated stubs, Actionlint 1.7.12, RustSec, Cargo Deny and
  `git diff --check` passed.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 609,160 | `acd0c8629a15e254e265ceca621100c1113a934922e12839f5d5c0c8af695a22` |
| `target/qa87-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,242,913 | `68ddf674be2f6ce4d1844c3613c27daa8eeb7e5b8cf38801efd42bf01444e3ce` |
| `target/qa87-a/cindergraph-0.1.0.tar.gz` | 663,444 | `60811b8e147668cc632a59ec643f66084584ba46451a1a91d7626268272a6e1d` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
