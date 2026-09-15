# Roadmap to close the Joern CFG differences

Date: 2026-09-15

This is the working disposition of every nonzero graph in the original
930-function comparison. The governing rule is semantic, not cosmetic:

1. when Joern models C control flow correctly, Cindergraph must match it; or
2. when Joern's graph contains a parser/preprocessor artifact, Cindergraph must
   retain the correct graph and the difference must receive a reproducible
   proof and regression test.

Merely driving VJ-GED to zero is not sufficient. VJ-GED reads degree and
entry/exit-role assignments, so unrelated errors can cancel.

## Progress ledger

| State | Functions | Share of 930 |
| --- | ---: | ---: |
| Original zero VJ-GED | 858 | 92.26% |
| Corrected by short-circuit loop-header rewrite | 32 | 3.44% |
| Corrected by conditionless-loop-header rewrite | 2 | 0.22% |
| Proven Cindergraph-correct macro artifacts | 16 | 1.72% |
| Proven Cindergraph-correct Joern entry-role artifacts | 4 | 0.43% |
| Proven Cindergraph-correct `offsetof` artifacts | 3 | 0.32% |
| Proven Cindergraph-correct branch-hint artifact | 1 | 0.11% |
| Proven Cindergraph-correct function-macro artifacts | 4 | 0.43% |
| Proven Cindergraph-correct complex-operator artifacts | 5 | 0.54% |
| Corrected by provider preprocessing | 2 | 0.22% |
| Proven Cindergraph-correct provider-boundary macro case | 1 | 0.11% |
| Proven Cindergraph-correct computed-goto artifact | 1 | 0.11% |
| Proven Cindergraph-correct conditional-store ordering | 1 | 0.11% |
| Remaining unadjudicated differences | 0 | 0.00% |
| **Current measured zero VJ-GED** | **894** | **96.13%** |
| **Exact or individually adjudicated** | **930** | **100.00%** |

The aggregate was remeasured over all 53 originally mismatch-bearing
translation units after both fixes: 167 of 205 functions were zero VJ-GED and
38 remained nonzero. The other 725 functions were zero in the immutable
baseline and their source files were untouched by the construct-specific
rewrite. The reduced-set result is
[`data/joern-remediation-pass-1-2026-09-15.json`](data/joern-remediation-pass-1-2026-09-15.json)
(SHA-256
`c51e716250c3a1b04d034382c24b41d7e2c8ae580bcdb44806f0908e56bdaff0`).
It records compared-source hash
`c4419a458eac2b7f15ae91668fde3705dc338fb0a5a63818817d5b4c7db04ebb`.

After all fixes, the complete 210-translation-unit/930-function population was
remeasured. It records 894 zero-VJ-GED graphs, 36 nonzero graphs, all 930
functions recovered by both providers, and zero provider failures. The raw
record is
[`data/joern-complete-2026-09-15.json`](data/joern-complete-2026-09-15.json)
(SHA-256
`b04b474117e4664e7260b6b2f1b1551869b2463af7bbe12be988ca79b300f380`).
Its population hash is the baseline's unchanged
`789a5d066e89b90034b184f66d61995d30bc48e104b50d5de28725009a06c8cc`;
its exact compared-source hash is
`e81eaa50f2a5cdfc959fc8cf230a9b9624d9bab43d6bc0f7f4de0a009e29b2dd`.

`tools/audit_joern_closure.py` fails closed unless the current nonzero identity
set equals the union of the proof artifacts. The completed audit covers all 36,
with no missing graph and no stale evidence:
[`data/joern-closure-audit-2026-09-15.json`](data/joern-closure-audit-2026-09-15.json)
(SHA-256
`4b413b2f6e624c739cda7ed96ec4b8f9d9ea6ff80a94ffb13654adeb98479eb2`).
The same audit compares the immutable baseline: 36 of its 72 differences became
exact and no new nonzero graph appeared.

## Class A: redundant short-circuit loop headers — fixed

### Root cause and adjudication

Cindergraph's general analysis CFG intentionally represents `while (a && b)`
with both a structural loop header and the expression-level short-circuit
graph. The structural header has a conservative direct-exit edge. That policy
is appropriate for the general over-approximating analysis graph, but not for
the Joern/DecBench parity projection.

In C semantics, entry and every back edge must evaluate `a`. If `a` is false,
control exits; otherwise it evaluates `b`, and only then selects the body or
exit. Joern's `str_cmp` graph has exactly this shape. The old parity graph also
allowed the structural header to exit directly, admitting a path that
evaluated neither operand. Joern is correct for this class.

