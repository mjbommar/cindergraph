# QA iteration 66: parenthesized parameter VLA declarators

Compiler-oracle probing found that QA 65's parameter-bound recovery was still
too narrow. GCC 15.2 accepts all of these under C11 with warnings as errors:

```c
int f(int n, int (values[n]));
int f(int n, int ((values[n])));
int f(int n, int (*values)[n]);
int f(int n, int (*values[n])(void));
```

The first, second and fourth forms previously omitted the runtime use of `n`.
The recovery treated every parenthesis as evidence of a nested function
signature. It now distinguishes grouping opened before the outer parameter
name from signatures opened after it. Parentheses inside an active bound remain
part of the bound, while `void (*callback)(int nested[n])` remains isolated.

Regression tests cover the four accepted forms, the ordinary and `static`
forms, multidimensional bounds, a later parameter that is not yet in scope,
and a nested callback signature that must not leak an outer use.

## Performance evidence

`tools/bench_analysis.py --shape parenthesized-parameter-vla` retains the new
shape. A release-wheel comparison on CPython 3.14.3 measured these medians at
512 array parameters:

| Operation | QA 65 artifact | Current |
| --- | ---: | ---: |
| `analyze` | 0.430 ms | 0.457 ms |
| `data_flow` | 1.193 ms | 1.512 ms |
| `call_summaries` | 0.685 ms | 0.934 ms |

The current result contains 512 bound uses that QA 65 omitted. The additional
`data_flow` time is about 0.62 microseconds per newly represented use. An
initial correct implementation allocated a parenthesis stack per grouped
parameter and measured 1.929 ms for `data_flow`; replacing it with bounded
counters reduced that result by about 22%. These are local medians on a shared
machine, not cross-machine guarantees.

## Validation and refreshed artifacts

- Focused Rust and Python regression tests passed, including all compiler-
  accepted declarator spellings.
- The Rust workspace passed 598 core tests, one README integration test, three
  binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,057 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs, Actionlint and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, the custom
  distribution checker, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 599,123 | `6880eebf928afb2cb97482d90b9cec38db3bcf5009cd5e1b9c0cd357e0668fe4` |
| `target/qa66-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,247,428 | `5f837558248585784ed3cf1a4fee055cfca16c39921c3c8f30455bfb488efe97` |
| `target/qa66-a/cindergraph-0.1.0.tar.gz` | 652,133 | `b57d0bcd670aecc32652d085876b56925e67da71933a64974cd59001e96943fe` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The packaged crate passed its 598 library tests, README test,
two doctests and strict rustdoc, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
