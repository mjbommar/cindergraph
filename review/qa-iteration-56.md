# QA iteration 56: declarator-scoped type recovery

Repaired the initializer-boundary defect first documented in iteration 49.
The old reader inferred a declarator's pointer depth by scanning from the
previous declared name and inferred array rank by scanning until the next one.
Those ranges included initializer expressions, so `*` and `[` operators could
be mistaken for declaration syntax.

The parser already represents each declarator as its own `Declarator` node and
places its initializer in a sibling node. Type recovery now consumes that
structural boundary directly:

- pointer stars are counted only before the name inside its declarator;
- array rank is counted from `ArraySuffix` descendants of that declarator;
- only direct `Declarator` children are attributed to a declaration, so a
  nested declaration in a GNU statement-expression initializer keeps its own
  identity.

This avoids text heuristics and also excludes brackets inside an opaque
function-pointer parameter list.

## Regression evidence

The original three failures now report the intended shapes:

| Declaration | Recovered result |
| --- | --- |
| `int a=x*2,b=x` | `a` and `b` are scalar `int` |
| `int a=p[0],b=x` | `a` and `b` are scalar `int` |
| `int a=*p,*b=p` | `a` is scalar `int`; `b` is `int *` |

Rust regressions additionally cover mixed scalar/pointer/array declarations,
a declaration nested inside an initializer, and array-looking syntax inside a
function-pointer parameter. A Python oracle generates 64 deterministic cases,
each with 20 independently selected scalar, pointer or array declarators and
initializers containing multiplication, dereference and subscript operators.
All 1,280 expected shapes matched.

The public README and Python reference no longer advertise the repaired defect.
They still state the real boundary: recovered types are source-level shapes,
not resolved typedefs, function signatures, ABI widths or aggregate layouts.

## Scaling sample

Release-extension `data_flow` timings used one mixed declaration per function,
25 samples after warm-up, GC disabled during measurement:

| Declarators | Source bytes | Median | p95 |
| ---: | ---: | ---: | ---: |
| 50 | 622 | 0.187 ms | 0.211 ms |
| 100 | 1,228 | 0.380 ms | 0.410 ms |
| 200 | 2,556 | 0.812 ms | 0.834 ms |
| 400 | 5,228 | 1.970 ms | 2.022 ms |
| 800 | 10,556 | 5.614 ms | 5.674 ms |

This is an end-to-end parser/dataflow sample, not an isolated type-reader
benchmark or a cross-machine performance guarantee.

## Full gates and artifacts

- Rust workspace: 585 passed and one ignored (579 core, one README integration,
  three binding tests and two doctests).
- Complete Python/design suite: 3,022 passed, ten skipped and three optional
  Joern cases deselected.
- Rust 1.88 formatting, Clippy, no-default-features check, strict rustdoc,
  RustSec, Cargo Deny, Ruff, ty and generated stubs: passed.
- Cargo publish dry-run, deterministic double packaging, custom distribution
  checker, Twine, check-wheel-contents and auditwheel: passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | — | `0d6572b4db1031d605e5a6bca5a3dde36853b68b016197eb22a981d481f87ad8` |
| `target/qa-manylinux62/cindergraph-0.1.0-cp312-abi3-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` | 1,219,078 | `e94ef73a0b0dc6b28d10ec952bd2cedacbd9c7430cc5eb3829f7613edef3e58c` |
| `target/qa-release62/cindergraph-0.1.0.tar.gz` | 645,640 | `be7d5dda47df4075e49a57780cb93dedf59dd89387fa1abea5e957916da7ea18` |

The exact wheel passed isolated CPython 3.12.13, 3.13.12 and 3.14.3 smoke
tests. The exact sdist rebuilt and passed on CPython 3.12.13. The unpacked crate
passed 579 library tests, one README integration test and two doctests with one
ignored library test, and its LICENSE/NOTICE match. Environments remain under
`/home/mjbommar/.cache/cindergraph/qa62.M10bn5`.

No remote platform job, clean commit, tag, registry publication or credential
configuration was performed.
