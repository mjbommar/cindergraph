# QA iteration 90: local event indexes in the linear solver

QA89 made wide pointer-assignment chains nearly linear, but a second workload
showed that read/modify/write chains still grew superlinearly. For 8,192
statements of `x = x + 1`, every use searched the complete global list of
definitions for `x` to find writes on its own CFG node.

The linear-CFG solver now builds a definition index keyed by CFG node and
binding. Same-node strong and weak write selection examines only that local
event list. Live alternatives remain sorted by public definition index, and
chain detection consumes at most two successors directly instead of allocating
a temporary vector for every node. Branched, cyclic and disconnected graphs
continue to use the unchanged general solver.

An exact differential generated 20,000 straight-line functions containing
direct, compound and multiple same-node assignments, increments, unknown
bindings, pointer copies, indirect loads/stores and discarded expressions.
QA89 and QA90 produced byte-identical serialized data-flow and call summaries
with digest
`378cbdc9146ed518fdc1a1d3b5895fb0813f42213a1a4b7f69a19615de41c578`.

## Performance

Seven single-call CPython 3.14 samples compared the exact QA89 and QA90 wheels
on repeated `x = x + 1` statements:

| Statements | QA89 ms | QA90 ms | Speed-up |
| ---: | ---: | ---: | ---: |
| 128 | 0.472 | 0.296 | 1.59x |
| 512 | 1.308 | 1.097 | 1.19x |
| 2,048 | 9.514 | 4.761 | 2.00x |
| 4,096 | 24.194 | 9.872 | 2.45x |
| 8,192 | 80.775 | 20.363 | 3.97x |

The largest point is about four times faster and the new curve is close to
linear. A chain with weak updates that all remain live can still produce a
quadratic number of output edges; that is output-sensitive behavior rather
than this eliminated sibling-search overhead.

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
| `target/package/cindergraph-0.1.0.crate` | 611,156 | `5d5ff527639924f45a799b3ca473fe6c5bd93b8a56b91fc03e89166183ec7ee6` |
| `target/qa90-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,263,890 | `1305a5d13b9f4ea30e44448b1622d96a2f1ab6470901a706121fd39d41babf99` |
| `target/qa90-a/cindergraph-0.1.0.tar.gz` | 665,522 | `50710e0ae65597f7d3b8c6abcec08323d33d812027104f2a8c80f5cc495b5d15` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
