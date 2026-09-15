# Cindergraph design and product roadmap

Date: 2026-09-15. Status: active roadmap. Listed work is not an implementation
claim.

## 1. Direction

Cindergraph should become a dependable, reusable semantic-analysis library for
ordinary and decompiler-shaped C. Its distinguishing qualities should be:

- tolerant parsing without silent semantic invention;
- deterministic CFG, value, memory, call and slice results;
- useful partial answers with explicit uncertainty;
- one implementation shared by Rust, Python and Glaurung;
- small typed APIs, not a reimplementation of Joern's database and query stack.

The next stage is neither more isolated corpus fixes nor broad CI expansion. It
is completion of the semantic spine already under construction: resolved
identities and types feed one evaluation model, which feeds CFG, memory,
summaries and queries. Release engineering certifies milestones of that work;
it does not substitute for functionality.

## 2. Current position

Working foundations include the standalone Rust crate and Maturin/PyO3 package,
tolerant syntax, general CFGs, metrics, graph export, reaching definitions,
local points-to reasoning, summaries, slicing, typed analysis identities,
structured semantic issues, an emerging operation/type layer, and a first-class
indirect-dispatch node. The narrow DecBench-adjacent Joern comparison has also
been completely adjudicated.

The architecture remains mid-migration. Some expression, memory and
interprocedural semantics still come from token/span heuristics. Coverage is not
yet uniformly derived from a positive ledger. Glaurung still compiles its
embedded copy, so fixes can drift. The current pending tree must become an
attributable snapshot before it can support a release claim.

The detailed diagnosis is in the
[semantic foundation plan](architecture/semantic-foundation-plan.md). This page
is the portfolio-level sequence above that implementation plan.

## 3. Governing contracts

### Conservative answers, explicit limits

Dependency and control results are conservative may-results. A found path is
permitted by the abstraction, not proven feasible. Absence is reportable only
when all relevant coverage is complete. Unsupported syntax, unknown memory,
external effects and exhausted budgets widen an answer or make it unknown.

This matches LLVM's official [Alias Analysis](https://llvm.org/docs/AliasAnalysis.html)
discipline: uncertain accesses require `MayAlias` or conservative Mod/Ref
answers rather than an invented exclusion.

### One semantic spine

```text
source + options
  -> tolerant syntax and diagnostics
  -> declarations, scopes and structural types
  -> values, places, operations and evaluation regions
  -> executable CFG and memory constraints
  -> intraprocedural facts and call summaries
  -> coverage-bearing queries, exports and adapters
```

Source spans are coordinates, not identities or execution order. Every migrated
language rule has one owner. A parser scan, CFG heuristic and dataflow heuristic
must not independently decide the same evaluatedness, binding or alias fact.
Clang's [Internals Manual](https://clang.llvm.org/docs/InternalsManual.html)
provides useful precedent for separating source syntax, declaration identity
and canonical/qualified types; Cindergraph should retain compact Rust-native
structures rather than copy Clang's object model.

### Stable core, ergonomic facades

The Rust crate is the semantic source of truth. Python exposes owned results and
stable serialized names, not another analyser. Compatibility layers translate
at the edge and explicitly reject unsupported behavior. Before stability, audit
the API against the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/):
private representation, common traits, validated inputs, examples and documented
failure behavior.

### Performance is part of correctness

Fixed-point computations require monotone lattices, termination, explicit
budgets and visible exhaustion. Optimized solvers remain checked against small
reference implementations. LLVM's [MemorySSA](https://llvm.org/docs/MemorySSA.html)
is instructive: explicit memory definitions, uses and joins avoid repeated
dependence scans while retaining documented precision tradeoffs. Cindergraph
should adopt those principles only after typed operations and places exist.

Runtime dependencies follow the measured
[dependency policy](architecture/dependency-policy.md): solvers and compiler
frameworks are development oracles unless a separately reviewed optional
interface requires them. JSON export no longer pulls Serde into the normal core
or wheel dependency graph.

## 4. Priority order

| Priority | Workstream | Required outcome |
| --- | --- | --- |
| 0 | Attributable baseline | Reviewable snapshot and honest defect/coverage ledger |
| 1 | Semantic operation spine | CFG and dataflow consume the same evaluation facts |
| 2 | Precise control targets | Per-dispatch computed-goto targets with sound fallback |
| 3 | Typed memory | Places, aliases, effects and memory versions share one model |
| 4 | Interprocedural/query truth | Call-site identity, caller-visible effects and qualified negative answers |
| 5 | Public API and documentation | Coherent sessions, typed results and practical guides |
| 6 | Verified `0.1` publication | Crate, wheels and sdist tied to one reviewed revision |
| 7 | Glaurung migration | No duplicate source-analysis implementation |
| 8 | Deliberate expansion | Incremental analysis, broader C and higher-level graphs as measured needs |

