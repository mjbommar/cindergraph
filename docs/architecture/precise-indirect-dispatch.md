# Precise indirect dispatch with explicit uncertainty

Date: 2026-09-15. Status: active design with an implemented first slice.

## 1. Decision

Cindergraph should resolve each GNU C computed `goto` to its own conservative
may-target set and attach a machine-readable account of whether that set is
exact or a fallback. It must never trade CFG soundness for a smaller graph.

The governing invariant is:

```text
valid run-time targets(dispatch) subset-of emitted CFG targets(dispatch)
```

When the analysis cannot prove a smaller set, it must emit every label whose
address is taken in the containing function. That is the current behavior and
remains the sound fallback. An incomplete analysis must not return an empty or
apparently exact set.

This is the next semantic milestone, not a Joern-parity exercise. The desired
result is a useful public CFG contract: consumers can distinguish a precisely
resolved four-way dispatch from a conservative function-wide fan-out without
re-parsing C or guessing from node degree.

## 2. Current implementation and gap

The structural repair is already present:

- `Flow::IndirectDispatch` represents one computed transfer atomically.
- `NodeKind::IndirectDispatch` gives it a stable public identity.
- `CfgBuilder` emits one `Jump` edge per target and no fall-through edge.
- missing targets are diagnosed as divergence instead of becoming an
  unexplained edge-free ordinary statement.
- the C front end collects `&&label` occurrences in deterministic source order.

Direct label values, conditional joins of supported values, chains of
structurally immutable scalar pointer initializers, and immutable unescaped
label-address arrays now resolve per dispatch. Other expressions retain the
function-wide `Reach::address_taken` set as their conservative fallback.
The reachability prepass owns one resolution table that graph emission reuses,
so graph shape and exported precision cannot diverge between duplicate solver
runs. It consumes that resolution before graph emission, so
labels excluded by an exact dispatch need not be kept as unreachable executable
nodes. A constant decimal table index is range-checked and selects one slot.
Supported scalar assignments join into a finite may-set. Because this first
reassignment model is flow-insensitive, its target set is marked
`Conservative` with `flow_insensitive_join`, even when every source is known;
an assignment after the dispatch may otherwise add an infeasible target.
Unsupported right-hand sides, compound writes, escape, or ambiguous ownership
still trigger the function-wide fallback. Point-sensitive strong updates,
nontrivial range proofs and shared memory constraints remain unresolved.

Data-flow analysis consumes the CFG's sparse dispatch metadata. A conservative
dispatch records `unresolved_control_target` in the `control_targets` coverage
dimension at that statement's span and makes interprocedural summaries
incomplete. Exact valid-target sets remain complete even when
`may_be_invalid` records a possible non-destination outcome such as an
unproven table bound.

This should not be fixed in the generic CFG builder. The builder does not know C
expressions, objects, initializers, aliases, or recovery state. Its job is to
validate and wire a resolution supplied by the language front end.

## 3. Research basis

The design follows four primary sources:

1. GCC's [Labels as Values](https://gcc.gnu.org/onlinedocs/gcc/Labels-as-Values.html)
   specifies `&&label` as a `void *` constant and `goto *expr` as the computed
   transfer. It explicitly shows label addresses stored in static arrays. A
   transfer to a label in another function is not a valid execution.