The parity rewrite now removes the redundant header, discards its spurious
direct-exit edge, and routes entry/back-edge predecessors to the first operand.
The general analysis CFG remains conservative and unchanged.

### Graphs closed

| Original VJ-GED | Graphs |
| ---: | --- |
| 205 | `wide154_dense_effects` |
| 130 | `big151_branch_ladder` |
| 10 | `bst_inorder_checksum`, `kmp_search`, `factorize` |
| 5 | `cond_reload_and_transform`, `while_prefix`, `while_reload_header`, `atomic_compare_exchange_loop`, `bit165_read_sequence`, `find_by_key`, `l192_chase_keys`, `mc193_names_differ`, `dsu_find`, `merge_sort_i32`, `insertion_sort_i32`, `shell_sort_i32`, `counting_sort_u8`, `quickselect_kth`, `rle_encode`, `bounded_sample`, `extended_gcd`, `gcd_i32`, `mod_pow`, `bignum_mul_small`, `rational_gcd`, `molar_mass_centi`, `balance_gcd`, `remaining_balance`, `simple_moving_average`, `match_order`, `str_cmp` |

All 32 became VJ-GED zero. Nine other functions containing the same construct
also received the semantic repair but remain nonzero because another difference
is present. Their scores increased by 3 or 5 where the old header happened to
cancel the other error. They remain assigned to their underlying classes below;
this is why the gate counts zero graphs rather than summing score improvement.

### Permanent gates

- Rust unit oracle for `str_cmp`: six nodes, seven edges, matching the measured
  Joern graph and asserting that no path skips the first operand.
- `GranularityStats.loop_headers_elided` distinguishes an exercised rewrite
  from an accidentally passing graph.
- Reduced differential run over all formerly failing translation units.

## Class B: conditionless `for (;;)` headers — fixed

### Root cause and adjudication

An empty `for` condition evaluates no expression. Joern correctly has no CFG
node for it. Cindergraph's parity projection retained its structural loop
header and a direct-exit edge, even though an infinite loop can exit only via an
explicit transfer in its body. That was one extra node and two extra edges.

The parity layer now identifies AST `ForStmt` nodes with an empty `ForCond`,
removes the header and direct-exit edge, and routes entry/back edges to the
first body block.

### Graphs closed

- `125_loop_shapes.c::infinite_with_internal_exit`
- `18_binary_heap.c::heap_pop`

Both now have VJ-GED zero. The two containing translation units are 6/6 zero
and 6/6 role-preserving isomorphic.

### Permanent gates

- Rust unit oracle measured against Joern 4.0.150.4: four nodes and four edges.
- `GranularityStats.empty_for_headers_elided` proves the rule ran.
- Explicit two-translation-unit differential artifact.

## Class C: object-like macros materialised by Joern as calls — 16 proven

### Evidence and correctness rule

Joern's graph labels contain nodes such as `Call: SEG_LEAVES()` and
`Call: FLAT145_S_DONE()` even though the source declares these identifiers as
object-like macros, not functions. A C preprocessor replaces each occurrence
with its replacement tokens before parsing; no call, call return, or call edge
exists in the C abstract machine. Cindergraph must not add these phantom calls
to obtain a zero score.

This class needs a checked evidence generator that records, per graph:

- the exact `#define` declaration and whether it is object-like;
- every Joern `Call: NAME()` block for that identifier;
- the preprocessed token sequence at the source occurrence;
- Cindergraph's graph before and after preprocessing;
- a regression asserting that the parity graph has no node attributable only
  to the nonexistent call.

### Closed graphs

| Subclass | Graphs | Observed Joern phantom names |
| --- | --- | --- |
| Flattened state machines | `flattened_accumulate`, `flattened_classify`, `flattened_gcd`, `flattened_search` | `FLAT145_S_*`, `FLAT145_MAX_*` |
| Obfuscated state machines | `obfuscated_digest`, `obfuscated_transform` | `COMP150_S_*`, `COMP150_MAX_*`, `COMP150_OPS` |
| Dynamic programming | `edit_distance`, `lcs_length`, `lcs_recover`, `matrix_chain_cost` | `ED_MAX`, `LCS_MAX`, `CHAIN_MAX`, `CHAIN_INFINITE` |
| Constant-bound algorithms | `huffman_code_lengths`, `tlv164_seek`, `classify_binary32` | `HUFF_SYMBOLS`, `TLV164_*`, `FP174_*` |
| Containers/layout | `ring_push`, `ring_pop`, `segment_build` | `RING_CAPACITY`, `RING_MASK`, `SEG_LEAVES` |

