# QA iteration 83: address-taking is not value flow

The pointer-result workload retained one semantic and performance defect after
the indexed and word-parallel solver work. An `AddressTaken` event was treated
as both a read and a weak reaching definition of the addressed object. In C,
`&x` exposes the location of `x`; it neither reads nor writes the stored value.
The old model consequently attached every address-taking event to later reads
of the same binding and generated a quadratic number of false dependence
edges.

Address-taking remains an explicit public event so the points-to analysis and
API consumers can observe local-storage escape. It is now excluded from value
uses, the reaching-definition lattice and dead-store candidates. Because the
escaped storage may be observed through an external reader, its prior stores
are conservatively excluded from dead-store diagnostics and its binding is not
reported unused. `MemoryWrite` remains the only weak value definition. The
unused-binding query now marks used, parameter and escaped bindings once in a
dense bitmap instead of scanning all uses and definitions for every binding.

Rust, Python and installed-wheel regressions pin the distinction: `&x` remains
in `definitions`, contributes no `uses` or data-flow edges, and does not make
the value it addresses dead or unused. An independent generated population of
2,000 address-taking programs confirmed those invariants. Separately, 4,000
generated programs without address-taking produced the same SHA-256 digest,
`ab126f38e6ec82c0c7e325e781ec2b05c6f67ac79f941b3d1fae1589021c185b`,
for serialized data-flow and call-summary output under both the exact QA82
wheel and this iteration's editable extension.

## Performance

Seven groups of five same-interpreter invocations were measured for each
operation; the table reports medians. The baseline is the exact QA82 CPython
3.14 wheel and the candidate is the release-mode editable extension.

| Initializers | Operation | QA82 ms | QA83 ms | Speed-up |
| ---: | --- | ---: | ---: | ---: |
| 128 | `data_flow` | 4.658 | 1.147 | 4.06x |
| 128 | `call_summaries` | 1.551 | 0.978 | 1.59x |
| 512 | `data_flow` | 85.214 | 7.942 | 10.73x |
| 512 | `call_summaries` | 14.653 | 7.040 | 2.08x |
| 1,024 | `data_flow` | 355.190 | 24.840 | 14.30x |
| 1,024 | `call_summaries` | 62.987 | 23.164 | 2.72x |

At 1,024 initializers, definitions remain 3,077 because public escape events
are preserved. Uses fell from 3,077 to 1,029 and dependence edges from
1,053,702 to 1,030. This is removal of false output, not suppression of true
value flow. On the in-repository corpus, edges fell to 22,804; dead stores
remained 13 and unused bindings remained one. Unresolved value uses increased
from 964 to 968 because four address operands are no longer counted as reads
that happen to reach a definition.

## Validation and refreshed artifacts

- The full Python/design population passed 3,653 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 612 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint 1.7.12,
  RustSec, Cargo Deny and `git diff --check` passed.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 606,567 | `bf80a6e65a4e64ee7cc1a1d1a40ce71ff9038cf18625095255d48eb58621a4f2` |
| `target/qa83-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,234,305 | `088cb2ce420b86aadd6a8ff824c82b8f523db9a539f14642cd59cb1d1f358dc2` |
| `target/qa83-a/cindergraph-0.1.0.tar.gz` | 660,694 | `ee8a35afbd788b13de6c907f6b0eb0910628f8056f6ac5c53e2237f67486325d` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
