# QA iteration 88: indexed event collection

QA87's new repeated-discarded-assignment benchmark remained superlinear. Two
independent repeated scans occurred before the reaching-definition fixed point:

1. write promotion searched `uses` linearly, then removed one element and
   shifted the remaining tail, for every assignment/address/increment event;
2. joining definitions and uses to CFG nodes scanned every node for every
   event.

Write promotion now indexes original use spans, preserves promoted-definition
order, marks plain assignments and address operands for removal, and compacts
the use vector once. Event joining now builds one source-interval index. A
prefix-maximum end table lets ordinary disjoint statement lookups stop after
one candidate, while overlapping recovered spans still select the lowest CFG
node ID exactly as the previous linear implementation did. Entry remains the
fallback for parameters and uncovered events.

A durable exhaustive unit test compares indexed and linear node selection for
every possible span in a branch/loop function. An exact differential generated
4,000 functions mixing direct assignments, pointer copies, address writes,
increments, conditionals and discarded pointer assignments. QA87 and QA88
produced byte-identical serialized data-flow and call summaries with digest
`d0a073ff25d576fc1340b938765d2b763dade9559abda65f060660435edfac60`.
The targeted workload is now available as
`tools/bench_analysis.py --shape discarded-pointer-assignments`.

## Performance

Seven single-call CPython 3.14 samples compared the exact QA87 and QA88 wheels:

| Assignments | QA87 ms | QA88 ms | Speed-up |
| ---: | ---: | ---: | ---: |
| 128 | 0.477 | 0.474 | 1.01x |
| 512 | 2.773 | 2.343 | 1.18x |
| 2,048 | 25.066 | 19.029 | 1.32x |
| 4,096 | 91.226 | 65.380 | 1.40x |
| 8,192 | 343.003 | 239.832 | 1.43x |

The optimization removes about 30% at the largest point but does not make the
entire operation linear. Separate `analyze`, `data_flow` and `call_summaries`
measurements show syntax analysis scaling close to source size while the
definition-width bitset lattice remains the next dominant target. A specialized
single-chain solver is plausible, but it must prove exact edge ordering,
same-node weak/strong write behavior, unresolved-use reporting and dead-store
identity before replacing the general fixed point.

## Validation and refreshed artifacts

- The full Python/design population passed 3,663 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 615 core tests, one
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
| `target/package/cindergraph-0.1.0.crate` | 610,199 | `255609b974db956de79b0acb8830e9936b4192a9de25b033a6433afe0f523d26` |
| `target/qa88-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,254,580 | `13ee248c402d2b4663074e1131db4b3a606d75280bc08c4b23ad89e0d2425a4c` |
| `target/qa88-a/cindergraph-0.1.0.tar.gz` | 664,574 | `25c469b123d93608c06ed6a1718302c59d2f6d909e94cef57fcf9439f79dba23` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
