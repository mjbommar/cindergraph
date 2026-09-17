# Porting Glaurung's four DecBench parity corrections, 2026-09-17

Date: 2026-09-17. Lane CG-PARITY, branch `cg-parity-2026-09-17` on top of
`de1865c`.

Glaurung switched from an embedded copy of this stack to the crate on
2026-09-17 (its `docs/decisions/source-001-depend-on-cindergraph-by-git-rev.md`
and `docs/development/cindergraph-migration-2026-09-17.md`). The migration
found four parity corrections Glaurung made on 2026-09-13 that exist nowhere in
cindergraph, each measured there over DecBench's 85,645-cell oracle:

| Glaurung commit | correction | where it lives |
| --- | --- | --- |
| `828a41a9` | deduplicate parallel edges before `parity_chains` reads degree | `parity/chains.rs` |
| `0266715f` | drop the impossible false exit of a syntactically constant-true loop, keeping the header | `parity/nodes.rs` |
| `68b39f18` | elide a bare literal `if` test, keeping its fork | `parity/nodes.rs` |
| `f9a5cbaa` | collapse the duplicate loop header around a ternary loop test | `parity/nodes.rs` |

This note is the sizing (section 1), the `for (;;)` verdict (section 2), and
the measurement after the port (section 3).

## 1. Sizing: what the crate produces today on each correction's own inputs

Method: each correction's unit-test inputs through `parity_cfgs()` of the
crate at `de1865c` (the Python provider, release extension). "Glaurung
expects" is the value the carried test asserts, projected the same way; each of
those was a fresh Joern differential in Glaurung's campaign record
(`docs/history/campaigns/decbench-joern-replacement-2026-09-13/DIFFERENCE-REVIEW.md`).

| correction | input | crate today | Glaurung expects | same bytes? |
| --- | --- | --- | --- | --- |
| dedup | `int f(int x){ if (x) {} return x; }` | 2 nodes, `[(0,1)]`, entry `[0]`, exit `[1]` | 1 node, no edges, entry `[0]`, exit `[0]` | no |
| dedup | `… if (x) {} else {} return x; …` | same | same | no |
| constant loop | `while (1) { if (x) break; x--; }` | 5 nodes, 6 edges; the header keeps its false exit | 4 nodes, 4 edges (false exit gone, header absorbed by contraction) | no |
| constant loop | `while (0) x--;` / `while (x) x--;` | 4 nodes, 4 edges, false exit kept | unchanged | yes |
| constant loop | `while (1) {}` | 2 nodes, `[(0,1),(1,1)]`, exit `[]` | the same shape (a header with a self-edge) | yes, by accident: F-12 drops the singleton funcend *after* the header's false edge has already forced it out of a chain |
| literal `if` | `if (1) { if (0) g(); } h();` | 4 nodes, 5 edges | 3 nodes, 3 edges: entry forks to `g()` or `h()`, `g()` reaches `h()` | no |
| literal `if` | `if (x) g(); h();` | 3 nodes, 3 edges | unchanged | yes |
| ternary loop test | `while (i < (x ? 14 : 8)) i++;` | 5 nodes, 6 edges (two consecutive forks over the same condition span) | 4 nodes, 4 edges | no |
| ternary loop test | `while (i < (x ? g() : 8)) i++;` and the cast/compare/load variants | 6–7 nodes with a `LoopHeader` block | no `LoopHeader`, one node fewer | no |
| ternary loop test | `while (i < 10) { x ? g() : h(); i++; }` | one loop header | unchanged | yes |

So none of the four is produced by the crate today; the one coincidence
(`while (1) {}`) is a shape, not a rule, and Glaurung's test also asserts the
rule's census counter. All four need porting.

Which of these differences can this repository's benchmark answer? The
recorded Joern comparison
([`joern-decbench-2026-09-15.md`](joern-decbench-2026-09-15.md),
[`data/joern-complete-2026-09-15.json`](data/joern-complete-2026-09-15.json))
holds, per function over 930 functions in 210 translation units, Joern
4.0.150.4's node count, edge count and entry/exit-role counts — not the
graphs. Over that population the crate at `de1865c` reproduces the recorded
cindergraph counts on 930/930 functions, and count-equality with Joern's
recorded counts coincides with the recorded `vj_ged == 0` on all 930, so
count-parity is a faithful proxy for VJ-GED zero on this corpus and is what
section 3 measures. What the corpus contains of each correction's shape:

- constant-true loops: **no** `while (<literal>)` or `do … while (<literal>)`
  in fixture code (every `while (1)` hit is in a comment); `for (;;)` appears
  twice (`125_loop_shapes.c::infinite_with_internal_exit`,
  `18_binary_heap.c::heap_pop`), both with a reachable `break`, both already
  VJ-GED 0 under `elide_empty_for_headers`;
- literal `if` tests: **none**;
- a ternary in a loop test: **none** in the condition; one in a `for` *init*
  clause (`111_self_referential_struct.c`), which is a negative fixture for the
  rule's containment check;
- parallel edges from an empty `if` arm: measured by the section 3 diff rather
  than by grep, because the shape is a property of the S2 graph.

So the benchmark can confirm that the corrections do not regress the 894 exact
graphs and can catch the dedup correction firing on real fixtures; it cannot
adjudicate the other three, whose evidence is Glaurung's fresh Joern
differentials carried in the ported tests.

## 2. The `for (;;)` conflict

Glaurung's constant-loop rule treats a clause-less `for (;;)` as constant-true:
it removes the header's false exit and keeps the header, on the stated ground
that "Joern incorrectly erases a truly infinite loop". Cindergraph's
`elide_empty_for_headers` (roadmap Class B) deletes the header outright and
routes entry and back edges to the first body block, as Joern does.

What the recorded data says:

- Joern 4.0.150.4 on `for (;;) { if (i >= limit) break; i++; }` is **four
  nodes and four edges with no header node** — the unit oracle
  `a_conditionless_for_has_no_structural_header`, measured directly, plus the
  two corpus functions above at VJ-GED 0. This is a direct measurement.
- Joern on `void infinite(void) { while (1) {} }` is **one node and no edges**
  — Glaurung's fresh minimal differential in `DIFFERENCE-REVIEW.md` ("35: Joern
  erases infinite loops"). Glaurung keeps a header with a self-edge on purpose.
- Joern on `for (;;) {}` with an empty body, on `for (;;) { x++; }` with no
  exit, and on `do { … } while (1)` is **not recorded** in either repository.
  The first is presumably erased like `while (1) {}`; that is an inference, not
  a measurement.

Where the two rules give the same bytes: on any `for (;;)` whose body's first
node has no predecessor other than the header — which is every `for (;;)` in
the corpus and the unit oracle. Removing the false exit leaves the header with
out-degree 1 into a node of in-degree 1, and `parity_chains` contracts the pair,
so the projected graph is the one `elide_empty_for_headers` produces by deleting
the header. Section 3 checks this by running both.

Where they differ: (a) an empty body, where cindergraph erases the loop (the
crate today gives `for (;;) {}` one node) and Glaurung keeps a self-cycle; (b)
a body whose first node is a label some `goto` inside the loop also reaches,
where Glaurung's kept header cannot contract and survives as a block Joern's
measured graph does not have.

Verdict: keep `elide_empty_for_headers` for the empty condition, because it is
the rule this benchmark measured against Joern; port the constant-true
false-exit removal for `while (<nonzero literal>)`, `do … while (<nonzero
literal>)` and `for (…; <nonzero literal>; …)` — shapes no cindergraph rule
touches today, where the false exit is infeasible by C semantics and where
Glaurung measured the removal at +227 net exact cells. A clause-less `for` is
excluded from the ported detector by construction (an empty `ForCond` has no
expression to be a literal), so the two rules never both fire.

The residual inconsistency is stated rather than hidden: after the port,
`while (1) {}` keeps a header with a self-edge (two nodes, the bytes the crate
already produced, now pinned by `constant_true_empty_loop_keeps_its_cycle`)
while `for (;;) {}` is erased to one node. Joern erases both, on the one
measurement there is. Whether this crate should follow Joern there or retain
the reachable non-returning cycle under the roadmap's rule 2 ("when Joern's
graph contains a parser artifact, Cindergraph must retain the correct graph")
is a policy decision this lane does not take; the fixtures that decide it are
`void f(void) { for (;;) {} }` and `void f(void) { while (1) { x++; } }`
through Joern 4.0.150.4, which was not available on this host.

## 3. Measurement after the port

Joern was not installed on this host, so the comparison the benchmark doc
describes (`tools/compare_joern_decbench.py`, which invokes pyjoern) **did not
run**. What ran instead is the half of it that needs no Joern: the crate's
projection over the same 210 translation units / 930 functions, through the
same provider path (`analyze_decompiled`, release extension), compared per
function against Joern's *recorded* node, edge, entry-role and exit-role counts
in `data/joern-complete-2026-09-15.json`. "Count-parity" below is equality of
all four counts. On this population it is a faithful proxy: at the baseline it
agrees with the recorded `vj_ged == 0` verdict on 930 of 930 functions.

