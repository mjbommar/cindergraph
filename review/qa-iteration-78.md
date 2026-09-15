# QA iteration 78: pointer-depth identity for address operands

Iteration 77 distinguished direct pointer copies from dereference loads, but a
fully local and resolvable chain such as `int **q = &p; int *r = *q` still
reported incomplete memory. The resulting value flow was correct, masking an
extra points-to edge at the wrong pointer depth.

A focused red probe showed that the name in `q = &p` was processed twice: once
correctly as the address target `p`, and again incorrectly as an ordinary value
copy. The latter copied `p`'s target into `q`, so the solver believed `q` might
also address the underlying object. Direct-copy constraint construction now
excludes reads whose source spans are address operands. Address-taking remains
represented solely by the existing address constraint.

The Rust regression requires the known local double-pointer load to preserve
parameter-to-return flow and report complete memory. The permanent Python
regression generates 100 deterministic programs with two to eight local
objects, a random initial target and one to six direct pointer aliases after
the load. Every case requires exact parameter-to-return flow and complete
memory and summary results. A separate 2,000-program exploratory population of
known load/copy chains passed the same oracle. The second-order mutation case
from iteration 77 continues to retain its concrete new target while reporting
incomplete; this change does not broaden that bounded model's completeness
claim.

Seven same-interpreter benchmark invocations compared the exact QA77 release
wheel with the current release editable extension on the 512-link pointer-copy
workload. `data_flow` medians were 2.656333 and 2.638280 ms;
`call_summaries` medians were 2.013622 and 1.997346 ms. These small favorable
movements are treated as noise rather than a performance claim.

## Validation and refreshed artifacts

- The full Python/design population passed 3,597 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 608 core tests, one
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
| `target/package/cindergraph-0.1.0.crate` | 604,477 | `6bb799758f55c59f7445b0d3867d4cb2bbd18112fa4f9c67c773eddd62e715bd` |
| `target/qa78-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,225,233 | `cea6d1ed5f32a13b312d6c1c768a701f3bead52c5db7adc6e76446b0029214df` |
| `target/qa78-a/cindergraph-0.1.0.tar.gz` | 658,632 | `17f94038315a4731decae23fda953413dddfaafc3a85b0eca962cba338f02192` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
