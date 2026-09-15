# QA iteration 73: pointer reassignment coverage and span-index scaling

Iteration 72 covered repeated writes through one pointer to one object. This
iteration widens the semantic population to two objects and two pointers,
including pointer copies and reassignments, then removes a repeated source-span
scan exposed by a new pointer-copy benchmark.

The new seeded Python oracle simulates the exact target of each pointer. For
each generated program it checks that the concrete final definition of the
returned object is among the reaching definitions, matching definitions by
kind and byte span rather than relying on analysis-table order. It covers 250
committed seeds; a separate exploratory population of 2,000 programs also
passed. The generated programs mix direct writes, indirect writes, pointer
copies and pointer reassignments across locals `x`, `y`, `p` and `q`.

Profiling the resulting pointer-copy chains found that local points-to recovery
rescanned all definitions and uses for every initializer expression. The
implementation now sorts definition and use indexes by source span, selects
only candidates whose start lies inside the expression with `partition_point`,
and uses a set for exact address-taken-span membership. This preserves the
existing conservative may-alias result while reducing unrelated span work.

## Performance and semantic equivalence

The benchmark comparison used the exact QA72 release wheel in an isolated
environment and the current release editable extension, both under CPython
3.14.3. Seven invocations of `tools/bench_analysis.py` measured a 512-link
pointer-copy chain:

| Operation | QA72 median | QA73 median | Change |
| --- | ---: | ---: | ---: |
| `data_flow` | 2.767899 ms | 2.654912 ms | 4.1% faster |
| `call_summaries` | 2.157951 ms | 2.007949 ms | 7.0% faster |

An initial comparison accidentally paired a debug editable extension with a
release build. Those figures were discarded and are not evidence for this
change. A supplementary single local series at 4,096 pointer copies measured
102.746064 ms before and 93.681963 ms after for `data_flow` (8.8% faster), but
the repeated 512-link measurements above are the primary result.

To check that the optimization did not change public results, the exact QA72
wheel and current release extension produced byte-identical serialized output
for 196 decompiler fixtures and six generated pointer-copy chains. Both output
streams were 4,113,149 bytes with SHA-256
`af6adfcb4f21bfcfdda2e24e17db06fa4737f2081227443b74adbc70df29307f`.

## Validation and refreshed artifacts

- Focused Rust data-flow tests passed 76 cases. Focused Python pointer and
  seeded-provenance tests passed 905 cases.
- The Rust workspace and exact packaged crate each passed 603 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- The full Python/design population passed 3,537 tests, skipped ten and
  deselected three optional Joern cases.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint and
  `git diff --check` passed.
- Cargo 1.88 publish dry-run, deterministic double normalized wheel/sdist
  builds, crate notices, the custom distribution checker, Twine,
  check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed its smoke test on CPython 3.12.13,
  3.13.12 and 3.14.3. The exact sdist rebuilt and passed the same smoke test on
  CPython 3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 602,752 | `bc2946556578ffc29fed07980661066ad30312a145916a3af9fd1e506daee915` |
| `target/qa73-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,218,731 | `4b63eb0f2664f5cbde3f327dada3ed9318ef90f6269db63477668c1acd054aff` |
| `target/qa73-a/cindergraph-0.1.0.tar.gz` | 656,847 | `7a7f132d4b3bd646164e8a5745eb5377480dc870b2cb74ade8d81230220e3a2d` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
