# QA iteration 65: parameter VLA provenance and artifact refresh

Ad hoc declarator probing extended the QA 64 VLA finding to function
parameters. Valid C such as:

```c
int f(int n, int values[static n]) { return values != 0; }
```

previously omitted the runtime read of `n`. Parameter declarators now recover
array-bound uses in source order, before binding the current parameter. This
models ordinary parameter scope correctly: earlier parameters may supply a
bound, while a later parameter is not retroactively visible.

The recovery balances parentheses inside bounds, visits every rank of a
multidimensional parameter and handles pointer-to-VLA declarators. It does not
leak bounds from nested function-pointer signatures into the outer function.
Unknown bounds fail `vla_complete` rather than supporting a falsely complete
negative. GCC 15.2 accepted the direct, `static` and pointer-to-VLA forms under
C11 with `-Wall -Wextra -Werror -fsyntax-only`.

## Performance evidence

`tools/bench_analysis.py --shape parameter-vla` retains 32, 128 and 512
parameter bounds. Comparing the QA 64 wheel with the current release extension
at 512 parameters gave these local medians:

| Operation | QA 64 | Current |
| --- | ---: | ---: |
| `analyze` | 0.429 ms | 0.353 ms |
| `data_flow` | 0.902 ms | 1.405 ms |
| `call_summaries` | 0.487 ms | 0.795 ms |

The current `data_flow` result contains 512 uses that the old artifact omitted.
Its 0.502 ms increase is about 0.98 microseconds per newly represented use.
These are medians on one shared machine, not cross-machine guarantees.

## Validation and refreshed artifacts

- The focused Rust data-flow module passed 54 tests; focused Python parameter
  and `sizeof` coverage passed 68 tests.
- The Rust workspace passed 598 core tests, one README integration test, three
  binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,054 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs, Actionlint and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, the custom
  distribution checker, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 598,974 | `c7a414341e0498c30730db4cb70c0a00761d7c0ad41f9aba4e78e8aaf1311e13` |
| `target/qa65-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,247,464 | `2e185c2f9481c98e58c6f3e95dad096af216d66e9caaf357c48089004cb723d7` |
| `target/qa65-a/cindergraph-0.1.0.tar.gz` | 651,989 | `5e1d821133853da047a0a8c2b4846760781567ec600effa232891d46f0c8ea74` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The packaged crate passed its 598 library tests, README test,
two doctests and strict rustdoc, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
