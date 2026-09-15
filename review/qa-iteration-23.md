# QA iteration 23: comma value versus evaluated side effects

Four regressions reproduced false return provenance from discarded comma
operands: `return (x,0)`, assignment/return overwrite, initialization from
`(x,0)`, and control dependence on `(x,0)`. Seven initial positive/ordering
controls already passed.

Dataflow now retains AST-derived `(comma expression, discarded operand)` span
pairs. Provenance excludes a read's value only when the queried expression
contains that comma expression. Evaluated side effects use their own smaller
expression spans and still propagate. This distinguishes `(y=x,y)` from
`(y=x,y=0)` without parsing rendered names or suppressing the left operand's
execution. Two additional tests distinguish a discarded call result from a
discarded argument value: `(sink(x),0)` still transfers x into sink, while
`sink((x,0))` does not.

The checked-in `tests/fixtures/comma_value.c` independently exercises discard,
keep, overwrite and side-effect behavior. It compiled and exited 0 at both
`gcc -std=c11 -O0` and `-O1`, with outputs under
`/home/mjbommar/.cache/cindergraph/comma-value.R0w0GC/`. No undefined evaluation
order is needed for these comma-sequenced examples.

After rebuilding the release ABI3 extension, `uv run --no-sync pytest
python/tests/ review/test_design_contracts.py -q` passed 2,023 tests, with ten
skips and three optional Joern deselections. Rust 1.88 workspace tests passed
578, one ignored. Clippy and Ruff/ty on the new test passed. Rust formatting
requested one layout change in the new predicate, subsequently applied.

The added DataFlow field is a Rust API addition; Python dictionary keys are
unchanged. This addresses comma value semantics, not full expression evaluation
or interprocedural memory completeness. Baseline: `edf2777` plus pending QA
increments. Changes remain local; no publication or external evaluator run.
