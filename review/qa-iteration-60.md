# QA iteration 60: indexed provenance propagation

QA 59 shared function-local adjacency across parameter traces, but two repeated
scans remained inside each trace:

1. every reached use scanned every definition to find assignment expressions
   containing that use;
2. every trace scanned every definition to rediscover the parameter definition
   the summary loop was already visiting.

The first attempted cache materialized the full definition/use Cartesian
product. It was semantically correct but deliberately rejected: it improved a
dense 256-parameter sample by only about 11% and regressed the sparse
512-parameter benchmark from roughly 0.415 ms to 0.479 ms.

The retained implementation instead builds a lazy source-order interval sweep.
Definitions enter an active set at their expression start and leave once their
effect range ends; only overlapping ranges are tested for containment. No index
is built for a function whose parameters reach no uses. The provenance API now
also accepts the exact parameter-definition index, removing the second scan.

An internal regression compares every indexed use-to-write result with the
previous linear predicate on ordinary, conditional, nested-assignment,
branching and malformed inputs. All matched.

## Independent semantic differential

The packaged QA 59 CPython 3.14 wheel and the post-change editable release
extension each summarized the same 1,000 seeded programs. Programs varied from
one to twelve parameters and zero to seventeen local assignments using copies,
addition, conditional expressions and known direct calls. Their canonical JSON
outputs were byte-identical:

`016213a0f5dba5f09afce6fefbc2cdf67803dbed2e797994c7f16a06958095e2`

This is finite differential evidence against semantic drift, not a proof for
arbitrary C.

## Performance evidence

The durable sparse-signature workload at 512 parameters improved from the QA
59 median of 0.4145 ms to 0.3164 ms, a 23.7% reduction. Relative to the start of
the two-step indexing work in QA 59, 0.5773 ms, the combined reduction is
45.2%.

An assignment-heavy workload gives every parameter its own local and returns
the sum of all locals. Thirty-one samples follow warm-up:

| Parameters | Source bytes | Before | After | Change |
| ---: | ---: | ---: | ---: | ---: |
| 16 | 359 | 0.0632 ms | 0.0628 ms | -0.6% |
| 32 | 743 | 0.1275 ms | 0.1250 ms | -2.0% |
| 64 | 1,511 | 0.2851 ms | 0.2707 ms | -5.1% |
| 128 | 3,159 | 0.6792 ms | 0.5925 ms | -12.8% |
| 256 | 6,743 | 1.8264 ms | 1.5136 ms | -17.1% |

These are local CPython 3.14.3 release-extension medians on a shared machine.
The increasing benefit is consistent with removing repeated scans but is not a
cross-machine throughput guarantee.

## Gates and refreshed artifacts

- All 57 focused dataflow/interprocedural Rust tests passed.
- Rust workspace: 590 passed and one ignored (584 core, one README
  integration, three binding tests and two doctests).
- Python/design: 3,030 passed, ten skipped and three optional Joern cases
  deselected.
- Rust formatting, Clippy, no-default-features, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs and `git diff --check`: passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, custom
  distribution checks, Twine, check-wheel-contents and auditwheel: passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 595,700 | `9902a56be44af5fdc875514abf03c86dc82e5272f057bc02250f8f3d71124791` |
| `target/qa60-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,237,450 | `6244411aac95c9c36e1b8c8c652f393acd6744c074c55f42db4001985c62af45` |
| `target/qa60-a/cindergraph-0.1.0.tar.gz` | 648,146 | `a24ada75aec0837473994cfcc55572194246b2ad6d8ee88163997d3dcf368ad5` |

The second wheel and sdist copies were byte-identical. The exact wheel passed
isolated CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt
and passed on 3.12.13. The unpacked crate passed 584 library tests, one README
integration test and two doctests, with one external-corpus test ignored.

The wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit
`edf2777abc99e1c3113b4f1d51fe021f31432538` plus pending QA changes. No commit,
push, workflow dispatch, tag or registry publication was performed.