There are 16 closed graphs. The executable adjudicator found at least one raw
Joern object-macro call in every graph; none of those calls survived real C
preprocessing. Cindergraph's raw and preprocessed graphs were role-preserving
isomorphic for all 16, while Cindergraph and Joern were role-preserving
isomorphic after preprocessing for all 16. This isolates the original
difference to Joern's treatment of unexpanded object-like macros.

The raw evidence is
[`data/joern-macro-adjudication-2026-09-15.json`](data/joern-macro-adjudication-2026-09-15.json),
generated by `tools/adjudicate_joern_macro_calls.py`. The artifact's SHA-256 is
`b88f375524fefb980ac66585b210953a0cdecc08ee21e13259285dd1e1872864`.
It records compared-source hash
`3aa6f0f55e8e98f3b7e7360d61e7e2b586cf79b0ac8ff5ea8cb1f23bf3e9adda`,
GCC 15.2.0, pyjoern 4.0.150.4, DecBench 1.1 and the pinned DecBench commit.

Two investigated graphs, `threaded_interpreter` and `parse_decimal`, also had
phantom macro calls, but Joern's raw and preprocessed graphs were already
role-preserving isomorphic. The calls coalesced without changing topology and
therefore do not explain their VJ-GED differences. They are correctly *not*
closed by this class and have been reassigned below.

### Exit criterion

The proof is checked by preprocessing rather than manually deleting graph
nodes: this uses the C language's actual macro semantics and cannot choose a
convenient graph rewrite. The fixture-local `#define` declarations are retained
while `#include` directives are removed, so system-header volume cannot obscure
the target functions. Each artifact row records the declaration, raw phantom
calls and all three graph comparisons.

## Class D: function-like macros and GNU statement expressions — four proven

### Assigned graphs

- `statement_expression_max`
- `single_evaluation`
- `total_weight`
- `obfuscated_pipeline`

The first two use GNU `({ ... })` statement expressions. `total_weight` uses
X-macro expansion, and `obfuscated_pipeline` uses the function-like `PICK`
macro plus a comma-expression `for` step.

### Roadmap

For all four, Cindergraph's raw and preprocessed graphs are role-preserving
isomorphic, Joern changes under preprocessing, and the two providers are
role-preserving isomorphic afterward. This proves the raw differences are
Joern macro/statement-expression recovery artifacts.

The executable record also tested `apply_opcode`. It correctly identified that
the raw parser and preprocessed graphs differ while both providers match after
preprocessing; the provider-boundary correction is recorded separately below.
Evidence:
[`data/joern-function-macro-adjudication-2026-09-15.json`](data/joern-function-macro-adjudication-2026-09-15.json)
(SHA-256
`5eae5670c2c2fe66c456a16aec1cc70b5bdcbb7dd59d0475af7dcc29c18a341c`).

## Class D2: X-macro-expanded switch — provider defect fixed

### Assigned graph

- `apply_opcode`

The raw parser graph has six isolated nodes because the Rust library's contract
deliberately accepts one already-prepared translation unit and does not execute
a preprocessor. The Python DecBench provider had failed to implement the
adapter contract already used for decompiled C elsewhere in the project.

`cfgs_from_decompiled` now preprocesses source containing conditional or macro
directives with the host C preprocessor, after removing includes, and fails
open to the original text if preprocessing is unavailable. `apply_opcode` now
produces its semantic 7-node/6-edge graph at the provider boundary. Its raw
Joern graph remains 12/11 because Joern materialises X-macro identifiers; the
providers are role-preserving isomorphic when both receive expanded C. This is
therefore closed as one provider fix plus the same checked macro-artifact rule
as Classes C/D, not by synthesising a switch for this fixture.

## Class E: conditional-compilation arms around GNU assembly — fixed

### Assigned graphs

- `asm_add_via_constraints`
- `save_flags`

The apparent assembly difference was not assembly lowering. Both fixture files
contain mutually exclusive architecture implementations under preprocessor
conditionals. Feeding all raw arms to the parser created disconnected regions.
After the provider preprocessing fix described in Class D2, each active
translation unit contains one implementation and both graphs become exact
zero-VJ-GED matches. No blanket assembly collapse was added; `asm goto` and
ordinary opaque assembly retain their existing distinct control contracts.

