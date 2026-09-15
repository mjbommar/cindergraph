# QA iteration 81: result-only pointer arithmetic and indexed solving

Iteration 80 made pointer-cast provenance result-sensitive, but pointer
arithmetic still used a token-wide test. Arithmetic in a conditional condition
or a discarded comma operand consequently tainted an otherwise known pointer:
both `(x + 1) ? &a : &b` and `(x + 1, &a)` were reported incomplete even
though their possible result values are local addresses.

The memory pass now indexes expression regions whose values do not contribute
to their enclosing result: conditional conditions and every comma operand but
the last. Pointer-arithmetic tokens in those regions no longer taint the
destination. The same regions are excluded from address, direct-copy,
dereference-load and unresolved-source constraints, so discarded pointer
expressions cannot add spurious targets or unknown alternatives. Arithmetic in
a conditional result arm or final comma operand remains incomplete.

An initial generated oracle incorrectly expected `(q++, q)` to stay complete;
the implementation correctly rejected it because the discarded increment
mutates the pointer subsequently used as the result. After separating
independent-address mutation cases from nonmutating discarded arithmetic, two
2,000-program populations passed: every non-result arithmetic case remained
complete, and every result-producing pointer-arithmetic case was incomplete.

The new durable `pointer-results` benchmark then exposed a broader pre-existing
solver bottleneck. Reaching-edge construction scanned every definition up to
three times per use even though the solver had already grouped definition
indices by binding. Those queries now visit only the selected binding's
siblings. Dead-store discovery likewise builds one used-definition bitmap
instead of rescanning every edge for every definition.

On the 512-initializer `pointer-results` workload, the exact QA80 wheel versus
the final release editable extension measured 153.791 versus 95.269 ms for
`data_flow`, and 75.955 versus 20.312 ms for `call_summaries`. At 1,024, the
same measurements were 1,746.811 versus 374.786 ms and 1,452.342 versus 84.059
ms. The remaining growth reflects the dense reaching-definition lattice and
is not claimed linear. On the established 512-link pointer-copy workload,
seven same-interpreter invocations moved from 2.650944 to 2.437067 ms for
`data_flow` and from 2.004891 to 1.767001 ms for `call_summaries`.

As an independent semantic check on the solver rewrite, 2,000 generated scalar
programs produced byte-for-byte identical serialized data-flow and call-summary
results under the exact QA80 wheel and the final editable extension.

## Validation and refreshed artifacts

- The full Python/design population passed 3,652 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 610 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint 1.7.12,
  RustSec, Cargo Deny and `git diff --check` passed. Ruff requested one
  mechanical smoke-test line wrap after the semantic suites passed; all
  affected formatting and static gates were rerun successfully.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 605,857 | `b5ef8483bf27acb759e6540a733cf7f70988dfc2e51df26c960fab6b2ac488c0` |
| `target/qa81-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,229,417 | `f19b41919139968553e60f63a8f236f7c80b79cb93f81da3f240982e3f7b76f7` |
| `target/qa81-a/cindergraph-0.1.0.tar.gz` | 659,993 | `b6f39a9632ad2513f42ceb2b4670a66a4866fdd3affab532c49dbb40ea4f057b` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
