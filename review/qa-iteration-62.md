# QA iteration 62: parameter type-name identity without quadratic scans

Ad hoc parameter QA found that `struct point`, `union cell` and `enum mode`
were incorrectly recovered as value parameters when the aggregate parameter
was unnamed. A second case, `typedef unsigned long size_t; int f(size_t)`,
likewise recovered the known type name as a binding.

Parameter recovery now keeps aggregate tags out of the value namespace and
uses preceding file-scope typedef declarations to disambiguate type-only
parameter groups. Visibility is source ordered: a later typedef has no effect,
every direct declarator in a comma-separated typedef is indexed, and legal
parameter names such as `int size_t` and `size_t size_t` still bind and shadow
the typedef.

This remains deliberately narrower than a C symbol table. Included typedefs
are unavailable, and block-scope typedef declarations and complete typedef
semantics are not resolved.

## Rejected implementation and performance evidence

The first correct implementation rescanned all preceding translation-unit
roots for each function and did so once for binding recovery and once for type
recovery. The retained implementation builds one translation-unit index from
typedef name to earliest declaration offset and shares it across all function
analyses.

`tools/bench_analysis.py --shape parameter-typedefs` preserves an adversarial
workload with one typedef and one function per element. Local CPython 3.14.3
release-extension medians were:

| Elements | QA 60 wheel | Rejected scan | Indexed implementation |
| ---: | ---: | ---: | ---: |
| 32 | 0.2228 ms | 0.3885 ms | 0.2095 ms |
| 128 | 0.9069 ms | 4.3988 ms | 0.8000 ms |
| 512 | 3.6513 ms | 73.7744 ms | 3.4774 ms |

At 512 elements the rejected design was 20.2 times slower than the QA 60
wheel. The retained design is 4.8% faster than that baseline on this sample.
These are local medians on a shared machine, not cross-machine throughput
guarantees.

## Validation and refreshed artifacts

- The focused Rust data-flow suite passed 64 tests.
- The Rust workspace passed 591 core tests, one README integration test,
  three binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,043 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature and no-default-feature tests, Clippy, strict
  rustdoc, RustSec, Cargo Deny, Ruff, ty, generated stubs, actionlint 1.7.12
  and `git diff --check` passed. The installed actionlint 1.7.4 still reports
  the two false unknown-runner errors documented in QA 58.
- Cargo publish dry-run, deterministic double wheel/sdist builds, custom
  distribution checks, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 597,397 | `b8a18675bca6136d6a16f9f3d0d5ffaf8eb6cbcff8d2a007c72463a4f7f8a8f8` |
| `target/qa62-c/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,244,067 | `10a8664e2fa9e7d040e2aa1ef1fe785b5671a9b5c4f5629a26e61c54762f2c7b` |
| `target/qa62-c/cindergraph-0.1.0.tar.gz` | 650,010 | `dfdd1808ef3033bc10f524d516e6a7354e17f2570557e3ab8b4489921031b59c` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The unpacked crate passed its 591 library tests, README test
and two doctests, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