Priorities express dependencies, not total serialization. Documentation and
focused artifact checks accompany API changes; platform release matrices run at
candidate boundaries. Until `0.1`, target roughly 70% semantic functionality,
20% semantic validation and 10% documentation/distribution.

## 5. Milestone A: attributable baseline

1. Partition pending work into reviewable semantic, CFG, dataflow, API,
   documentation and distribution commits.
2. Rebuild the native extension after the last Rust change and run core gates
   on that exact snapshot.
3. Record unsupported constructs and red semantic obligations; do not turn them
   into skips or blanket incompleteness.
4. Freeze the analysis options, coverage vocabulary and graph identity rules
   needed by later milestones.
5. Measure small, deep, wide and alias-heavy baseline workloads.

Exit: a clean attributable revision builds both surfaces, focused semantic
suites pass, and each known defect belongs to a later milestone. Remote CI is
not needed to establish this local architectural baseline.

## 6. Milestone B: common evaluation lowering

This is the highest-value functionality work and closes P4/P5 of the foundation
plan:

- parse expression children still stored as opaque token runs;
- lower reads, constants, operators, assignment, increments, calls, comma,
  conditionals and short-circuit forms to explicit values and operations;
- represent guarded regions and phi-like value joins;
- distinguish produced values, discarded values and effects;
- place operations in the executable CFG rather than by span containment;
- route VLA bounds, `sizeof`, `typeof`, initializers and returns through the
  same pipeline;
- give supported builtins explicit rules and unsupported ones unknown ops;
- delete each corresponding scanner from `dataflow/events.rs` after migration.

Progress (2026-09-15): local array bounds are now parsed through the common
expression grammar rather than retained as opaque suffix tokens. Conditional
and short-circuit bounds therefore produce real CFG arms, evaluation guards
resolve to concrete edge identities, and the dataflow adapter suppresses
duplicate legacy emission by source occurrence. This closes the parser/CFG
prerequisite for guarded bound effects. The next slice now materializes the
language-defined bypass constant for `&&` and `||`, retains call operations in
guarded arms with conservative effect qualification, and places guarded scalar
writes on the taken CFG arm. A later use consequently sees both the incoming
definition on the bypass edge and the guarded write on the taken edge.
Dead-region filtering also prevents a call on a literal short-circuit bypass
from adding false uncertainty. Assignment expressions inside this bound slice
now lower computed right sides and explicitly distinguish the old target from
the assigned value. Evaluation identity is no longer synonymous with a VLA
bound slot: supported pure scalar initializers and returns now end in explicit
`FinishExpression` consumers with distinct `EvaluationId`s, purposes, CFG
owners, and topological value inputs.
Dataflow now replaces legacy reads at the exact modeled occurrences with
operation-owned reads and uses `FinishExpression` to place initialized
declarations and their visibility point. Scalar assignment, compound
assignment, and increment writes in those roots now likewise replace legacy
promotion and retain operation-owned CFG placement and completion order,
including guarded conditional arms. Event emission distinguishes direct
inputs from transitive value provenance, so a postfix value consumed by a join
does not evaluate its source twice. Unsupported roots remain wholly on the
legacy path. A supported ordinary root may contain calls; a call/write pair is
admitted only when one depends on the other's produced value. Its computed
argument reads use operation
placement, and a call is marked directly returned only when its output
`ValueId` is the input of the return's `FinishExpression`. Existing intrinsic,
defined-callee, and external-policy summaries still own effects, so ordinary
calls do not inherit the VLA adapter's deliberate type/effect uncertainty.
For these migrated calls, `CallScalar` also owns the callee, ordered argument
extents, direct argument bindings, and direct-return identity used to construct
`CallRecord`; the syntax collector remains only for roots the operation plan
declined. Dependencies and guards now produce explicit `SequencedBefore`
relations; sibling call executions are `IndeterminatelySequenced`, while calls
in opposite conditional arms are `MutuallyExclusive`. This admits nested,
sibling, and alternative calls without treating vector or source order as C
execution order. A name resolved to a local or parameter object is not lowered
as a direct call; function-pointer calls retain the legacy indirect-call
policy. This now covers a call feeding an assignment, an assignment feeding a
call argument, and call/write effects in opposite conditional arms. Call/write
pairs without a dependency path or mutual-exclusion proof remain on the legacy
event path and are explicitly qualified. Recursive scalar names
are now explicit `ReadScalar` producers rather than producerless leaves, so a
later compute operation cannot falsely order sibling accesses. Same-object
read/write and write/write pairs are accepted only with a dependency,
short-circuit sequence, or mutual-exclusion proof. Rejected roots retain legacy
events but add a localized `unsequenced_access` issue, preventing effect-complete
negative claims. Reusable phi-like joins, parameter
declarator interiors, and removal of the remaining token scanners are open.
Ordinary and bound comma expressions now share a `SequenceScalar` operation:
the discarded left value is an effect predecessor, while only the right value
is a provenance input and result. This preserves calls and writes on the left
without allowing them to flow to an initializer or return. The former
bound-specific scalar lowering implementation has been deleted; bounds and
ordinary roots now use the same recursive operator lowering. Expression
statements are ordinary roots too, with an explicit `Discard` purpose: their
reads, calls, writes, comma sequencing, and unsequenced-access qualifications
now use the same operations while the completed value has no language-level
destination. Root token extraction uses binary-bounded slices of the ordered
token table, effect-free plans skip order construction, and CFG owner lookup
reuses a stable width/index ordering to contain the added work.
`if`, `while`, `do`/`while`, `for`, and `switch` conditions are now ordinary
roots with an explicit `Control` purpose and construct kind. The `for` path
selects the grammar-owned `ForCond` child specifically, so its initializer and
step cannot be mistaken for the branch value. Supported condition reads,
writes, calls, comma sequencing, and unsequenced-access qualification therefore
replace the corresponding legacy events and retain the existing branch CFG
owner. This is a consumer cutover, not yet CFG construction from operations:
the structural CFG is still built first and the evaluation plan is placed onto
it. Expression-form `for` initialization and step clauses now also have
distinct `ForClause` purposes and use the common operation path. C99
declaration-form initialization continues through the declaration initializer
root, so the two grammar forms do not acquire duplicate owners.
GNU computed-`goto` operands now have an `IndirectDispatch` purpose as another
common-operation client. The parser's synthetic leading unary `*` is stripped
as statement grammar rather than misreported as a memory dereference; the
actual operand owns supported reads, calls, writes, sequencing, and localized
unsequenced-access qualification. Direct `goto label` has no evaluated value
and correctly receives no expression root. Target-set resolution still owns
the separate control-successor proof described in Milestone C.

