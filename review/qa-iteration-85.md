# QA iteration 85: indexed indirect-access lookup

An adversarial sequence of repeated `*p = x; y += *p;` operations exposed two
remaining whole-use scans in the local-memory pass. Each indirect store and
load searched the complete use table to recover its direct pointer base. The
table was already indexed by source start for initializer processing, so wide
functions paid quadratic lookup work unnecessarily.

Indirect stores and loads now use a shared binary range query over that index.
The query filters for containment and chooses the lowest original use index,
preserving the former `uses.iter().find(...)` behavior even for overlapping or
recovered spans. A new durable `pointer-accesses` benchmark shape exercises
this exact path.

As an independent semantic check, 4,000 generated programs mixed direct and
parenthesized indirect stores, indirect loads, conditional stores and several
local pointer targets. The exact QA84 wheel and final editable extension
produced the same serialized data-flow and call-summary digest:
`5a603083a18df11536edffc332a09b035a54fc151ebd68fc8d2cabc96af254f5`.

## Performance

Seven groups of same-interpreter calls compared the exact QA84 CPython 3.14
wheel with the release-mode editable extension. At 2,048 repeated access pairs,
median `data_flow` time moved from 119.104 to 109.874 ms and
`call_summaries` from 110.579 to 100.658 ms, improvements of 7.7% and 9.0%.
At 1,024, the respective medians moved from 32.522 to 30.458 ms and from
29.217 to 27.376 ms. Output size and semantics were unchanged; the remaining
superlinear work is elsewhere in pointer propagation, provenance and the
reaching solver.

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
| `target/package/cindergraph-0.1.0.crate` | 606,976 | `11572910a34737594247f278dab62e1390e779b327e1d62c72b8d539d735b12f` |
| `target/qa85-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,235,027 | `a6297d2cdc1afd69a2341479aad220f040d38fbc250d994ca96de25acf931c48` |
| `target/qa85-a/cindergraph-0.1.0.tar.gz` | 661,113 | `3d719c2d5d0d77283693f89aaf54db3cf91ac55a708585fdb07905c7ec37d2e2` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
