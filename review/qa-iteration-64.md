# QA iteration 64: fail-closed VLA value provenance

Ad hoc type-context probing found a false negative on valid C:

```c
int f(int n) { int values[n]; return sizeof(values); }
```

The array suffix is an opaque token run in the syntax tree, so the ordinary
name walk never saw `n`. No definition/use edge was emitted and the summary
incorrectly claimed that parameter zero did not reach the return.

Array-suffix event recovery now records identifiers that resolve to known
lexical values. The direct VLA above produces a parameter use, a reaching edge
and a parameter-to-return summary flow. Both the direct form and the equivalent
VLA-typedef form were accepted by GCC 15.2 under C11 with warnings as errors.

## Explicit incompleteness

A VLA typedef captures its bound at the typedef declaration. The present value
graph has no type-level edge that can carry that captured value through a later
`sizeof(alias)`. Rather than preserving a falsely complete negative,
`DataFlow.vla_complete` and the Python `vla_complete` field are false for this
case. Summary completeness incorporates the flag, so `reaches()` returns
`Unknown`, not `No`.

An identifier in an array extent that cannot be resolved as a lexical value
also fails this signal. It may be a macro, enum constant, field, type name or
callee; the analysis cannot certify its value effects from the opaque token
run. Numeric fixed bounds remain complete.

The 25-file in-repository C fixture population retained byte-identical
definitions, uses, edges and defect results after removing only the new
`vla_complete` field:

`b0419de043999dc06ee7fb56999cd1b03889d961f25741a89e0dfb42bea65e86`

## Performance evidence

`tools/bench_analysis.py --shape vla` now retains 32, 128 and 512 direct VLA
declarations. At 512 declarations, the QA 63 wheel and current release
extension measured 1.648 ms and 2.209 ms respectively for `data_flow`.
The additional 0.561 ms produces 512 previously missing uses and reaching
edges; it is a measured semantic cost rather than unexplained overhead. These
are local medians on a shared machine, not cross-machine guarantees.

## Validation and refreshed artifacts

- The focused Rust data-flow suite passed 70 tests.
- The Rust workspace passed 597 core tests, one README integration test,
  three binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,049 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, custom
  distribution checks, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 598,962 | `ef7b05f6c075e462c8b0d5d9b40bdf1c69fba6aae19a32b8aad81681eddf59e1` |
| `target/qa64-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,247,294 | `a082be6cec19766c54700593325cb271f136a657c3e64751980e763a905e51e4` |
| `target/qa64-a/cindergraph-0.1.0.tar.gz` | 651,919 | `39b45aa158d6fc24703854929b005a9bd4bb78870bd679b4273f325032ab219e` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The unpacked crate passed its 597 library tests, README test
and two doctests, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
