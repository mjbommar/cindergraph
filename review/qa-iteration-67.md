# QA iteration 67: unevaluated operators inside array bounds

Ad hoc semantic probing found a false dependency in valid C:

```c
int f(int n) { int values[sizeof(n)]; return sizeof(values); }
```

The opaque array-suffix recovery added in QA 64 treated every identifier as an
evaluated bound use. It therefore read `n` and produced a false
parameter-to-return summary flow, even though ordinary `sizeof` operands are
unevaluated. The same problem affected `_Alignof` and parameter array bounds.

Array-bound recovery now classifies parenthesized `sizeof` and `_Alignof`
regions at the token level. It retains C's important exception:
`sizeof(int[n])` evaluates the VLA type bound and still reads `n`. A visible
typedef and leading type qualifiers are handled, nested `sizeof` operands are
visited, and unparenthesized `sizeof name` is covered without guessing the
extent of a more complex unary expression. Punctuation and identifiers are
trimmed before classification so whitespace cannot change semantics.

Regression coverage includes expression operands, VLA type operands, typedef
and qualified types, mixed `sizeof(n) + n` expressions, unparenthesized
`sizeof n + n`, and `_Alignof(int[n])`. A compiled GCC 15.2 runtime oracle
observed zero side effects for `_Alignof(int[++g])` and one for
`sizeof(int[++g])`, matching the implemented distinction.

## Performance evidence

A release-wheel comparison of the existing 512-parameter VLA workload measured
1.423 ms for QA 66 and 1.452 ms for the current `data_flow` path. The roughly
2% difference is small enough to be noise on the shared machine, but is
reported rather than claimed as a speedup. `call_summaries` measured 0.838 ms
and 0.839 ms respectively. These are local medians, not cross-machine
guarantees.

## Validation and refreshed artifacts

- Focused Rust and Python semantic regressions passed, including whitespace
  variants and evaluated VLA-type exceptions.
- The Rust workspace passed 599 core tests, one README integration test, three
  binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,067 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs, Actionlint and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, the custom
  distribution checker, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 600,486 | `03c2788b08fb47e13b6d97525824dff34d66a9a24a66c3ec571cf1b07b094e05` |
| `target/qa67-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,248,891 | `bf658f51cc2307388f549e974278d558c0a4a17cf4a24b99204beb082ac2e3d5` |
| `target/qa67-a/cindergraph-0.1.0.tar.gz` | 653,750 | `bb3c207a561a5ca879116df47dd7e6fb72ca960411dcfcddbd82218dc8068b64` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The packaged crate passed its 599 library tests, README test,
two doctests and strict rustdoc, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