2. LLVM's [basic-block address](https://llvm.org/docs/LangRef.html#addresses-of-basic-blocks)
   and [`indirectbr`](https://llvm.org/docs/LangRef.html#indirectbr-instruction)
   contracts require the instruction to list the full set of possible
   destinations. A possible target omitted from that list is undefined in LLVM
   IR. This is a strong precedent for making the candidate set part of the
   control-transfer representation rather than reconstructing it later.
3. LLVM's [Alias Analysis](https://llvm.org/docs/AliasAnalysis.html#must-may-and-no-alias-responses)
   distinguishes `MayAlias` from facts strong enough to exclude an alias and
   specifies conservative Mod/Ref answers whenever an access might occur. The
   same discipline applies here: unresolved storage or escape widens the set;
   it never justifies removing a destination.
4. Clang's source-level [CFG API](https://clang.llvm.org/doxygen/classclang_1_1CFG.html)
   has a distinguished indirect-goto block. That supports retaining the
   first-class node Cindergraph now has, although Cindergraph should expose more
   useful target and uncertainty data than that API boundary alone provides.

These sources do not prescribe Cindergraph's exact data structures. The lattice,
fallback policy, and sparse metadata layout below are project decisions derived
from their semantic constraints and from Cindergraph's existing architecture.

## 4. Semantic contract

### 4.1 What a target means

An emitted successor means that the abstraction permits a valid execution of
the computed transfer to land at that label. It is a **may** edge, not a claim
that the path is feasible for every input.

Invalid pointer values and out-of-range table accesses are not additional label
destinations. They are potential undefined behavior and must be represented as
uncertainty/diagnostic metadata. Cross-function labels likewise must not become
CFG edges in the current function.

### 4.2 Resolution result

Use one common result type from expression resolution through graph export:

```rust
pub struct IndirectTargets {
    pub labels: Vec<Symbol>,
    pub precision: TargetPrecision,
    pub may_be_invalid: bool,
    pub reasons: Vec<DispatchUncertainty>,
}

pub enum TargetPrecision {
    Exact,
    Conservative,
}
```

`labels` is sorted in source order and deduplicated. `Exact` means that, under
the declared language and recovery assumptions, every valid destination and no
other destination is present. It does not prove the dispatch expression itself
is valid. `Conservative` means the set contains a fallback or a widened fact.

`may_be_invalid` is separate because “all valid labels are known” and “the
expression cannot exhibit undefined behavior” are different claims. For
example, an unproved array bound may leave the set of valid table entries exact
while making the access potentially invalid.

Initial stable reasons should be deliberately small:

- `unresolved_value`
- `unknown_memory`
- `escaped_label_address`
- `unsupported_pointer_arithmetic`
- `index_may_be_out_of_bounds`
- `recovered_syntax`
- `budget_exceeded`
- `flow_insensitive_join`

Reasons are sorted and deduplicated. Adding uncertainty must only preserve or
widen `labels`, change `Exact` to `Conservative`, or set `may_be_invalid`; it
must never narrow a set.

### 4.3 Internal lattice

The solver needs an internal bottom value while reaching a fixed point:

```text
Bottom
Finite(labels, exactness, may_be_invalid, reasons)
TopWithinFunction(address_taken_labels, reasons)
```

Join unions label sets and reasons, ORs `may_be_invalid`, and retains `Exact`
only if every joined input and the joining operation are exact. An unknown
label-pointer origin becomes `TopWithinFunction`, not `Bottom` and not an empty
finite set. Because the valid GNU C destination universe is function-local and
finite, convergence is monotone and bounded.

`Bottom` is never a public answer. If it survives at a dispatch, finalize it as
the function-wide fallback with `unresolved_value`.

## 5. Ownership and data layout

### 5.1 C semantic resolver

Create a narrow resolver under `csource/semantic/` or `csource/eval/`; do not
add another recognizer to `cfg/reach.rs`. It should consume resolved expression,
binding, place, initializer, and recovery facts as those become available under
the broader [semantic foundation plan](semantic-foundation-plan.md).

The first implementation may use today's syntax arena, but its public boundary
must be semantic:

```text
resolve_indirect_targets(function, dispatch_expression) -> IndirectTargets
```

The resolver should reuse the existing dataflow vocabulary and worklist design.
`dataflow/memory.rs` already establishes the essential rule that local points-to
sets are may sets and never justify killing a target. Do not build a second,
incompatible theory of pointer escape and unknown memory. Extract shared
constraint primitives when doing so is smaller than translating between two
models.

### 5.2 Generic CFG

Change the event to carry the resolution:

```rust
Flow::IndirectDispatch {
    resolution: IndirectTargets,
    span: Span,
}
```

The builder validates it, creates the node, backpatches one `Jump` edge per
label, and records the resolution. Retain `EdgeKind::Jump`: it already states
the control semantics, while direct-versus-indirect provenance belongs to the
source node and its metadata. Adding an `Indirect` edge kind would needlessly
change graph consumers and degree/parity projections.

Do not enlarge every `CfgNode`. Add sparse metadata to `Cfg`, keyed by node ID
and stored as a node-sorted compact vector after construction:

```rust
pub struct IndirectDispatchInfo {
    pub node: NodeId,
    pub precision: TargetPrecision,
    pub may_be_invalid: bool,
    pub reasons: Vec<DispatchUncertainty>,
}
```

The actual targets remain the node's successor edges, preserving one source of
truth. The side table records qualifications only. `Cfg::indirect_dispatch(id)`
and `Cfg::indirect_dispatches()` expose it without penalizing ordinary CFGs.
Alternate projections built through `Cfg::from_parts` initially have no such
metadata and must not synthesize precision claims.

### 5.3 Coverage integration

Add a control-target coverage dimension (for example `ControlTargets`) to the
existing `CoverageDimension` system, and map dispatch uncertainty into the
existing reason-carrying `SemanticIssue` envelope. This makes an incomplete
dispatch qualify negative control/dependence answers consistently with unknown
memory and recovered syntax.

Avoid two unrelated truth systems. The node-level `DispatchUncertainty` is the
specific explanation; function-level coverage is the conservative aggregate.

## 6. Resolution algorithm

### 6.0 Solver boundary

This analysis does not require an SMT solver. Its core domain is a finite
function-local set of labels with monotone union, so a small deterministic
worklist is sufficient and keeps the Rust crate and Python wheel free of a
solver runtime. Constant index and interval reasoning should first use a small
bounded abstract domain in the core crate.

Axeyum or another solver may be used outside the shipped dependency graph as a
development oracle, differential-test backend, or future opt-in provider for
genuinely relational path constraints. Publication artifacts must not acquire
that dependency merely to improve computed-goto resolution.

### 6.1 Constraint extraction

Build inclusion constraints for label-pointer values and aggregate slots:

```text
&&L                    => {L}
p = q                  => targets(q) subset-of targets(p)
p = cond ? q : r       => targets(q) union targets(r)
table[k] = p           => targets(p) subset-of targets(table[k])
p = table[k_constant]  => targets(table[k_constant])
p = table[i_unknown]   => union of addressable table slots
```

Parentheses, compatible casts, and the value-producing arm of the comma
operator are transparent. A static or automatic aggregate initializer creates
slot constraints in declaration order. Reassignment unions in the initial
flow-insensitive stage; it must not perform a strong kill without flow evidence.

Represent variables and aggregate slots by stable semantic IDs, not source
spellings. Use adjacency indexes plus a `VecDeque` worklist. Each label can
enter each value once, giving a practical bound proportional to constraints
times the number of address-taken labels. Use dense bitsets when the label
universe warrants them, following the existing points-to implementation.

### 6.2 Widening rules

Widen to the function-wide address-taken universe when:

- the value comes from unmodeled memory or an unknown call;
- a storage object holding label addresses escapes and mutation cannot be
  excluded;
- pointer arithmetic cannot be related to a known label table;
- syntax recovery obscures the value expression;
- the configured analysis budget is reached.

Known candidates discovered before widening remain visible, but the final edge
set is the full fallback. This makes failure monotone and prevents accidental
false precision.

### 6.3 Dispatch evaluation

After constraint convergence, evaluate each computed-goto expression to a
`TargetValue`, finalize it against the function's label universe, and pass that
resolution to `Flow::IndirectDispatch`.

The CFG must not depend on a flow analysis that itself requires the final CFG.
The initial resolver is therefore syntax/semantic and flow-insensitive. Later
flow-sensitive refinement may consume a conservative bootstrap CFG, but it may
replace the broad graph only after proving its result is a subset-safe
refinement. On failure, retain the bootstrap edges.

### 6.4 Arrays and bounds

For the common form:

```c
static void *targets[] = { &&add, &&sub, &&mul, &&done };
goto *targets[opcode];
```

an immutable, unescaped table with a constant in-range index resolves to one
label. A proven in-range but otherwise unknown index resolves to the four table
labels. If the bound is not proven, the same valid labels remain candidates and
`may_be_invalid` plus `index_may_be_out_of_bounds` records the possible invalid
access.

A write through an alias or escape to unknown code prevents an exact immutable
table claim and triggers conservative widening. Merely seeing another label
address elsewhere in the function must not contaminate an exact table.

Relative label-offset tables, such as GCC's `&&foo - &&foo` extension, are a
later supported fragment. Until modeled, they widen with
`unsupported_pointer_arithmetic`.

## 7. Public API and serialization

Rust should expose target symbols or resolved label node IDs, precision,
invalidity, reasons, and source span. Python should expose the same facts through
`SourceCfg`, using stable strings:

```python
cfg.indirect_dispatches()
# [{
#   "node": 17,
#   "targets": [21, 24, 30, 33],
#   "precision": "exact",
#   "may_be_invalid": False,
#   "reasons": [],
# }]
```

GraphML/JSON exports should attach `dispatch_precision`,
`dispatch_may_be_invalid`, and `dispatch_reasons` to the dispatch node. Edges
remain ordinary jump edges. Add a schema/version note before these keys become
part of a published compatibility promise.

The legacy node/edge view remains available, but documentation must say that
absence claims are trustworthy only when the relevant coverage is complete.

## 8. Roadmap and exit gates

### Phase 0 — Freeze contracts and fixtures

- Add lattice-law tests: idempotent, commutative, associative and monotone join.
- Add fixtures for one table, two disjoint tables, conditional selection,
  copies, mutation, escape, unknown pointer, missing label, recovery, and
  unproved bounds.
- Record today's function-wide results as fallback expectations, not golden
  precision.

Exit: the soundness invariant and every expected precision/uncertainty result
are executable tests before the resolver changes CFG edges.

### Phase 1 — First-class result and sparse metadata

- Introduce `IndirectTargets`, `TargetPrecision`, and stable uncertainty enums.
- Carry the result through `Flow`, `CfgBuilder`, and `Cfg`'s sparse side table.
- Initially populate every dispatch with the current function-wide set marked
  `Conservative`; preserve graph shape.
- Export the data through Rust, Python, JSON/GraphML, and stubs.

Progress (2026-09-15): `TargetPrecision`, stable uncertainty reasons and sparse
`IndirectDispatchInfo` now cross `Flow`, `CfgBuilder`, `Cfg`, coalescing, the
general Python CFG dictionary and generic graph export. Structural projections
created with `Cfg::from_parts` carry no invented resolution metadata. The
fixed-width Python feature schema now also includes the first-class node kind.
Control-target coverage integration is implemented; schema-version policy
remains open. Phase 2 now resolves direct and conditional label values,
immutable scalar copy chains, supported scalar reassignment may-sets,
structurally owned label-address arrays, and constant in-bounds slots. Unknown
initializer elements, unsupported uses or writes, escape, or a larger unknown
expression retain the function-wide fallback.

Exit: no precision improvement yet, but no consumer needs to infer uncertainty;
ordinary CFG node size and non-computed-goto output remain unchanged.

### Phase 2 — Local finite-set resolver

- Resolve label constants, local copies, compatible casts, conditionals,
  commas, and aggregate initializer slots.
- Support constant-index and unknown-index reads of immutable local/static
  tables.
- Reuse/extract existing points-to constraint machinery and deterministic
  worklist conventions.
- Fall back on mutation, escape, recovery, or unsupported operations.

Exit: the common dispatch-table fixture and two-disjoint-table fixture are
`Exact`; every unsupported fixture retains all valid targets and a reason.

### Phase 3 — Shared semantic integration

- Replace syntax-spelling keys with declaration, value, place, and aggregate
  slot IDs from the semantic foundation work.
- Unify escape/unknown-memory facts with `dataflow/memory.rs`.
- Add `ControlTargets` coverage and propagate it into summaries, slicing, and
  negative query results.
- Remove the old independent `Reach::address_taken` responsibility except as
  the universe/fallback collector.

Exit: there is one owner for binding, storage, escape, and uncertainty facts;
the CFG and dataflow layers cannot disagree silently about target completeness.

### Phase 4 — Flow and range refinement

- Refine reassignment and table mutation using the conservative bootstrap CFG.
- Prove constant/range-constrained indices where the current integer model is
  complete; otherwise retain the broader result.
- Model supported label-difference tables and document target/layout
  assumptions.
- Enforce budgets whose exhaustion widens rather than truncates.

Exit: refinement only removes edges when a testable proof obligation succeeds;
turning off refinement reproduces Phase 2's conservative superset.

### Phase 5 — Validation and release

- Differentially compare supported fixtures with compiler-emitted LLVM
  `indirectbr` destination lists. Treat compilers as development oracles, not
  runtime dependencies.
- Add runtime traces for positive witnesses; do not use finite executions to
  prove a target impossible.
- Add mutation/property tests asserting that uncertainty never narrows output.
- Benchmark candidate count per dispatch, exact/conservative rate, solve time,
  peak memory, and zero overhead on functions without computed gotos.
- Update reference docs and release notes only after Rust, editable-extension,
  Python, export, and documentation gates pass on one attributable snapshot.

Exit: published APIs describe the implemented supported fragment and fallback
semantics; no claim of complete GNU C alias analysis or Joern parity is made.

## 9. Acceptance matrix

| Case | Required targets | Precision | Qualification |
| --- | --- | --- | --- |
| `goto *&&a` | `a` | Exact | none |
| `p = &&a; goto *p` | `a` | Exact | none |
| `p = c ? &&a : &&b` | `a`, `b` | Exact | none |
| immutable table, constant index | selected slot | Exact | invalid only if bound unproved |
| immutable table, proven in-range unknown index | all table slots | Exact | none |
| two independent tables | selected table only | Exact | none |
| table may be mutated through alias | all function address-taken labels | Conservative | unknown memory |
| label pointer escapes to unknown call | all function address-taken labels | Conservative | escape/unknown memory |
| unsupported pointer arithmetic | all function address-taken labels | Conservative | pointer arithmetic |
| recovered dispatch expression | all function address-taken labels | Conservative | recovered syntax |
| no address-taken labels | no successors, diagnosed divergence | Conservative | unresolved/invalid |

## 10. Non-goals

- recovering machine-code jump tables from binaries;
- whole-program or fully path-sensitive alias analysis;
- treating a transfer to another function's label as a valid CFG edge;
- proving general absence of undefined behavior;
- changing parity projections merely to improve a benchmark score;
- making compiler libraries runtime dependencies.

## 11. Recommended next increment

Implement Phase 0 and Phase 1 together as one reviewable change. They establish
the public truth model while deliberately preserving today's edge sets. Then
implement Phase 2 behind focused fixtures. This order prevents a new precision
algorithm from shipping before Cindergraph can say when that algorithm gave up.
