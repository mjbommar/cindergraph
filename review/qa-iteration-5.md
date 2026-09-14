# QA iteration 5: local pointer dependence

Baseline: `bc035f8`. The local-pointer store missing from the original review
now contributes to dataflow edges, PDG export, backward slices and return
summaries through one shared model.

The new memory pass computes flow-insensitive points-to sets from address-taking
and pointer-copy assignments. Indirect stores to known local targets create
`memory_write` definitions. They are weak updates: alternatives remain live.
Subsequent direct assignments remain strong updates and kill earlier projected
definitions. Known pointee reads are projected as uses of the target binding.
Event spans identify the pointer expression, while name/binding identify the
possible target; the Python documentation explains this distinction.

Address-taking is now a weak event as well. Merely evaluating `&y` no longer
kills the reaching definition of `y`. The solver orders same-node definitions
by their effect point and combines weak alternatives with the latest strong
definition instead of discarding them.

`DataFlow::memory_complete` and the Python field of the same name expose
unsupported memory cases. Summaries inherit this incompleteness. Unknown
pointees, mixed known/unknown alternatives, fields, arrays, complex dereferences
and tested unsupported pointer updates are not silently reported complete.
The old claim that recording pointer stores only as uses loses no real
dependence has been removed.

## Coverage and remaining limits

Added 53 Python cases: three local alias setup forms tested across slice/PDG/
summary consumers, direct overwrite, pointee load, address-taking preservation,
40 generated pointer-copy chains with independently selected store/return
targets, unresolved pointer access, mixed unknown targets and pointer updates.
The three original store setups failed against the previous extension.

This is local, conservative alias modeling, not complete C memory analysis.
Target sets union across assignments and control paths, so they can contain
spurious alternatives. Fields/array elements, full pointer arithmetic,
multi-level pointer operations, external-call memory effects, and general
cross-function global/pointer mutation still require further work. Consumers
must inspect incompleteness before interpreting an omitted dependence as
independence. The graph/slice API's diagnostic transport still needs improvement.

Release ABI3 extension rebuilt for validation. Final gates: 448 Python tests
passed, ten inherited CLI skips, three optional Joern tests deselected; 572
Rust core plus three binding tests passed, one core test ignored. Formatting,
Rust 1.88 Clippy, Ruff and ty checked. The original review suite is 47 passed /
one failed, with absent reference documents as its remaining failure. This
does not close the other limitations listed above or the broader QA goal.

The statement benchmark (Python 3.14.3, release, seven batches of five after
warmup) measured median `call_summaries` times of 0.120 / 0.400 / 2.251 ms for
32 / 128 / 512 assignments before the final pointer-update classification tweak.
The preceding iteration measured about 1.94 ms at 512 assignments. Memory event
processing and weak-update handling add work even to scalar inputs; preserve
this baseline for a subsequent fast-path/performance investigation.

No Joern/DecBench execution, upstream interaction, or publication occurred.
