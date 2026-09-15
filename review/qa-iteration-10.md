# QA iteration 10: seeded scalar provenance oracle

Added 400 deterministic cases: seeds 0–199, each with and without direct calls.
Each builds 25 scalar updates from copies, constants, addition assignments and
increments; the call variant additionally uses identity and constant-return
helpers with proper declarations. A separate Python coefficient model tracks
the contribution of each of three input parameters to the selected return.
It does not consume Cindergraph graphs, definitions or summaries to compute
the expected answer.

The generated fragment uses unsigned arithmetic and nonnegative coefficients
below 2**32, avoiding cancellation of a nonzero coefficient modulo the unsigned
width on the intended 32-bit-unsigned target. This is a narrow algebraic oracle
for provenance, not a general C semantic or compiler differential oracle.
Constant terms are immaterial to the dependence set. Control flow, pointers,
recursion and external calls are deliberately outside this test's coverage.

All cases passed without a production fix. The suite now independently checks
that an overwrite discards old provenance, compound updates retain it, and
identity versus constant-return calls propagate or erase it appropriately.
Tests use fixed seeds and print the generated C on a failed assertion.

Local command `uv run pytest python/tests/ review/test_design_contracts.py -q`
reported 907 passed, ten skipped and three optional Joern cases deselected.
Ruff formatting/lint and ty passed for the new test. The extension is the same
release ABI3 build tested in iteration 9; no Rust source changed in this round,
so Rust gates were not rerun. No Joern/DecBench execution or publication occurred.

Baseline: `edf2777` plus pending iterations 6–9. This is additional evidence,
not proof that the broader analysis or release work is complete.