Exit: CFG, uses/definitions, effects and captured type values consume the same
operation records; conditional effects occur only in their regions; value
inputs are topological; migrated constructs have no duplicate legacy emission;
well-defined fixtures agree with GCC/Clang; unsupported forms retain localized
reasons and cannot certify absence.

## 7. Milestone C: precise indirect control

Use computed `goto` as the first end-to-end client of the shared value/place
model. Implement the
[precise indirect-dispatch design](architecture/precise-indirect-dispatch.md):

- carry exact/conservative target metadata through `Flow`, `Cfg` and exports;
- solve label constants, copies, conditionals and immutable table slots;
- resolve each dispatch independently;
- retain all function-wide address-taken labels as the sound fallback;
- record escape, mutation, unknown memory, pointer arithmetic, bounds, recovery
  and budget reasons;
- propagate incomplete control-target coverage to queries.

Progress (2026-09-15): the public truth-bearing shell is implemented. Rust,
Python and graph exports expose sparse exact/conservative metadata. Direct
label values and immutable, unescaped label-address arrays now resolve per
dispatch; uncertain expressions and mutated/escaped tables retain the previous
function-wide superset. Coalescing preserves the dispatch boundary, and the
numeric feature schema counts the new node kind. Control-target uncertainty now
propagates from CFG metadata into the structured data-flow coverage ledger and
summary completeness. Direct conditional label values and chains of immutable,
unescaped scalar pointer initializers now resolve exactly as well. The
reachability prepass consumes those same per-dispatch target sets, permitting a
constant in-bounds table index to retain only its selected label. Supported
scalar reassignments now form a narrower flow-insensitive may-set, explicitly
marked conservative; unsupported assignments still fall back function-wide.
Point-sensitive strong updates, nontrivial range proofs and shared memory
constraints remain open.

Exit: disjoint tables produce disjoint exact successors; any resolver failure
reproduces the prior conservative superset with a reason. No valid target is
silently removed.

## 8. Milestone D: typed memory and memory versions

Replace expression rescanning with constraints from typed operations and places:

- stable identities for locals, parameters, globals, fields, elements,
  dereferences and unknown memory;
- address, load, store, copy, escape, call-clobber, volatile and atomic ops;
- flow-insensitive may-points-to as the safe baseline;
- a separate must-initialization analysis;
- weak updates by default; strong updates only with point-specific uniqueness
  and lifetime proof;
