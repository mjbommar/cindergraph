# QA iteration 89: exact linear-CFG reaching solver

QA88 isolated the remaining superlinear cost on wide straight-line functions
to the reaching-definitions bitset lattice. The general solver stores one
definition-width bitset per CFG node. That is appropriate for joins and loops,
but a graph with exactly one acyclic path has neither: every node has one
unambiguous predecessor state.

The solver now recognizes only a traversal from entry that visits every CFG
node exactly once and has zero or one successor at each step. Branches,
conditional expressions, loops, cycles, disconnected nodes and duplicate
successor edges retain the existing general fixed point. On an accepted chain,
the fast path carries the currently reaching definition indices per binding.
It preserves the general solver's details: the latest same-node strong write,
weak writes after that strong write, unresolved uses, public definition-index
edge order and the common dead-store calculation.

A structural unit test pins acceptance of a straight chain and rejection of an
`if`, loop and conditional expression. An exact differential generated 20,000
straight-line functions with direct and compound assignments, increments,
pointer copies, indirect loads/stores, multiple same-node writes, discarded
pointer assignments and unresolved bindings. QA88 and QA89 produced
byte-identical serialized data-flow and call summaries with digest
`2fbe11159ef07fa6226649bdbe6d32a9be0ed00ef728752fa87f368d08cd0236`.

## Performance

Seven single-call CPython 3.14 samples compared the exact QA88 and QA89 wheels
on `discarded-pointer-assignments`:

| Assignments | QA88 ms | QA89 ms | Speed-up |
| ---: | ---: | ---: | ---: |
| 128 | 0.461 | 0.411 | 1.12x |
| 512 | 2.358 | 1.546 | 1.53x |
| 2,048 | 19.748 | 7.102 | 2.78x |
| 4,096 | 65.133 | 12.475 | 5.22x |
| 8,192 | 238.513 | 26.177 | 9.11x |

The largest point is about nine times faster and the new curve is close to
linear over this range. Branched and cyclic functions deliberately receive no
fast-path claim. The full Python population completed in 14.09 seconds with a
release extension, but that is not compared with QA88's 88.89-second debug-
extension run because build profiles confound the result.

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
| `target/package/cindergraph-0.1.0.crate` | 611,113 | `23dfb23da826e5b22e927d8034557b9a3d6be00fe968ba87783a43120ab16dfe` |
| `target/qa89-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,260,538 | `b2f3617bfddbe3c1f787fb08708ad1c51ef00ffc3f6b41eaf334e6e08d8b7149` |
| `target/qa89-a/cindergraph-0.1.0.tar.gz` | 665,485 | `b31c1bfcb1c8a3576baa33d3a18d88aa10337ef8f4a00053619d6421c6aa504b` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
