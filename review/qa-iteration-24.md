# QA iteration 24: retain control of discarded-operand side effects

Adversarial follow-up found a regression introduced by iteration 23's comma
filter: `(x ? (y=1) : (y=2), y)` and its `&&`/`||` variants lost the dependency
on x. Value filtering used the entire function as the expression context when
deciding whether a use could control writes. The surrounding comma expression
therefore incorrectly suppressed the inner condition.

Dataflow now retains graph-local node spans. Control provenance evaluates each
use against its own CFG node's expression extent, while return/assignment
values retain their existing expression extents. This preserves the distinction
between the discarded value of a compound expression and the condition that
governs side effects within it. Three new positive regressions failed before
the change; a new constant-return negative control stayed negative. All 17
comma/call sequencing cases now pass.

Expanded `tests/fixtures/comma_value.c` with conditional and short-circuit side
effects for zero and nonzero inputs. Compiling via `gcc -std=c11 -O0` and `-O1`
and running both produced exit 0. Binaries remain at
`/home/mjbommar/.cache/cindergraph/comma-value.R0w0GC/conditional-o0` and
`conditional-o1`. This is concrete evidence for the fixture, not a general
proof of C expression semantics.

Rebuilt the release ABI3 extension. The added `DataFlow::node_spans` is a Rust
API field; Python dictionary schemas are unchanged. Baseline is `edf2777` plus
pending QA increments. No publication or external evaluator run occurred.

Final gates: 2,027 Python/review tests passed, ten skipped, three optional Joern
cases deselected; 578 Rust workspace tests passed, one ignored. Rust Clippy and
Ruff/ty on the regression file passed. Rustfmt requested one layout adjustment,
which was applied; the repeated format and diff whitespace checks passed.