- abstract caller-visible parameter-pointee and global regions;
- memory def/use/join versions where they improve provenance and query cost;
- one unknown-memory mechanism shared with dispatch and summaries.

Full scalar SSA is not a prerequisite. Introduce memory versions only when the
typed operation model makes them correct and measurements justify them.

Progress (2026-09-15): scalar object access now has a first stable place
identity. `ReadScalar`, `WriteScalar`, and direct declaration operands carry a
`PlaceId` derived from the resolved `SymbolId`; source spans remain coordinates
for diagnostics and compatibility, not the key used to join operations to
dataflow bindings. Repeated accesses to one object reuse its place while a
shadowing declaration receives a distinct identity. Address formation for a
resolved scalar is now an explicit `AddressOf` operation over that same place,
not a generic unary computation or a read of the object's stored value. The
dataflow adapter consumes that operation as one address/escape event and
suppresses syntax-based rediscovery for the owned evaluation root. This
deliberately covers only resolved scalar declaration places and their direct
addresses. A scalar dereference is now an explicit `LoadScalar` consuming its
pointer value, rather than an opaque unary operator; ordinary evaluated roots
therefore retain the load in their topological value graph. Operation assembly
preserves that lowering order rather than re-sorting nested producers by source
span, so an outer load cannot precede the conditional, comma, or arithmetic
value that produces its address. The load hands its address `ValueId` to the
same pointer-source walker used by assignments: conditional addresses union
their may-targets, comma addresses use only their result operand, and arithmetic
retains operand targets while adding explicit uncertainty. The points-to pass
projects those pointee uses once and skips syntax rediscovery of the owned load,
including transparent parentheses. VLA-bound loads remain on the fail-closed
compatibility path. Fields, elements,
globals, unknown memory, explicit
stores, and memory versions remain required rather than being encoded as
scalar places. Plain assignment through a pointer now has an explicit
`StoreScalar` operation consuming both the address and assigned value. Because
an unresolved indirect store may alias any scalar place, the evaluation gate
accepts it beside other reads/writes only when dependency, language sequencing,
or mutually exclusive guards prove the pair safe. Transparent dereference of a
direct object canonicalizes to the same direct-place path for plain assignment,
compound assignment, and prefix/postfix increment. These forms therefore
neither invent an escape nor lose strong-update precision. A direct-pointer
`StoreScalar` now carries
the written-place span and supplies its exact pointer occurrence to target
projection; the compatibility AST scan skips that owned store, and the
operation produces each projected memory definition once. Store addresses now
use the same operation-derived pointer-source constraints as loads: conditional
addresses weakly update the union of possible objects, comma addresses use only
the resulting operand, and arithmetic retains known may-targets while making
memory coverage explicitly incomplete. The operation also owns its complete
LHS span, so the compatibility load/store scans do not rediscover it through
parentheses. The same store constraint also carries pointer sources for its
assigned `ValueId`. A local second-order write such as `*pp = &object` therefore
updates each possible pointer target without rescanning the RHS; conditional
values union exact alternatives, while arithmetic retains its observed targets
and propagates `Unknown`. Updates remain deliberately weak in this
flow-insensitive may analysis. Full operation-derived pointer-value coverage
for evaluator-rejected roots remains open. Assigned-value provenance is walked
only when the resolved store targets include pointer objects, avoiding a second
graph traversal for the common non-pointer store. Pointer-valued scalar
initializers and direct assignments now emit first-class `Address`, `Copy`,
`Load`, or `Unknown` constraints by walking `ValueId` producer edges. Resolved
`PlaceId` destinations and sources map to dataflow bindings once; the points-to
pass skips its RHS source-range inference for those owned writes. Selects retain
all alternative targets, while arithmetic or unary transformation retains its
operand targets and adds `Unknown`, preserving the may model without claiming
completeness. Unsupported roots still require compatibility discovery.

