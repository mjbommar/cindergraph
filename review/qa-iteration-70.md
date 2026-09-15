# QA iteration 70: event-order contract and corrected pointer-write evidence

Follow-up inspection corrected the final claim in iteration 69. The analysis
was not leaving a stale weak write after a later strong assignment; the probe
had selected the last item in the `uses` table, which was an appended projected
pointer read rather than the source-last return read.

The return read is now selected by its byte span in both Rust and Python
regressions. They pin both relevant orders:

```c
*p = 1; x = 2; return x; /* only the direct assignment reaches */
x = 2; *p = 1; return x; /* both may-definitions reach */
```

This is the intended conservative local-points-to behavior. A direct strong
write kills earlier definitions; a later indirect weak write cannot kill the
direct definition because the pointee set is only a may-alias set.

The Python docstring and reference now state the observable ordering contract:
definition and use tables have stable analysis order, not source order, because
projected memory events can be appended. Consumers should use byte
`start`/`end` spans for source ordering and preserve table indices when joining
edges, unresolved uses or dead stores. The event tables were not reordered:
doing so would create needless index churn for an already deterministic API.

Clippy 1.97 also requested a guarded match arm in the pending VLA declarator
scanner. That mechanical rewrite does not change its conditions or behavior.

## Validation and refreshed artifacts

- The focused pointer-order regression passed in Rust; the Python seeded
  provenance module passed all 402 cases.
- The Rust workspace passed 602 core tests, one README integration test, three
  binding tests and two doctests; one external-corpus test was ignored.
- The full Python/design population passed 3,078 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, no-default-feature checking, all-feature Clippy, strict
  rustdoc, RustSec, Cargo Deny, Ruff, ty, generated stubs, documentation links,
  executable examples, Actionlint and `git diff --check` passed. The installed
  Actionlint 1.7.4 needed the two established hosted-runner label exceptions.
- Cargo 1.88 publish dry-run, deterministic double wheel/sdist builds, the
  crate notice checker, custom distribution checker, Twine,
  check-wheel-contents and auditwheel passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 602,093 | `7a2b1bbc65366efee0997281e8d9c01065353e2b4663b9aa832c73b6eca73bf5` |
| `target/qa70-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,252,073 | `d57c58a59c4416578cf3f83e5094dd218ed3c1574e76eb224b9bec8d32485fdb` |
| `target/qa70-a/cindergraph-0.1.0.tar.gz` | 656,040 | `a01fdd051628253511ead95eb3a61cdc0006f30927a22e9159580318b914f109` |

The second wheel and sdist were byte-identical. The exact wheel passed isolated
CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt and
passed on 3.12.13. The packaged crate passed its 602 library tests, README test,
two doctests and strict rustdoc, with one external-corpus test ignored.

The Linux wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit `edf2777` plus pending QA
changes. No commit, push, workflow dispatch, tag or registry publication was
performed.
