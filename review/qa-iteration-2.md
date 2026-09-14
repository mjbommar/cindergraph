# QA iteration 2: reaching values and call-site edges

Baseline: local commit `83675a6`. Production changes:

- Dataflow records actual return-node IDs and per-call argument-expression
  spans. Parameter origins follow reaching definitions, honoring scalar kills.
- Each enclosing call gates expression provenance by its callee's return
  summary. Nested calls that drop arguments no longer leak their input origins.
- Summaries retain explicit parameter-to-callee-argument transfers.
  `reaches()` traverses these edges with `(function, parameter)` visited states,
  including reordered arguments and recursive call graphs. It no longer
  connects unrelated functions by return-summary shape.
- A definition/use work queue computes local provenance. Repeated full scans
  were replaced after large-function timings exposed their cost.

Validation: 575 Rust tests passed (one ignored), 302 Python tests passed
(ten inherited CLI skips and three optional Joern tests deselected).
New coverage comprises 18 focused expression/call cases, 480 reachability
queries over 40 seeded six-function graphs with an independent argument-edge
oracle, and 40 random scalar programs checked against symbolic origin sets.
Ten of the first 18 cases failed before the implementation change. All 98 new
test cases pass after it. Rust 1.88 Clippy, Ruff, and ty passed.

The original review suite improves from eight failures to three: pointer-store
slicing, unresolved binding indexing, and absent reference documents remain.
Two new review assertions deliberately expose another unresolved contract gap:
`return x ? 1 : 0` and `if(x)return 1;return 0` omit control-mediated parameter
flow while claiming a complete summary. These assertions permit either a
represented flow or explicit incompleteness; neither is currently produced.
There are therefore five current review failures, not three.

## Performance observations

Linux, Python 3.14.3, release ABI3 extension; timing includes Python conversion.
The existing 128-small-function workload remains about 1.34 ms for summaries
and 1.40 ms for dataflow. These are local synthetic observations, not a general
performance guarantee.

A single function with `int y=x;`, N copies of `y=y+1;`, and `return y;`
exposed repeated-scan cost in the first provenance implementation. Median of
five invocations (same interpreter and build mode):

| Assignments | Repeated-scan ms | Work-queue ms |
| --- | ---: | ---: |
| 32 | 0.0993 | 0.0737 |
| 128 | 1.6299 | 0.2673 |
| 512 | 73.9005 | 1.4938 |

The 512-assignment workload improved about 49x relative to the first repaired
provenance algorithm. This is not a comparison with the original inaccurate
algorithm. Reproduce the workload with
`uv run python tools/bench_analysis.py --shape statements` (the durable harness
uses seven samples of five calls after warmup).

## Remaining design work

Control-mediated provenance, memory/field effects, and recovery uncertainty
still need representation. The existing 16-round summary cap remains and can
make unrelated summaries incomplete when a long chain exhausts it. Function
pointer shadowing and unusual expression forms need further adversarial tests.
No whole-language soundness or complete interprocedural-analysis claim follows
from the scalar and call-site improvements here. Continue the broader QA goal.