Loads and stores now also attach an interned `ProjectedPlaceId` to their
`EvaluationOp`. The corresponding projected-place table records a dereference
in terms of the exact address `ValueId`; direct scalar operations attach their
existing scalar place through the same `SemanticPlace` field. Memory projection
therefore asks the operation for its place and follows the place's address
value, rather than treating an expression span as the identity of the accessed
storage. Complete field/element semantics, global places, and unknown memory
remain before this becomes the sole memory representation. Pointer-member
reads (`p->field`) and indexed reads (`base[index]`) now lower through the same
`LoadScalar` operation and create field or element projected places. Pointer
bases and indices retain their evaluated `ValueId` dependencies, including
nested side effects. These
places still qualify memory completeness because field layout and target-set
projection are not yet resolved. Evaluation lowering now consumes structural
types when choosing an element base: a local array remains its object
`PlaceId`, while an array parameter uses its language-adjusted pointer value.
This removes a fictitious whole-array scalar read without confusing written
array-parameter syntax with array-object storage. A projected base now
distinguishes an evaluated `ValueId` from a direct object `PlaceId`, so simple
`object.field` reads and plain writes use the aggregate object's place without
inventing a scalar read of the complete aggregate. Projected bases can also
name another `ProjectedPlaceId`: nested direct and mixed chains such as
`object.inner.field` and `p->inner.field` are interned inside-out without
loading the aggregate intermediate. Plain assignments to `p->field` and
`base[index]` now lower to `StoreScalar` over the same projected-place kinds.
The store explicitly consumes the base, optional index, and assigned value in
language order. Dataflow retains those inputs and its alias-order checks but
continues to report unknown memory effects rather than inventing field layout
or element targets. Compound assignments and prefix/postfix increments over
dereferences, fields, and elements now reuse the same projected place: they
evaluate the base/index once, explicitly load the prior value, compute the
replacement, and store once. `StoreScalar` records both prior and assigned
values plus pre-write/post-write result semantics, so postfix yields the old
load while prefix and compound forms yield the replacement. Known direct
pointer targets continue to project exactly one use and one write; unresolved
field/element updates remain visibly incomplete. Exact `*&object` forms resolve
to the object's `PlaceId`, emit direct reads/writes with correct pre-write or
post-write results, and never become indirect memory effects. Field-member
array typing now has a structural path rather than another expression scan.
Struct/union bodies expose grammar-owned `MemberDecl` nodes whose declarators
reuse the ordinary pointer/array/function grammar. Named, inline, and
self-referential records receive stable `RecordId`s; member types point into the
same `TypeId` graph as parameters and locals. Named record bindings carry their
enclosing block/record extent, so an inner tag stops shadowing at the end of its
lexical scope. Consequently `object.array[i]`
and `pointer->array[i]` project an element from the field place without a
fictitious scalar load of the array-valued field. A missing record closer also
retains later function definitions through the parser's existing recovery task
machine. Complete tag/member namespace rules, anonymous records inside opaque parameter
groups, bit-field type metadata, byte layout, anonymous/unknown union overlap,
and complete field/element points-to projection remain open; current member knowledge improves operation
identity but does not claim concrete offsets or complete memory effects.
Structural type propagation now also crosses each index operation. Local and
member multidimensional arrays therefore build nested `Element` places for
intermediate rows, including adjusted pointer-to-array parameters, and emit a
`LoadScalar` only for the final scalar element. This closes the remaining
aggregate-decay ambiguity in place construction; the memory adapter still
qualifies these regions until it has an explicit region lattice.
The first public region boundary is now present. `DataFlow::memory_regions`
interns binding roots, named-field children, and conservative all-element
summaries, while `memory_accesses` attaches operation-owned reads/writes and
distinguishes exact direct-object associations from may-alias pointer or
element associations. Direct `object.field` and a resolved `pointer->field`
therefore converge on one structural identity without pretending that a field
is a scalar binding. Element subscripts deliberately collapse below their
array region until index equivalence and disjointness are proved. A separate
region reaching-definition solver now emits `memory_definitions`,
`memory_uses`, and `memory_edges`: exact writes strongly kill earlier writes of
the same region, may-alias writes join weakly, branches union alternatives, and
loop back-edges converge by worklist. The region table now publishes explicit
ancestor/descendant containment and known union-member overlap. The solver
uses the latter bidirectionally: a write to one union member reaches a read of
another, and an exact later member write kills earlier writes to sibling
members; ordinary struct siblings remain disjoint. Containment is recorded but
does not yet justify destructive root/subregion transfer because partial
aggregate writes require fragmentation. Field/element accesses continue to
emit `UnknownMemoryEffect` because aggregate/root transfer, unknown pointees,
calls, escaped storage, byte layout, and general aggregate initializers are not
yet complete. By-value aggregate parameters now seed each accessed region with
an `incoming_parameter` definition at function entry. A direct returned field
therefore produces positive parameter-to-return provenance; an exact overwrite
kills it, while a conditional overwrite preserves the entry alternative.
Evaluation-owned direct aggregate/array bases are also removed from the legacy
scalar-use stream: naming `s` in `s.field` identifies storage and does not read
the complete value of `s`. The next slice is escaped regions plus conservative
root/subregion transfer, after which supported direct fields can stop failing
closed.
Calls now conservatively participate in this local memory model. The evaluator
walks each argument's `ValueId` producers; a known local address or pointer
target adds a weak `call_clobber` definition to every accessed region rooted in
that object, and scalar pointees receive a weak compatibility memory write.
The call span remains an `UnknownMemoryEffect` until a callee effect summary
can replace the clobber. Non-pointer value arguments do not affect memory
coverage. Local array arguments use an explicit `DecayArray` operation, so
`touch(array)` resolves the array root without fabricating either a scalar load
or an explicit address-taking event.
Incoming pointer values now own abstract `parameter_pointee` region roots.
Direct and copied formal pointers retain the same caller-owned identity;
different formal roots explicitly may alias, so a write through one can reach
a read through another without allowing a strong cross-parameter kill.
Resolved field and all-element accesses beneath those roots no longer inherit
blanket token-scanner uncertainty. Function summaries publish direct typed
read/write effects as paths relative to the formal and qualify them separately
with `memory_effects_complete`. Pointer address operands are excluded from
scalar-return provenance, so `return p->field` does not falsely claim that the
pointer value itself is the returned scalar. Complete effects now compose
through the summary fixed point and instantiate as typed reads or weak writes
on concrete caller and inherited formal-pointee regions. Region overlap is
rebuilt after instantiation, so containment, union, and cross-formal aliasing
remain visible to the memory solver. Unknown, incomplete, indirect, and
ambiguous callees retain generic weak clobbers and explicit uncertainty.
Region edges already feed the ordinary DDG, PDG and backward-slice consumers;
the new semantics are not isolated in a diagnostic side table. DDG exports
retain separate memory definition/use nodes and region paths, while PDGs lift
them onto their owning CFG nodes alongside scalar dependences.

