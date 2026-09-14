# QA iteration 3: control provenance and indirect-call identity

Baseline: `cc3dc65`.

Provenance now incorporates the existing CFG control-dependence relation.
An influenced branch propagates to controlled writes and return statements;
reaching-definition edges still determine whether those writes survive later
assignments. Source spans connect conditional-expression operands across the
CFG nodes introduced by short-circuit/ternary lowering. Actual return spans
preserve the connection from a ternary condition to its returned value.

Seven focused cases cover ternary return/assignment, separate conditional
returns, guarded writes, an overwritten guarded write, and calls that preserve
or discard their argument before use as a condition. Five failed before the
repair; all pass afterward. Empty branches and overwritten values remain
negative. Control dependence is conservative: same-valued branch arms can
still produce a may-dependence; this is now explicit in the Python API docs.

Forty generated programs each contain eight conditional assignments. A small
independent interpreter evaluates nine input pairs; output differences when
exactly one input changes are witnesses that the summary must cover. All forty
pass. This finite check establishes coverage of those observed influences,
not equality with a complete semantic dependence relation.

An additional adversarial test found a parameter function pointer shadowing a
file-level function: `f(int (*keep)(int), int y){return keep(y);}` was treated as
a direct call to another definition named `keep`. Call collection now consults
the resolved binding at the callee span. The test verifies an incomplete
summary, no fabricated return flow, and `unknown` reachability to the unrelated
same-named function.

## Evidence and cost

- 350 Python tests passed; ten inherited CLI tests skipped, three optional
  Joern tests deselected. This increment adds 48 cases.
- 572 Rust core tests and three binding tests passed; one core test ignored.
- Review suite: 45 passed, three failed. Remaining failures concern pointer
  writes in slices, unresolved binding indices, and absent reference documents.
- Rust 1.88 Clippy, Rust/Python formatting, Ruff, and ty checked.
- Release ABI3 module rebuilt and used for Python testing.

The durable statement benchmark reports median summary times of 0.0676,
0.3389, and 1.9414 ms for 32, 128, and 512 assignments, respectively, on
Python 3.14.3. The previous ad hoc 512-assignment observation was 1.4938 ms.
Control-dependence construction and cross-node expression matching introduce
additional work; these runs do not justify claiming no performance regression.
Both remain far below the 73.9 ms repeated-scan implementation from iteration 2.

Continue with memory/field dependence, stable unresolved variable identities,
recovery completeness, and release/documentation quality. The broad QA goal is
still active. No Joern/DecBench execution or upstream interaction occurred.
