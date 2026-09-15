# QA iteration 79: fail-closed integer-to-pointer casts

Iteration 78 made fully local pointer loads complete, then a neighboring probe
found that this completeness could be claimed too broadly. In
`int *p = x ? &a : (int *)1`, the solver retained the known target `a` but had
no representation for the integer-derived alternative. A later `*p = x` was
therefore reported as complete even though `p` may designate memory outside
the local points-to model.

Pointer-producing casts are now classified before points-to propagation. A
cast is unsupported when its operand has a literal source, or when it has no
known address or pointer source. Any pointer definition containing such a cast
is marked unknown. Casts of known local addresses and known pointers continue
to preserve their targets and completeness; integer-derived and mixed
address/integer alternatives fail closed. This is deliberately conservative,
including for a cast of the integer literal zero: the local model does not
attempt to prove which conditional paths can reach a later dereference.

The first implementation was semantically correct but scanned nested cast
ranges repeatedly. A 2,048-cast stress case increased from approximately 1.19
ms on the QA78 wheel to 4.01 ms. The accepted implementation collects cast
nodes during the existing dereference walk, indexes literal starts once, and
lets the innermost pointer cast classify a pointer-cast chain. The same stress
case then measured approximately 1.34 ms. A permanent Rust regression includes
a 2,048-deep known-pointer cast chain and a 128-deep integer-derived chain to
protect both termination and classification.

The Python regressions cover literal, null and scalar integer-derived pointer
casts. Two exploratory 2,000-program populations varied local counts, targets,
integer sources and cast-chain lengths. Every integer-derived case remained
incomplete, while every known-pointer cast chain remained complete and
preserved parameter-to-return flow.

Seven same-interpreter benchmark invocations compared the exact QA78 release
wheel with the final release editable extension on the 512-link pointer-copy
workload. `data_flow` medians were 2.669366 and 2.638904 ms;
`call_summaries` medians were 2.017578 and 1.978856 ms. The small favorable
movements are treated as noise rather than a performance claim.

## Validation and refreshed artifacts

- The full Python/design population passed 3,648 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 609 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint 1.7.12,
  RustSec, Cargo Deny and `git diff --check` passed. The installed Actionlint
  1.7.4 was also tried, but its obsolete runner-label table rejected two valid
  current GitHub-hosted labels; that tool result was not counted as a gate.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 605,370 | `3e40c8ced99a653fdb927575ea4265413882cf93d14ebca324c53b5dee337bb2` |
| `target/qa79-c/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,226,161 | `cedcd8401a5b8805c6a0f69d47713c54d46084f321ba4acadbe45285e149c823` |
| `target/qa79-c/cindergraph-0.1.0.tar.gz` | 659,537 | `2a16cbd5ef51a003824ed6a4b82fcb49f264bb28c85c60732e993e662ccc6288` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
