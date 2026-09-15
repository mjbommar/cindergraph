# QA iteration 63: lexical typedef identity inside functions

QA 62 documented block-scope typedefs as unresolved. Direct probing showed two
concrete consequences:

- `typedef unsigned long word_t;` appeared in `bindings` as though `word_t`
  named an object;
- `sizeof(word_t)` could then be attached to that false binding and reported
  as an unresolved value use.

The data-flow scope stack now represents both value bindings and typedef
identities in C's ordinary-identifier namespace. A typedef declaration adds no
value binding or definition, a name used as a type in `sizeof` adds no value
use, nested scope exit removes the typedef, and a later object declaration
correctly shadows it. Preceding file-scope typedefs use the QA 62
translation-unit index rather than being copied into every function.

This is identity recovery, not typedef expansion. Reported source-level types
still retain their typedef spelling and do not claim ABI width, aggregate
layout or resolved target shape.

## Adversarial QA and performance

Four valid cases covering a scalar typedef, object shadowing, a multi-name
typedef and a function-pointer typedef passed GCC 15.2 with C11 warnings as
errors. Focused Rust and Python tests additionally cover scope exit and
file-scope `sizeof` identity.

An initial implementation eagerly copied every visible file typedef into every
function scope. The permanent QA 62 parameter-typedef benchmark caught the
result: the 512-element `data_flow` median rose from 3.48 ms to 9.53 ms. That
implementation was rejected. Indexed on-demand file lookup restored the local
median to 3.53 ms.

A new `local-typedefs` benchmark retains 32, 128 and 512 declaration cases.
At 512 declarations, QA 62 and the retained implementation measured 1.050 ms
and 1.047 ms respectively for `data_flow`; the semantic correction therefore
does not impose a material regression on that adversarial sample. Timings are
local medians on a shared machine, not cross-machine guarantees.

The 25-file in-repository C fixture population produced byte-identical
canonical data-flow output before and after the change:

`b0419de043999dc06ee7fb56999cd1b03889d961f25741a89e0dfb42bea65e86`

Those fixtures contain no affected local typedef case, so this is finite
non-regression evidence rather than evidence for the repaired behavior.

## Validation and refreshed artifacts

- The focused Rust data-flow suite passed 68 tests.
- The Rust workspace passed 595 core tests, one README integration test,
  three binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,046 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, all-feature tests, Clippy, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs and `git diff --check` passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, custom
  distribution checks, Twine, check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 598,061 | `5466032f353ea0f14da5e5abf0d86ca8cbd8710133230710a90594e370fafa00` |
| `target/qa63-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,246,096 | `d50036d2e6b411481c5c4f2e2c35169f147d72ab93db5ec134169f9b1711823b` |
| `target/qa63-a/cindergraph-0.1.0.tar.gz` | 650,707 | `1c877e181d414882a63a09f13e8074daa45ff919beecf7ef7caddedd27c04905` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The unpacked crate passed its 595 library tests, README test
and two doctests, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
