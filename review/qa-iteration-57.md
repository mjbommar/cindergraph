# QA iteration 57: balanced parameter declarators

The parameter reader previously searched one opaque parameter-list token run
for identifiers followed by a comma, close parenthesis or array suffix. That
worked for ordinary parameters but did not respect commas or names inside a
nested function signature. For example, the outer function in
`int f(int (*cb)(const char *, size_t), int x)` acquired a false `size_t`
binding, and a nested `values[4]` could be mistaken for an outer array
parameter.

Parameter recovery now splits only at balanced, top-level commas and selects
one declarator name per outer group. Binding recovery and type recovery consume
the same `ParameterDeclarator` result, preventing identity/type disagreement.
The scanner also:

- retains the outer name in direct function parameters such as
  `int callback(int)`;
- suppresses nested identifiers in unnamed function-pointer parameters such
  as `int (*)(size_t)`;
- counts every consecutive array suffix in `int matrix[4][8]`;
- keeps pointer stars and array suffixes scoped to the selected declarator.

This remains syntactic recovery, not typedef resolution. A group consisting of
one identifier, such as an unnamed `size_t` parameter, is inherently ambiguous
to this translation-unit-local reader and may still be interpreted as a name.
The Python reference and support matrix state that boundary.

## Regression and scaling evidence

Four Rust regressions cover a nested named function pointer, a rank-two array,
a direct function declarator and an unnamed function pointer. Six Python cases
exercise nested commas, nested array names, direct functions, unnamed function
pointers and rank-two arrays. The focused results were 40 passing Rust
dataflow tests and 205 passing Python initializer/parameter tests.

Release-extension `data_flow` timings used functions whose every parameter was
`int (*cbN)(int values[4], size_t)`, 25 samples after warm-up with GC disabled:

| Parameters | Source bytes | Median | p95 |
| ---: | ---: | ---: | ---: |
| 16 | 574 | 0.047 ms | 0.055 ms |
| 32 | 1,134 | 0.084 ms | 0.327 ms |
| 64 | 2,254 | 0.163 ms | 0.406 ms |
| 128 | 4,522 | 0.314 ms | 0.644 ms |
| 256 | 9,130 | 0.643 ms | 1.107 ms |

This is an end-to-end local parser/dataflow sample, not a standalone scanner
benchmark or cross-machine guarantee.

## Full gates and candidate artifacts

- Rust workspace: 589 passed and one ignored (583 core, one README
  integration, three binding tests and two doctests).
- Complete Python/design suite: 3,028 passed, ten skipped and three optional
  Joern cases deselected.
- Rust 1.88 formatting, Clippy, no-default-features check, strict rustdoc,
  RustSec, Cargo Deny, Ruff, ty and generated stubs: passed.
- Cargo publish dry-run with the explicitly required dirty-tree override,
  deterministic double packaging, custom distribution checks, Twine,
  check-wheel-contents and auditwheel: passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 594,625 | `d5d7da783ce8fc04bc14e481fd3d3ff2062285d827ab3ffc040f99e56fd5df72` |
| `target/qa57-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,226,203 | `ce8f022b12abb69b76ea1645e6df64acd625e904e5c7c094f9f0f56d6d657665` |
| `target/qa57-a/cindergraph-0.1.0.tar.gz` | 646,840 | `626918a28ee87cbb725845c539a9bdbd6e9073239e78e060a7e5c26f2869b3e2` |

The second wheel and sdist builds had byte-identical hashes. The exact wheel
passed isolated CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist
rebuilt and passed on CPython 3.12.13. The unpacked crate passed 583 library
tests, one README integration test and two doctests with one ignored external
corpus test. The host wheel is correctly tagged manylinux 2.34 and is not the
manylinux 2.17 release-policy artifact; the pinned container workflow remains
responsible for that wider-compatibility build.

The evidence baseline is commit `edf2777abc99e1c3113b4f1d51fe021f31432538`
plus the still-uncommitted QA series. No clean release commit, remote platform
job, tag, registry publication or credential configuration was performed.