Exit: each supported load/store has an explicit place and effect; the may model
includes all observed concrete writes; unknown pointees qualify negative
results; optimized solving agrees with a bounded reference solver.

## 9. Milestone E: interprocedural semantics and query truth

Rebuild summaries on function, call-site, value and memory-region identity:

- ordered actual/formal mapping and scalar return dependencies;
- parameter-pointee and global effects, escape and termination facts;
- direct and indirect call targets with explicit uncertainty;
- SCC/worklist convergence for recursion;
- configurable external-callee policies;
- issue propagation to precisely the caller facts affected.

Promote queries to typed claims: `FoundMayPath` with provenance, `NoMayPath`
only with its coverage proof, and `Unknown` with reasons and partial discoveries.
Slices carry source revision and graph identity; ambiguous names are errors.

Exit: caller-visible stores cross calls, recursion and duplicate names retain
identity, incomplete callees cannot yield unqualified negative results, and
found paths are explainable through calls and memory.

## 10. Milestone F: public API and real documentation

Stabilize an analysis-session API rather than unrelated convenience functions:

```rust
let unit = AnalysisUnit::parse(source, options)?;
let function = unit.functions().by_id(id)?;
let cfg = function.cfg();
let flow = function.dataflow();
let answer = function.query(query);
```

Python mirrors these concepts idiomatically and reuses parsing/resolution.
Public results consistently expose diagnostics, assumptions, issues, revision
and graph kind.

Progress (2026-09-15): Rust `AnalysisUnit` now owns one parse, executable CFGs,
resolution/type state, and lazily cached evaluation plans, dataflow and
summaries. Syntax and CFG-only consumers therefore do not pay to lower
operations they never request.
Typed function views bind `FunctionId`, CFG and flow to that snapshot. Existing
one-shot functions remain compatible. Python `AnalysisSession` now owns the
same native unit across CFG, dataflow and summary requests and returns fresh
Python views of cached results. Slicing and all five export representations now
consume that same unit rather than reparsing or rebuilding analysis products.
Reachability now also has a typed `ReachabilityResult`: found may-paths carry a
formal-parameter path, complete negatives carry a coverage marker, and unknowns
carry stable reasons plus explored states. The older three-valued string API is
a compatibility projection. Summary storage and reachability now retain every
`FunctionId`; identity-first queries distinguish duplicate bodies while
name-based queries fail closed as ambiguous. `AnalysisOptions` and Python's
session `dialect=` now make ordinary, preprocessed, or decompiler-source
preparation explicit before identity and parsing; sessions expose the resulting
source and diagnostics, and structured products record the dialect. Rich
value/memory provenance remains open. Configurable external-callee policy is
now part of the owning Rust/Python session: unknown remains the safe default,
taint-return adds conservative return flows without claiming complete effects,
and an explicitly named pure/no-flow contract can justify negatives. Policy
models stay private rather than appearing as source functions, participate in
the summary fixed point transitively, and are recorded on structured results.
The first dependency-free Python `NativeGraph` view now exposes all five graph
families with snapshot identity, bulk topology, native adjacency, degree and
transitive traversal. NetworkX remains an explicit optional conversion rather
than an analysis engine; requirements and baseline measurements live in the
[native graph API design](architecture/native-graph-api.md). Forward and
reverse adjacency now use contiguous offset/neighbor arrays rather than
per-node tree entries and vectors, preserving parallel edges and deterministic
order while materially reducing measured construction and traversal time.
Native components, projections and bounded paths remain demand-driven follow-up
work rather than reasons to add a general graph dependency.