## Class F: layout builtins and packed types — Cindergraph proven correct

### Assigned graphs

- `layout_offsets`
- `offset_of_payload`
- `pk161_member_offset`

`offsetof`/`__builtin_offsetof` is an integer constant expression and introduces
no runtime branch or call. GCC verified every substituted offset, including the
packed offsets 1, 5 and 7. Cindergraph's graph was role-preserving isomorphic
before and after replacing every `offsetof` with the verified integer literal.
After substitution, Cindergraph and Joern were role-preserving isomorphic for
all three functions.

The raw Joern graphs had collapsed the two-arm conditional, four-way switch and
eight-way switch to one node with no edges. Those branches reappear when the
constant expressions are replaced by literals, proving that the loss is a
Joern parse artifact rather than runtime semantics. Cindergraph correctly
retains 4/4, 5/4 and 9/8 node/edge graphs respectively.

The executable proof is `tools/adjudicate_joern_offsetof.py`; the record is
[`data/joern-offsetof-adjudication-2026-09-15.json`](data/joern-offsetof-adjudication-2026-09-15.json)
(SHA-256
`bd24b04e9fc6d5b6e67193d9c62a47d5d95b013bbcc8c1c0a897a10f2a92f5a1`).

## Class G: weak symbols and ifunc attributes — Cindergraph proven correct

### Assigned graphs

- `weak_absent_probe`
- `weak_dispatch`
- `weak_fold`
- `ifn159_resolve`

Node and edge counts agree and all four directed topologies are isomorphic when
roles are ignored. Cindergraph gives each graph exactly one entry with
in-degree zero. Joern gives the graphs two, two, two and three entrypoints;
several of those marked entries have incoming edges.

A function CFG has one distinguished entry, and an entry cannot have a
predecessor within that CFG. Symbol binding and ifunc resolution can change
which body is selected, but cannot create multiple intra-function entries.
Cindergraph therefore satisfies the structural invariant and Joern's repeated
`FUNCTION_START` roles are artifacts. Copying them would make Cindergraph less
correct.

The executable proof is `tools/adjudicate_joern_entry_roles.py`; its raw record
is
[`data/joern-entry-role-adjudication-2026-09-15.json`](data/joern-entry-role-adjudication-2026-09-15.json)
(SHA-256
`8d54c31a133f380d44fb52067de86f319d45c462c0efcd5dff5ffba037584d68`).
All four topology checks and all four Cindergraph entry invariants pass; zero
of four Joern graphs has a unique zero-in-degree entry.

## Class H: complex arithmetic lowering — Cindergraph proven correct

### Assigned graphs

- `complex_add_conj`
- `complex_float_multiply`
- `complex_multiply`
- `complex_through_call`
- `complex_array_sum`

Cindergraph has four fewer nodes and six fewer edges in each scalar case; the
array loop has the same fixed delta. `_Complex` arithmetic and
`__real__`/`__imag__` operations have value semantics but no C-level branch.
The four scalar functions contain no control construct and therefore have one
basic block in Cindergraph; Joern invents a five-node/six-edge diamond around
the complex-part operators. The ordinary `struct_pair_control` is one block in
both providers.

For every assigned graph, the executable proof replaced only the final
straight-line arithmetic return expression with a scalar return. This changes
the computed value but cannot add or remove a control alternative. All five
Cindergraph graphs remained role-preserving isomorphic, and all five then
matched Joern. This isolates the raw difference to Joern's parsing of
`__real__`/`__imag__`; copying the diamond would introduce nonexistent control
flow.

Evidence: `tools/adjudicate_joern_complex.py` and
[`data/joern-complex-adjudication-2026-09-15.json`](data/joern-complex-adjudication-2026-09-15.json)
(SHA-256
`4931dedcb6f9ecfc90663fc5c169f61d1aaaebd6c9dc1a5aa25b9ef38d5c0835`).

## Class I: branch prediction intrinsic — Cindergraph proven correct

### Assigned graph

- `hinted_validation`

`__builtin_expect(x, expected)` evaluates and returns `x`; its second argument
is branch-prediction metadata and does not add a C control-flow alternative.
The fixture contains a source-equivalent `unhinted_validation` control.
Cindergraph produces role-preserving-isomorphic 13-node/16-edge graphs for the
hinted and control functions. Joern produces 17/26 for the hinted function but
13/16 for the control, and Cindergraph matches Joern on that control.

