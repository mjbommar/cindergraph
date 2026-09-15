# QA iteration 11: dependency-driven summary fixed point

Reproduced declaration-order sensitivity: a 25-function identity chain in
forward order yielded zero complete summaries and no return flow for `f0`;
the same definitions reversed yielded 25 complete summaries and the expected
return flow. The old implementation scanned all functions for at most 16
rounds, regardless of which summaries changed.

The fixed point now tracks direct-call dependents and queues a caller only
when its callee's summary changes. All functions receive an initial evaluation.
Duplicate-definition refusal is preserved. The total budget remains bounded
at 16 times the input function count in function evaluations, with all summaries
marked incomplete if the queue remains nonempty at exhaustion. This is not an
unbounded solver or a guarantee of order-independent answers under exhaustion.

Regression coverage compares complete outputs for 25- and 128-function chains,
forward, reversed and seed-701 shuffled, with identity and constant-return
terminals. Both identity-chain tests failed before the fix. A 32-parameter
self-recursive rotation additionally verifies that budget exhaustion still
produces incompleteness and an absent-sink `unknown` answer.

Local release ABI3 extension rebuilt with `TMPDIR=/home/mjbommar/.cache/cindergraph
uv run maturin develop --release`. Rust 1.88 workspace tests: 575 passed, one
ignored. The initial Python/review suite passed 911 cases, ten skips and three
optional Joern deselections; the additional budget regression passed in the
five-case focused suite. The final full Python/review rerun passed 912 cases
with the same skips/deselections. Ruff, ty, Rust 1.88 Clippy, formatting and
strict rustdoc passed.

Baseline: `edf2777` plus pending iterations 6–10. No performance percentage is
claimed here: fewer redundant evaluations is an algorithmic change, not itself
a timing measurement. No publication, Joern or DecBench execution occurred.
