# QA iteration 68: fail-closed generic selection in array bounds

C11 `_Generic` exposed another false dependency in opaque array suffixes:

```c
int f(int n) {
    int values[_Generic((n), int: 4, default: 8)];
    return sizeof(values);
}
```

The controlling expression is unevaluated, but the old recovery reported `n`
as a runtime use and produced a false parameter-to-return flow. It also
reported identifiers from every association expression even though exactly one
association is selected.

Array-bound recovery now excludes the controlling expression and every
association from definite reads. Selecting an association requires C type
compatibility beyond this token-level layer, so `vla_complete` is false and a
negative reachability query returns `Unknown`. This avoids both invented
positive flows and falsely conclusive negatives.

Regression tests cover constant associations, a read in either arm, reads in
both arms, summary completeness and the public three-valued reachability API.
GCC 15.2 accepted the tested forms under C11 with warnings as errors.

## Performance evidence

`tools/bench_analysis.py --shape generic-vla` retains 32, 128 and 512 generic
array bounds. At 512 bounds, comparing the QA 67 wheel with the current release
wheel gave these local medians:

| Operation | QA 67 | Current |
| --- | ---: | ---: |
| `analyze` | 1.205 ms | 1.386 ms |
| `data_flow` | 3.298 ms | 2.992 ms |
| `call_summaries` | 2.558 ms | 2.411 ms |

The parser-only difference is run noise because this change is downstream of
`analyze`. The data-flow result removes 512 invented reads and is about 9%
faster on this workload. These are medians on one shared machine, not
cross-machine guarantees.

## Validation and refreshed artifacts

- Focused Rust and Python generic-selection regressions passed.
- The Rust workspace passed 600 core tests, one README integration test, three
  binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,071 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs, Actionlint and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, the custom
  distribution checker, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 600,816 | `c3d147fc49a113bce9e51075c45596a5ce227f4b1c5545518ad695de284e0b62` |
| `target/qa68-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,249,515 | `df71706598b8841339597c110d646378648bb51c3d32a76ca59048ec182d747f` |
| `target/qa68-a/cindergraph-0.1.0.tar.gz` | 654,267 | `70bc0fe6b827f6091e1d34ae9f59b00b7d34e1d31d3c405f7b816de15afa9737` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The packaged crate passed its 600 library tests, README test,
two doctests and strict rustdoc, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