The only semantic difference between the paired source functions is the hint,
so Joern's four extra nodes and ten edges are frontend artifacts. The executable
proof is `tools/adjudicate_joern_branch_hint.py`; the record is
[`data/joern-branch-hint-adjudication-2026-09-15.json`](data/joern-branch-hint-adjudication-2026-09-15.json)
(SHA-256
`e2b9534bb4cb4d5911af853a099f76c962abc2181425be0bd6b2a0eb7b48a0db`).

## Class J: computed-goto dispatch — Cindergraph fixed and proven correct

### Assigned graph

- `threaded_interpreter`

The former Cindergraph representation encoded one computed transfer as a chain
of binary conditions and direct gotos, then recognised and collapsed that
accidental shape in the parity layer. That was a systemic design error.

The core vocabulary now has atomic `Flow::IndirectDispatch { targets, span }`
and `NodeKind::IndirectDispatch`. The C emitter supplies the address-taken label
set in source order; the generic builder creates one node, backpatches one jump
edge per target and permits no fall-through. Empty sets become diagnosed
divergence, and a partially unresolved set retains its valid targets. The
pattern-matching parity rewrite was removed.

The resulting Cindergraph graph has 14 nodes and 20 edges, including exactly
one four-successor dispatch. Joern has the same dispatch and topology plus one
non-entry node with in-degree zero containing only `targets[opcode]` and
`*targets[opcode]`. Removing only that unreachable Joern node makes the graphs
role-preserving isomorphic. It cannot represent an executed computation because
no entry-reachable path reaches it.

## Class K: conditional value stored through a pure lvalue — Cindergraph proven correct

### Assigned graph

- `parse_decimal`

The earlier three-term-loop hypothesis was false: labelled edge inspection
shows that both providers contain all three comparisons and both `&&` operator
nodes with the correct short-circuit exits.

The difference is the final
`*value = (negative ? -accumulator : accumulator)`. Joern chooses one ordering
allowed by C and places the side-effect-free address computation for `*value`
before the conditional, as a separate fork-source block. Cindergraph chooses
the other permitted ordering and coalesces that pure computation with the
post-conditional assignment. The lvalue is a non-volatile local pointer read;
its address computation has no side effect and adds no control alternative.

A source-equivalent control explicitly evaluates the conditional into a local
temporary and then stores it through `*value`. Cindergraph's raw and sequenced
graphs are role-preserving isomorphic at 26 nodes/35 edges, and Cindergraph and
Joern are role-preserving isomorphic on the sequenced control. The raw Joern
27/36 graph is therefore a legal but unnecessarily split evaluation ordering,
not a missing Cindergraph branch.

Classes J and K share the executable evidence generator
`tools/adjudicate_joern_final_cfgs.py`. Its record is
[`data/joern-final-adjudication-2026-09-15.json`](data/joern-final-adjudication-2026-09-15.json)
(SHA-256
`4a0606e9d48684f6cbd743be271bb96ba70721fbb97f4d6d9ac277b043fdbd8d`).

## Completed execution order

1. Classes A/B were repaired and checked across all originally failing units.
2. Macro, entry-role, layout, branch-hint, complex and statement-expression
   classes received executable source-equivalent adjudicators.
3. Provider preprocessing repaired X-macro and conditional-compilation input
   handling without changing the raw Rust parser contract.
4. Computed goto was redesigned in the core graph vocabulary, not patched in
   the parity projection.
5. The last apparent loop difference was isolated to conditional-store
   evaluation order and closed with an explicitly sequenced control.
6. The fresh 930-function rerun measured 894 exact and 36 nonzero graphs with
   zero provider failures. The closure audit maps all 36, and only those 36, to
   checked proof artifacts.

## Required evidence for “100%”

The target is satisfied only when all 930 graphs have one of these outcomes:

- zero VJ-GED and role-preserving isomorphism against Joern after a
  semantics-preserving Cindergraph fix; or
- a checked per-graph proof record showing the exact Joern artifact, the
  preprocessed/source-equivalent C semantics, and a Cindergraph regression test.

A graph is not closed merely because its VJ-GED improves, because its node and
edge counts match, or because another error cancels its score. Aggregate
progress is reported as both exact parity and adjudicated intentional
difference, with the two columns kept separate.