Required documentation includes five-minute Rust and Python paths; concepts for
CFG, may-dependence, memory, summaries and uncertainty; a generated support
matrix tied to fixtures; error and incomplete-result examples; API migration
notes; benchmark methodology; useful rustdoc; and native/stub consistency.

Exit: a new user can install a candidate, analyze a function, understand an
uncertain answer and export a graph without reading implementation code. Every
advertised example runs against a packaged artifact.

## 11. Milestone G: verified `0.1` publication

Publication is a bounded promotion of a chosen semantic/API candidate. Cargo's
[publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html)
notes that versions are effectively permanent and recommends inspecting and
dry-running the actual package. PyO3's
[distribution guide](https://pyo3.rs/main/building-and-distribution) documents
the `abi3` tradeoff: fewer wheels in exchange for the limited API.

1. Freeze one clean revision and changelog entry.
2. Run semantic, Rust, Python, typing, docs and export gates on it.
3. Inspect and test the extracted `.crate`, not just the checkout.
4. Build wheels and sdist; install each into clean environments.
5. Verify `abi3-py312` on CPython 3.12–3.14; state that free-threaded/PyPy are
   unsupported rather than implying coverage.
6. Obtain green remote artifacts for every claimed platform.
7. Recheck names, ownership, licensing and provenance immediately before upload.
8. Publish only through the documented human-approved boundary.
9. Fetch both packages anonymously, verify identity/hashes and rerun consumer
   smoke tests.
10. Record a partial release plainly if only one registry succeeds.

Exit: crates.io and PyPI artifacts are downloadable, tied to the same tagged
revision and independently tested. A configured or green workflow alone is not
publication evidence.

## 12. Milestone H: Glaurung migration

1. Pin the reviewed Cindergraph candidate in Glaurung by path or Git revision.
2. Retain only Glaurung-specific LLIR, solver, KB, binary and orchestration
   adapters.
3. Compare identical Glaurung-facing fixtures before and after substitution.
4. Remove the embedded source-analysis copy after Glaurung's gates pass.
5. Move to the verified registry version and record its identity.

See [Relationship to Glaurung](architecture/glaurung.md) for the full boundary.
This follows a reviewed candidate but need not block the first upload.

Exit: Glaurung cannot silently compile a second implementation and receives
Cindergraph fixes through normal dependency updates.

## 13. Post-`0.1` expansion

Choose additions by user value and measured evidence:

- dialect profiles, target layouts, target-aware constants and staged C23;
- optional preprocessor/source maps and user-supplied header/external summaries;
- field/element sensitivity, range facts and flow-sensitive alias refinement;
- dominance/post-dominance and nontermination-aware control dependence;
- immutable snapshots, function-level invalidation and deterministic parallelism;
- compact caches and uncertainty/performance profiles;
- optional feasibility clients consuming—not weakening—the conservative graph.

A richer program graph may eventually combine syntax, control, value, memory
and call edges, but only after their identities stabilize. Call it a Cindergraph
program graph, not “Joern compatibility,” unless a separate tested API/query
contract exists. A small typed query API should precede a general query language
or database service.

## 14. Measurement programme

Maintain four distinct scorecards:

| Scorecard | Measures |
| --- | --- |
| Semantic correctness | supported obligations, independent counterexamples, false negatives, false positives, exact/conservative/unknown rates |
| Precision/usefulness | targets per dispatch, pointees per dereference, slice size, call/name resolution, qualified negative answers |
| Performance | time and peak memory by stage plus tokens, operations, graph edges, definitions, points-to and summary facts |
| Distribution/adoption | packaged-crate docs/build, installed wheel/sdist smoke, stub drift, released downstream examples, Glaurung dependency identity |

Keep debug/release and cold/warm measurements distinct. Compare identical
populations. Joern/DecBench-adjacent shape remains one narrow regression set,
not the product scorecard; rerun it when projection rules change, but do not
optimize the semantic architecture around it.

The next external comparison is a robustness study across Cindergraph,
Tree-sitter, Clang, CDT and Joern, governed by the
[source-front-end robustness plan](benchmarks/robustness-comparison-plan.md).
It uses a fixed manifest, one result per tool/specimen even on failure,
separate raw and transformed inputs, explicit resource outcomes, and a vector
of survival, yield, locality, validity, semantic honesty, determinism and scale
metrics. This is cross-cutting validation of Milestones B--E, not a new feature
priority and not a scalar product ranking. Initial implementation follows the
semantic spine: define the tool-neutral contract, run Cindergraph plus
Tree-sitter and Clang, then add persistent Joern and pinned CDT adapters.

Progress (2026-09-15): the dependency-free tool-neutral contract is executable.
Its JSONL validator verifies source hashes and bounded paths, rejects duplicate
or overlapping specimen identities, requires replay metadata for generated and
mutated cases, computes an ordered canonical manifest hash, and requires one
explicit success or failure result per specimen from a single run/tool identity.
The first manifest now freezes 210 clean files, 26 decompiler-dialect cases,
256 deterministic damaged inputs, and 33 controlled locality cases (525
specimens total), and its generator checks the stored mutation bytes against
the existing totality suite. The controlled cases span eleven damage operators
at three severities and declare 63 byte-preserved neighbour obligations without
using any compared parser as their oracle. The
Cindergraph adapter preserves duplicate and anonymous recovered functions,
validates closed AST/CFG graphs, retains its analyzed source and native
diagnostics, and makes completeness claims only from explicit recovery and
control-target signals. Each specimen now runs in a disposable process with a
wall-time limit, an optional address-space limit, signal/timeout/memory outcome
classification, startup/analysis/serialization timing, and peak-RSS reporting.
This remains benchmark infrastructure and a local self-baseline, not
comparative evidence. A raw-input Clang adapter now retains partial ASTs on
ordinary compiler-error exits and reports CFG, RSS, and startup metrics as
unavailable rather than fabricating them. A local dirty-tree run recovered all
930 clean oracle functions in both tools and exposed twelve decompiler-dialect
cases recovered only under Cindergraph's declared decompiler policy. This is a
harness validation, not a publishable benchmark. A separately locked
Tree-sitter C 0.24.2 worker now provides the third recovery projection without
entering the product dependency tree; it recovered 20/25 named dialect cases
in the same local run. Captured Clang build contexts and CFGs, independent
damaged-code oracles, scaling series, and publishable clean-revision reruns are
the next execution boundary.
The first controlled curve localized Cindergraph's only three misses to the
single lexer policy that consumed an unterminated block comment through EOF.
The lexer now keeps that diagnostic and its incomplete-coverage consequence but
can conservatively restart at a later top-level-function-shaped line; it
rejects control statements and calls as boundaries. Cindergraph consequently
retains all 63/63 protected neighbours on the unchanged controlled population,
up from 60/63, without weakening a negative completeness claim.

## 15. Rules against whack-a-mole development

1. Reduce each defect to a violated invariant and ownership boundary.
2. Add a minimal regression plus an interaction or metamorphic test for the
   same root cause.
3. Repair the shared representation/transfer rule, not a permanent token case.
4. Remove the old path once its supported cases migrate.
5. Keep a simple reference analysis beside optimized fixed-point code.
6. Fail open for may sets and fail closed for absence/completeness claims.
7. Benchmark semantic edits locally; reserve full release reproduction for
   candidate boundaries.
8. Report source success, local artifacts, remote CI and publication separately.

## 16. Immediate execution order

1. Establish and review the attributable baseline.
2. Close guarded expression/effect lowering.
3. Make indirect dispatch the first precise control client of that model.
4. Migrate pointer/memory constraints to typed operations.
5. Rebuild interprocedural effects and coverage-bearing queries.
6. Stabilize the Rust/Python session API and practical documentation.
7. Cut and independently verify `0.1`.
8. Migrate Glaurung and delete its duplicate implementation.
9. Select post-`0.1` work from measured user needs.

This sequence addresses the architectural cause of recent edge cases while
still reaching a usable release. It avoids both endless local patching without
a model and infrastructure work without a better analyser.

## 17. Detailed documents

- [Semantic foundation](architecture/semantic-foundation-plan.md): root causes,
  internal design and P0–P8 migration details.
- [Precise indirect dispatch](architecture/precise-indirect-dispatch.md): target
  lattice, CFG metadata and implementation phases.
- [Glaurung relationship](architecture/glaurung.md): ownership and migration.
- [Support and evidence](support-and-evidence.md): current qualifications.
- [Release checklist](releasing.md): operator procedure.
- [Benchmark records](benchmarks/README.md): dated evidence, not the roadmap.
- [Robustness comparison](benchmarks/robustness-comparison-plan.md): fixed
  populations, adapter contract and execution phases for Cindergraph,
  Tree-sitter, Clang, CDT and Joern.
