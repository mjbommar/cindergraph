# QA iteration 59: shared parameter-provenance indexes

Iteration 9 removed one quadratic parameter lookup but recorded another open
frontier: each parameter provenance trace rebuilt the same definition-to-use,
CFG-control and node-to-write indexes. A 512-parameter function therefore
repeated three function-wide scans 512 times, and fixed-point reevaluation
could repeat all of that work again.

`TraceIndex` now owns those immutable function-local indexes. `summarize()`
constructs one per analyzed function and shares it across:

- every parameter-to-return trace;
- every fixed-point reevaluation of that function;
- the final parameter-to-call-argument transfer pass.

Selected-binding reachability, summary knowledge and trace queues remain fresh
per parameter, so this changes setup cost rather than the provenance semantics.
The existing wide-signature benchmark remains available as `parameters`, and
`parameters-call` now durably exercises the transfer path as well.

## Before/after measurements

Both durable `parameters` runs used the same CPython 3.14.3 process model,
release extension, source generator, and seven batches of five calls after
warm-up. Only `call_summaries` should benefit directly:

| Parameters | Before | After | Change |
| ---: | ---: | ---: | ---: |
| 32 | 0.02655 ms | 0.02490 ms | -6.2% |
| 128 | 0.10097 ms | 0.08971 ms | -11.2% |
| 512 | 0.57728 ms | 0.41453 ms | -28.2% |

A separate 101-sample direct-call workload compared two independently loaded
binaries: the pre-change QA 57 wheel and the post-change editable release
extension. It summarized `return sink(p0)` across a wide signature:

| Parameters | QA 57 wheel | Shared index | Change |
| ---: | ---: | ---: | ---: |
| 32 | 0.03674 ms | 0.03258 ms | -11.3% |
| 128 | 0.14002 ms | 0.10645 ms | -24.0% |
| 512 | 0.84124 ms | 0.52605 ms | -37.5% |

These are local medians on a shared machine, not cross-machine guarantees.
The independently loaded binaries strengthen attribution, but neither sample
is a whole-corpus throughput claim.

## Gates and refreshed artifacts

- All 56 focused dataflow/interprocedural Rust tests passed.
- Rust workspace: 589 passed and one ignored (583 core, one README
  integration, three binding tests and two doctests).
- Python/design: 3,030 passed, ten skipped and three optional Joern cases
  deselected.
- Rust formatting, Clippy, no-default-features, strict rustdoc, RustSec, Cargo
  Deny, Ruff, ty, generated stubs and `git diff --check`: passed.
- Cargo publish dry-run, deterministic double wheel/sdist builds, custom
  distribution checks, Twine, check-wheel-contents and auditwheel: passed.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 594,929 | `f1ea749acbcbb36dc8e317d5d8b0f7daee36cef061e221052e822a4d3a196b40` |
| `target/qa59-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,226,433 | `712c7f8575947ee8455479665d365769e9375a7210659eec45a3b666c3e2fb75` |
| `target/qa59-a/cindergraph-0.1.0.tar.gz` | 647,391 | `0565bdd652b2578d8e347088c9e1156a8b0048e43feb22099221e57f129b056c` |

The second wheel and sdist copies were byte-identical. The exact wheel passed
isolated CPython 3.12.13, 3.13.12 and 3.14.3 smoke tests; the exact sdist rebuilt
and passed on 3.12.13. The unpacked crate passed 583 library tests, one README
integration test and two doctests, with one external-corpus test ignored.

The wheel is a local manylinux 2.34 candidate, not the release workflow's
manylinux 2.17 artifact. Baseline remains commit
`edf2777abc99e1c3113b4f1d51fe021f31432538` plus pending QA changes. No commit,
push, workflow dispatch, tag or registry publication was performed.
