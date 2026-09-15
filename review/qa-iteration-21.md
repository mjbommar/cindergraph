# QA iteration 21: honest global-effect coverage

Probe: `int g; void put(int x){g=x;} int f(int x){put(x);return g;}`.
Both functions reported complete summaries with no return flow, even though
the summary model has no global input/output slots to propagate this effect.
Local dataflow interns `g` as an unresolved binding; that information was not
used when determining summary completeness.

Summaries now require an empty unresolved-binding set to claim completeness.
Global reads/writes therefore expose uncertainty rather than certifying an
omitted dependence as independence. Local dataflow edges and memory coverage
flags are unchanged: this is specifically a summary coverage limitation.
The shared completeness flag propagates to callers through the worklist.

Five regressions failed before the change: direct/compound/increment/constant
global writes and a global read. A sixth case verifies that a local variable
shadowing a global retains its complete, precise scalar return flow.

This is not global-effect implementation. A constructive next step is stable
translation-unit global identities and explicit global input/output summary
slots, with call-site strong/weak update rules and tests for read-after-call,
overwrites, recursion and aliasing. Until then, unknown is the honest result.
The current coarse unresolved category may include constants/macros that a
richer resolver could classify more precisely; the docs state this tradeoff.

Validation against the rebuilt release ABI3 extension:
`uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`
reported 2,004 passed, ten skipped, three optional Joern cases deselected.
Baseline: `edf2777` plus pending QA increments. No publication or external
evaluator execution occurred; changes remain local and uncommitted.

Rust 1.88 workspace tests passed 578 cases with one ignored; Clippy, formatting,
Ruff/ty on the new test and diff whitespace checks passed.
