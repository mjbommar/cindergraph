# QA iteration 69: VLA-bound writes and reduced node transfers

Side-effect probing found that opaque array suffixes treated assignment targets
as reads and emitted no write:

```c
int f(int n) { int values[n = 4]; return n + sizeof(values) * 0; }
```

The old graph left the parameter definition reaching the return and produced a
false parameter flow. Direct `=`, compound assignments and prefix/postfix
increments in array bounds now emit definitions. A plain assignment removes
the target read; read-modify-write forms retain it. `n = m` therefore kills
parameter `n` provenance and transfers parameter `m` provenance to a later
read of `n`.

These writes take effect at the end of the opaque bound. Arbitrary comma and
nested side-effect ordering is not reconstructed, so `vla_complete` remains
false for every recovered bound write rather than claiming exact sequencing.
GCC 15.2 accepted the tested forms after the declared VLA was referenced to
avoid an unrelated unused-variable warning.

## Solver performance

The new stress shape exposed quadratic work in the reaching-definition
transfer: every strong write on one CFG node repeatedly cleared every sibling
definition of the same binding. The solver now precomputes the last strong
write per node and binding plus weak writes that survive after it. All events
remain available for same-node `effect_at` ordering; only the node's final OUT
state is reduced.

`tools/bench_analysis.py --shape assignment-vla` retains 32, 128 and 512
successive writes. Comparing release wheels before and after the reduced
transfer at 512 writes gave:

| Operation | Before | After |
| --- | ---: | ---: |
| `data_flow` | 2.601 ms | 2.343 ms |
| `call_summaries` | 1.791 ms | 1.537 ms |

That is about 10% and 14% faster respectively on this workload. These are
local medians on a shared machine, not cross-machine guarantees.

## Validation and refreshed artifacts

- The focused data-flow population passed 74 Rust tests; focused Python bound
  and wide-parameter coverage passed 54 tests.
- The Rust workspace passed 601 core tests, one README integration test, three
  binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,076 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs, Actionlint and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, the custom
  distribution checker, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 601,931 | `c2868658c6471b3f30b7516b0583f439d6617b546df1d5e7b84989c63e5317ae` |
| `target/qa69-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,251,943 | `2345c4d918e5a75a2f11770fa1bce57952d6549877e0305f1fd0d42c1c5fd92f` |
| `target/qa69-a/cindergraph-0.1.0.tar.gz` | 655,657 | `fe5f84f4a592bc5fd9d235d9e6a7020fac5f0059bf63f68797dc7d96c0d19202` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The packaged crate passed its 601 library tests, README test,
two doctests and strict rustdoc, with one external-corpus test ignored.

A preliminary known-pointee probe appeared to reproduce an ordering defect, but
it selected the last appended projected dereference use rather than the
source-last return use. Selecting by byte span showed the intended behavior in
both wheels: weak-before-strong leaves only the later strong assignment, while
strong-before-weak conservatively retains both alternatives. The public event
tables use stable analysis order rather than source order; the next iteration
pins and documents that contract.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
