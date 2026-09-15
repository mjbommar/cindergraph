# QA iteration 76: second-order local pointer mutation

The pointer completeness audit found a false negative after a pointer was
replaced through another local pointer:

```c
int f(int x) {
    int a = 0, b = 0;
    int *p = &a;
    int **q = &p;
    *q = &b;
    *p = x;
    return b;
}
```

Before the repair, Cindergraph reported complete memory and summary results but
omitted the concrete `x`-to-return flow. The first indirect write was projected
onto `p` as a value definition, but did not update `p`'s local target set; the
second write was consequently projected only onto stale target `a`.

A red Rust and Python regression captured both the incorrect completeness and
missing flow. When an indirect store may target a pointer binding, memory
projection now gathers locally visible address-taken and pointer-copy operands
from the right-hand side and unions their known targets into the mutated
pointer. Subsequent stores therefore retain the newly visible concrete target.
The analysis remains explicitly incomplete because this bounded local model
does not prove that it represents every second-order memory effect. Existing
targets are retained as may-alias alternatives rather than unsafely killed.

The same source-span indexes introduced in iteration 73 restrict RHS discovery
to the indirect assignment expression. An exploratory population of 2,000
programs varied initial and replacement objects, two to seven locals, and
direct-address versus pointer-copy replacement. Every program preserved the
concrete replacement-target flow and remained fail-closed at both memory and
summary layers.

Seven same-interpreter benchmark invocations compared the exact QA75 release
wheel with the current release editable extension on a 512-link pointer-copy
chain. `data_flow` medians were 2.669469 and 2.621671 ms; `call_summaries`
medians were 2.017691 and 1.980761 ms. These small favourable movements are
treated as noise, not a performance claim; importantly, the indexed RHS work
does not show a scaling regression in the established copy workload.

## Validation and refreshed artifacts

- The full Python/design population passed 3,496 tests, skipped ten and
  deselected three optional Joern cases.
- The Rust workspace and exact packaged crate each passed 606 core tests, one
  README integration test and two doctests; one external-corpus test was
  ignored. The workspace's three binding tests also passed.
- Rust formatting, no-default-feature checking, Clippy, strict workspace and
  packaged-crate rustdoc, Ruff, ty, generated stubs, Actionlint, RustSec, Cargo
  Deny and `git diff --check` passed. Ruff initially identified two mechanical
  formatting changes after the semantic suites passed; they were applied and
  the remaining gate was rerun successfully before packaging.
- Cargo 1.88 publish dry-run with explicit dirty-tree packaging, deterministic
  double normalized wheel/sdist builds, crate notices, the custom distribution
  checker, Twine, check-wheel-contents and Auditwheel passed.
- The exact wheel installed and passed the strengthened smoke test on CPython
  3.12.13, 3.13.12 and 3.14.3. The exact sdist rebuilt and passed it on CPython
  3.12.13.

| Artifact | Bytes | SHA-256 |
| --- | ---: | --- |
| `target/package/cindergraph-0.1.0.crate` | 603,912 | `710a4e85c8600875ccaec0d882d7f5ecce2c2c304d28c9e404b0c898b9950ba1` |
| `target/qa76-a/cindergraph-0.1.0-cp312-abi3-manylinux_2_34_x86_64.whl` | 1,221,002 | `19b8625c6aa9741906c31eb01380f00d9cbf6de63c33f08e5b49422a478531d2` |
| `target/qa76-a/cindergraph-0.1.0.tar.gz` | 658,062 | `f412c50d7e5f425f234d83971759dc83c37056ef2064a05e1b5cc93da149d856` |

The second normalized wheel and sdist were byte-identical. The Linux wheel is
still a local manylinux 2.34 candidate, not the release workflow's manylinux
2.17 artifact. Baseline remains commit `edf2777` plus pending QA changes. No
commit, push, workflow dispatch, tag or registry publication was performed.
