# QA iteration 22: parameter-address output false positives

The summary builder emitted `Sink::Parameter(index)` whenever a parameter's
address was taken. This confuses a callee's by-value parameter object with
caller-visible pointee storage: even `int f(int x){int *p=&x;return x;}` claimed
an output through parameter zero. Four new cases failed before removing that
inference, including scalar/pointer local writes and passing `&x` externally.

Added `tests/fixtures/parameter_address.c`. Both commands completed with exit 0:

```bash
gcc -std=c11 -O0 tests/fixtures/parameter_address.c -o /home/mjbommar/.cache/cindergraph/parameter-address.PQsHxO/o0
/home/mjbommar/.cache/cindergraph/parameter-address.PQsHxO/o0
gcc -std=c11 -O1 tests/fixtures/parameter_address.c -o /home/mjbommar/.cache/cindergraph/parameter-address.PQsHxO/o1
/home/mjbommar/.cache/cindergraph/parameter-address.PQsHxO/o1
```

The fixture verifies that replacing the local scalar/pointer parameter object
does not change the caller's scalar or pointer. A Python regression analyzes
that same checked-in source and rejects parameter-output claims. Another
regression retains incompleteness for `*out=x`, whose genuine caller-visible
effect still needs implementation.

`Sink::Parameter` remains an API variant reserved for a future interprocedural
memory model; the current implementation does not emit it. Rust/Python docs
now state that limitation rather than claiming pointer outputs are supported.
This removes unsupported positive claims, not the need to implement real
pointee summaries and call-site updates.

Release ABI3 rebuild followed by the Python/review suite: 2,009 passed, ten
skipped, three optional Joern cases deselected (before adding the fixture-based
sixth test). Rust 1.88 workspace tests: 578 passed, one ignored. No publication
or external evaluator run. Baseline: `edf2777` plus pending QA changes.
