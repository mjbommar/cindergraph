# QA iteration 91: indexed projected-memory placement

Scale testing after QA90 separated unavoidable result size from another stale
quadratic lookup. Repeating `*p = x; y += *p;` produces eight dependence edges
per pair, so its output is linear. Runtime nevertheless curved sharply because
every projected indirect write and read still called the old linear
event-to-CFG-node scan. QA88 had indexed initial lexical events but did not
reuse that index for events synthesized by the local points-to pass.

`NodeSpanIndex` is now shared within the data-flow module. The memory projection
pass constructs it once per function and uses it for every synthesized write
and load. The old linear lookup remains test-only as the exhaustive reference
oracle. Its containment, lowest-node-ID tie break and entry fallback semantics
are unchanged.

A 10,000-program exact differential mixed plain and parenthesized indirect
stores, indirect loads, compound direct writes and different source orders.
QA90 and QA91 produced byte-identical serialized data-flow and call summaries
with digest
`c377dfd78b3ad7e612eb13f28f7b436800b96dfd99e9595f68e54b27a034b2a9`.

## Performance

Seven single-call CPython 3.14 samples compared the exact QA90 and QA91 wheels.
Every row produced exactly `8 * pairs + 1` edges:

| Access pairs | QA90 ms | QA91 ms | Speed-up | Edges |
| ---: | ---: | ---: | ---: | ---: |
| 1,024 | 11.582 | 7.239 | 1.60x | 8,193 |
| 2,048 | 24.804 | 15.318 | 1.62x | 16,385 |
| 4,096 | 71.042 | 33.738 | 2.11x | 32,769 |
| 8,192 | 230.506 | 64.395 | 3.58x | 65,537 |
| 16,384 | 856.270 | 141.715 | 6.04x | 131,073 |

The new curve tracks source and output size far more closely. These are local
host measurements; they do not claim platform-independent latency.

## Validation and refreshed artifacts

- The full Python/design population passed 3,663 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 616 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict rustdoc, Ruff,
  ty, generated stubs, Actionlint 1.7.12, RustSec, Cargo Deny and
  `git diff --check` passed.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 611,165 | `f6eae9070b63c6b02e173e50b9e1a4cd7ad37d8b82a5a82dd9823dcf11b48cea` |
| `target/qa91-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,264,848 | `edac08cdc7dadf27a21ab7e4eb95fa63bd8eb236d0233261da0e2eb5e6e11879` |
| `target/qa91-a/cindergraph-0.1.0.tar.gz` | 665,528 | `dab8c5d037d0a0e5820d660736f2479d9d038a178b340f8c5b700eac3d9f0359` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
