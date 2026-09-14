# QA iteration 4: unresolved name identity

Baseline: `8d689c9`.

Unresolved reads and writes now have stable, dense binding IDs. Equal unresolved
spellings share an ID within a function; different names and local shadows do
not. IDs are appended after local bindings in sorted-name order. Final event
records no longer expose the `u32::MAX` placeholder as an index.

Rust `DataFlow::unresolved_bindings` records which IDs lack a local declaration.
Python binding records expose `is_unresolved`; their type is `None`. The solver
can now kill previous writes to the same global without killing other names.
Unresolved writes still escape local dead-store and unused-local diagnostics.
Call classification happens before remapping so an unresolved direct callee is
not confused with a locally declared function pointer.

Five focused tests failed against the old extension and pass after rebuilding:
three distinct spelling examples, repeated global writes, and local shadowing.
Forty generated twenty-write programs independently compute the last write of
the returned global and check every event's binding/name join. All pass.

The corpus type gate previously counted only declared local bindings. It now
reports **3,602 typed / 4,176 total**, including **574 unresolved** names, and
**3,602 / 3,602 recovered declarations typed**. The original >95% declaration
coverage requirement is preserved; the all-binding percentage is not presented
as 100%. Every event ID is range-checked in that corpus test, and every
unresolved binding must have an empty type. There are no type conflicts or
unused declared bindings in that census.

The interning implementation uses a name-to-ID map and remaps each event once,
rather than scanning all events for every unresolved name. Ad hoc release ABI3
timing on Python 3.14.3, median of seven invocations of `data_flow()`:

| Distinct globals written | Binding records | Reaching edges to returned g0 | Median ms |
| ---: | ---: | ---: | ---: |
| 32 | 32 | 1 | 0.0786 |
| 128 | 128 | 1 | 0.2739 |
| 512 | 512 | 1 | 1.3195 |

The workload is available as `tools/bench_analysis.py --shape globals`; the
durable harness uses its existing warmup and seven batches of five calls.
These timings are a new baseline, not evidence of speedup over the incorrect
shared-identity implementation.

Final validation: 395 Python tests passed, with ten inherited CLI skips and
three optional Joern deselections. Rust workspace tests passed: 572 core plus
three binding tests, one core test ignored. Rust 1.88 Clippy, formatting, Ruff,
and ty passed. Review suite is now 46 passed / two failed: pointer-store slicing
and absent standalone reference pages remain. Alias analysis, cross-function
global effects, and recovery completeness still require work beyond assigning
distinct names. The overall QA goal remains active.
