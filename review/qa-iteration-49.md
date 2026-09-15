# QA iteration 49: type-recovery finding and documentation handoff

The user redirected work to wrap-up and documentation before an engine repair
began. No Rust implementation changed in this iteration.

## Confirmed finding

On `edf2777` plus the pending QA changes, using the existing release extension:

| Body within `int f(int x,int *p){...}` | Actual recovered type | Expected declaration |
| --- | --- | --- |
| `int a=x*2,b=x;return a+b;` | `b`: pointer depth 1 | scalar `int` |
| `int a=p[0],b=x;return a+b;` | `a`: array rank 1 | scalar `int` |
| `int a=*p,*b=p;return a+*b;` | `b`: pointer depth 2 | `int *` |

Read-only reproduction from the repository:

```bash
export TMPDIR=/home/mjbommar/.cache/cindergraph
uv run --no-sync python -c 'import cindergraph as c; print(c.data_flow("int f(int x){int a=x*2,b=x;return b;}")[0]["bindings"])'
```

`crates/cindergraph/src/csource/dataflow/types.rs::declared_types` counts
stars between consecutive declared names and brackets after a name up to the
next one. Those token ranges include initializer expressions. A future repair
should use individual declarator syntax boundaries, excluding initializers,
while retaining actual pointer/array declarators and nested declaration scopes.

Four temporary Python cases were run: three reproduced the defect, and the
array-declaration control passed. The temporary cases were removed when the
user requested documentation focus; restore them before implementing the fix.
No compiler oracle or benchmark was run for this finding, and no semantic
repair is claimed. The public Python reference now discloses the limitation.

## Wrap-up and next documentation work

The previous broad validation is recorded in [iteration 48](qa-iteration-48.md):
2,925 Python/review tests passed, ten skipped, three optional Joern cases
deselected. That is historical evidence, not a fresh full-suite result here.
Earlier Rust/package evidence has its own source snapshots; do not present it
as validation of every pending change or as cross-platform release approval.

Prioritise documentation next:

1. Add a concise documentation index separating maintained API guidance from
   chronological QA evidence. The iteration-1 progress file is not current status.
2. Expand Rust usage documentation to cover parsing diagnostics, graph export,
   dataflow identities and uncertainty; compile every Rust example.
3. Document installation from a local wheel or source checkout with verified
   commands. Do not advertise a PyPI release before publication is verified.
4. Consolidate supported analysis fragments and known limitations, especially
   VLA size dependencies, declaration types, aliasing and completeness flags.
5. Publish a support/evidence matrix distinguishing local Linux artifact checks
   from configured-but-unexecuted platform CI and Glaurung migration work.

All pending work remains local and uncommitted. No publication, push, external
evaluator or upstream interaction occurred. The broader QA objective is not
complete; the next requested focus is documentation.
