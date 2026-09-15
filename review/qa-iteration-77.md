# QA iteration 77: pointer loads through local pointer aliases

Iteration 76 propagated targets written through a local double pointer, but the
inverse operation still confused pointer depth. In `int *r = *q`, ordinary
copy propagation gave `r` the objects directly targeted by `q`; the correct
local targets are the objects targeted by each intermediate pointer that `q`
may address. After `*q = &b`, this ordering defect could again omit a later
`*r = x` flow to `b`.

The points-to solver now records direct pointer copies and dereference loads as
different constraints. A direct copy unions `targets[source]` into the
destination. A dereference load visits each intermediate target, checks that it
is itself a pointer, propagates its unknown status, and unions that pointer's
target set. These monotone constraints are solved to closure again whenever a
second-order indirect store grows a target set, before later stores are
projected. Unknown or empty intermediate sets remain fail-closed.

The implementation discovers unary direct-name dereferences in one AST walk
and uses the existing sorted use indexes for exact binding lookup. Per-pointer
definition work is restricted to dereference spans inside that definition's
source extent, avoiding a repeated whole-function scan.

Red/green Rust and Python regressions cover a store-through-double-pointer,
load-through-double-pointer, store-through-loaded-pointer chain. An exploratory
2,000-program population varied two to seven local objects, initial and
replacement targets, and direct-address versus pointer-copy replacement. Every
program retained the concrete replacement-target flow and remained explicitly
incomplete.

Seven same-interpreter benchmark invocations compared the exact QA76 release
wheel with the current release editable extension on the 512-link pointer-copy
workload. `data_flow` medians were 2.639000 and 2.628988 ms;
`call_summaries` medians were 1.975044 and 1.998997 ms. The mixed sub-1.3%
movements are treated as noise rather than a win or regression.

## Validation and refreshed artifacts

- The full Python/design population passed 3,497 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 607 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint, RustSec, Cargo
  Deny and `git diff --check` passed. Ruff requested two mechanical line wraps
  after the semantic suites passed; the remaining gate was rerun successfully
  before packaging.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 604,437 | `58ea00b24fccab604ba4b5a5b9562c05a6e45d3f091464d8f51807b63611ada5` |
| `target/qa77-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,225,005 | `2bc9ff16bdcf7eefdc85c5deb483c6136a34cfe4a8b3d43b81729ad95dd2120f` |
| `target/qa77-a/cindergraph-0.1.0.tar.gz` | 658,584 | `326bdac1d82d64d8fc09daed2d000ba4646584b096e4c72a5d96b76a00e408c8` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