| state | commit | parity unit tests | count-parity | functions whose projection bytes changed |
| --- | --- | ---: | ---: | ---: |
| baseline, `de1865c` | — | 67 | 894 / 930 | — |
| + dedup | `828a41a9` port | 68 | 894 / 930 | 0 |
| + constant-true loop | `0266715f` port | 72 | 894 / 930 | 0 |
| + literal `if` | `68b39f18` port | 74 | 894 / 930 | 0 |
| + ternary loop test | `f9a5cbaa` port | 78 | 894 / 930 | 0 |

The final dump is byte-identical to the baseline dump over all 930 functions
(same SHA-256 of the serialized projections), so the DecBench projection's
bytes on every existing benchmark fixture are unchanged — the corpus simply
holds none of the four shapes, as section 1 predicted from the source. The
corrections' evidence is therefore the carried unit tests (each pinned to a
fresh Joern differential from Glaurung's campaign) plus the mutation controls,
not a corpus movement. Per correction:

| correction | carried tests (all in `parity/`) | mutation control (`tools/mutation_controls_parity.py`) |
| --- | --- | --- |
| dedup | `chains::parallel_empty_branch_edges_are_deduplicated_before_coalescing` | reverting `outgoing.dedup()` kills exactly that 1 test |
| constant-true loop | `nodes::constant_true_loop_loses_only_its_impossible_false_exit`, `constant_false_and_unknown_loops_keep_their_false_exits`, `constant_true_empty_loop_keeps_its_cycle`; new here: `a_conditionless_for_is_not_a_constant_true_header` | disabling the literal proof kills exactly the 3 that assert the rule; the constant-false/unknown negative survives |
| literal `if` | `nodes::nested_literal_if_tests_cost_no_nodes_but_keep_their_forks`, `a_variable_if_test_still_materializes` | disabling the literal test kills exactly 1; the variable-test negative survives |
| ternary loop test | `nodes::value_only_ternary_in_a_loop_condition_has_one_final_branch`, `a_side_effecting_ternary_arm_keeps_the_expression_but_not_the_duplicate_header`, `ternary_loop_cast_compare_and_load_variants_keep_their_expression_nodes`, `a_ternary_at_the_start_of_a_loop_body_is_not_a_loop_test` | disabling the collapse kills exactly 3; the loop-body negative survives |

The harness itself was checked to fail: with one expected test name
substituted, it exits 1 naming the actual kill set. Its first run against a
reused target directory reported the *baseline* failing the three ternary
tests — the copy had preserved source mtimes older than the last mutant's
artifact, and cargo reused that artifact. The copy now refreshes every mtime.

What the corrections do to the shapes they target, through `parity_cfgs()`
after the port (the same inputs as section 1):

| input | before | after |
| --- | --- | --- |
| `if (x) {} return x;` | 2 nodes, 1 edge | 1 node, 0 edges, entry `[0]`, exit `[0]` |
| `while (1) { if (x) break; x--; }` | 5 nodes, 6 edges | 4 nodes, 4 edges — the bytes of the same loop spelled `for (;;)` |
| `while (1) { x++; }` | 3 nodes, 3 edges | 2 nodes, `[(0,1),(1,1)]` — the bytes of `for (;;) { x++; }` |
| `while (1) {}` | 2 nodes, `[(0,1),(1,1)]` | unchanged |
| `for (;;) {}` | 1 node, no edges | unchanged (the stated residual difference from `while (1) {}`) |
| `do { if (x) break; x--; } while (1);` | 4 nodes, 5 edges | 4 nodes, 4 edges |
| `if (1) { if (0) g(); } h();` | 4 nodes, 5 edges | 3 nodes, 3 edges |
| `while (i < (x ? 14 : 8)) i++;` | 5 nodes, 6 edges | 4 nodes, 4 edges |

To re-run the Joern half once pyjoern is available: the commands in
[`joern-decbench-2026-09-15.md`](joern-decbench-2026-09-15.md) § Reproduction,
unchanged; the population hash must still be
`789a5d066e89b90034b184f66d61995d30bc48e104b50d5de28725009a06c8cc`, and the
expected outcome from the table above is 894 zero graphs and the same 36
nonzero. The fixtures that would extend the corpus to cover the four shapes —
and settle the section 2 question — are one translation unit with `while (1)`
/ `do … while (1)` / `for (;;)` in each of the empty, no-exit and
reachable-`break` forms, a literal `if`, and a ternary in a loop test; none has
been added here because no Joern output for them could be recorded on this
host, and a fixture without its oracle would pin the crate's own answer.
