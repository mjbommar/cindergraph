# QA iteration 80: result-sensitive pointer-cast provenance

Iteration 79 rejected literal and purely scalar integer-to-pointer casts, but
its “any known source” rule was still too weak for a cast whose operand mixed
source kinds. In `int *p = (int *)(x ? &a : x)`, seeing `&a` was enough to
classify the entire cast as locally known even though the other value-producing
arm was the scalar parameter `x`. A later indirect store was consequently
reported complete.

Pointer-cast classification now follows expression result semantics with an
iterative worklist. Both value arms of a conditional must be pointer-known;
only the final operand of a comma expression contributes; parentheses and
non-pointer cast wrappers are transparent; address expressions and resolved
pointer names are known; scalar names, literals and unsupported expression
shapes are not. Direct-name dereference loads remain delegated to the existing
points-to constraints. This makes the mixed scalar case fail closed without
discarding completeness for `(int *)(x ? &a : &b)`, `(int *)(x, &a)` or
conditionals between known pointer variables.

The implementation remains non-recursive and retains iteration 79's nested
pointer-cast shortcut. In a 2,048-cast stress case, QA79 and QA80 measured
approximately 1.226 and 1.213 ms respectively. Two generated 2,000-program
populations composed up to six levels of parentheses, pointer casts, comma
expressions and conditionals. Every expression whose result alternatives were
known addresses or pointers remained complete; every expression with a scalar
result alternative was incomplete.

Seven same-interpreter benchmark invocations compared the exact QA79 release
wheel with the final release editable extension on the 512-link pointer-copy
workload. `data_flow` medians were 2.606812 and 2.650328 ms;
`call_summaries` medians were 1.970900 and 2.012918 ms. The roughly 1.7–2.1%
unfavourable movements are treated as run noise rather than a performance
claim; the workload contains no casts and the accepted implementation adds no
separate tree or token walk when no casts are present.

## Validation and refreshed artifacts

- The full Python/design population passed 3,649 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 609 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint 1.7.12,
  RustSec, Cargo Deny and `git diff --check` passed. Ruff requested one
  mechanical smoke-test line wrap after the semantic suites passed; all
  affected formatting and static gates were rerun successfully.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 605,453 | `c759385390c61776fe77ab1b82b48725d66903514f9c18c249fda25e3fe3e95f` |
| `target/qa80-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,226,896 | `68c086a656d8027c98f43f140d2ffccfd6c1f8eb9e7b4d9209f83631fb945f43` |
| `target/qa80-a/cindergraph-0.1.0.tar.gz` | 659,610 | `41bb6a6be086ab5f762afbcaa4e9404f4ab6e139f65f805339741962b5ef6620` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
