# Semantic foundation: review and implementation plan

Date: 2026-09-14. Status: implementation in progress, based on inspected source,
the extraction history, today's QA records, and the subsequent semantic work.
This document records the design decision, verified increments, and remaining
work sequence. A phase's target architecture is not implemented unless its
progress paragraph and exit condition say so explicitly.

## 1. Decision and diagnosis

Cindergraph needs an explicit C semantic layer between its tolerant syntax
tree and its dependence analyses. Extend the existing parser to preserve the
required syntax, resolve declarations and types once, then lower evaluated
expressions to operations with explicit value inputs and control positions.
Make CFG, memory analysis, summaries, slicing, and confidence reporting consume
that shared representation.

The recurring problems are architectural. Many recent counterexamples involve
uncommon C constructs, but the same missing distinctions also caused ordinary
assignment, initializer, pointer-store, and call-site failures. More isolated
token recognizers in `dataflow/events.rs` will continue producing regressions
at the intersections of these rules.

There is useful implementation to retain: the token buffer and arena, explicit
stack traversal, recovery machinery, generic graph algorithms, serializers,
reaching-definition lattice, worklists, and independent test oracles. The
required change is the information and contracts passed between those parts.
Moving existing scanners to a differently named module alone would not solve it.

### The original assumption that stopped holding

Glaurung's original `docs/design/static-c-analysis/architecture.md`, section 4,
argues that declaration/expression ambiguity is mostly immaterial to CFG shape.
Its `requirements.md`, `REQ-GEN-5`, separately requires an AST that supports
lowering without reparsing. The current compact AST and token recovery strategy
do not satisfy the latter requirement for the semantics now exposed publicly.

For example, `T * x;` may have similar superficial graph shape under either
reading, but the readings have different declarations, reads, types, and
possibly observable volatile effects. `typeof(a)` also cannot be interpreted
from spelling alone: its behavior depends on the resolved type of `a`.

This is a change in the demands on the original design, not evidence that the
Rust arena or fixed-point algorithm needs wholesale replacement.

## 2. Review scope and exact current state

The review covers the extraction and committed repairs, the themes and evidence
in QA 1–91, current changes to parsing, CFGs, dataflow, Python, exports and
packaging, and the later work recorded in this conversation. The latter includes
dead-code pruning, pointer increments, builtin effects, VLA-bound recovery,
enumerators, parameter scopes, and `typeof`. Implementation review concentrated
on the shared semantic paths and algorithms; this is not an exhaustive proof of
every changed line or every accepted C construct.

### Provenance and port fidelity

- [EXTRACTION.md](../../EXTRACTION.md) pins the Glaurung origin to
  `0892552157be6bd9267007231419ff6606a2dd38`.
- Standalone extraction commit: `ec47556`. Initial reviewed baseline: `28ae864`.
- Current Cindergraph HEAD at inspection: `edf2777`, with extensive pending
  changes. Most of today's QA improvements are not represented by that commit
  alone.
- Current sibling Glaurung HEAD at inspection:
  `32b1e8bdaf19848f1c7a17aae0e76f7c80725112`.
- Direct diffs of origin versus extraction for `dataflow/events.rs`,
  `interproc.rs`, `types.rs`, and `solve.rs` showed formatting changes, not
  replacement algorithms. The inspected parser tag file was identical.
- Glaurung still carries its embedded implementation. Its LLIR lowering,
  solver-backed checks and KB integration were deliberately excluded; they
  were not functionality lost from Cindergraph's source dataflow during copying.

Thus the initial summary/typing limitations were inherited. Later Cindergraph
repairs and their regressions must be judged separately. Glaurung does not
automatically receive these fixes. See [the ownership and migration
boundary](glaurung.md).

### The current tree is not yet a green baseline

The review ran:

```sh
export TMPDIR="$PWD/target/tmp"
cargo check -p cindergraph
```

At the time of the review it failed with `E0425` at
`dataflow/events.rs:501`: the interrupted captured-VLA patch called an undefined
`typeof_value_names` function. The partial patch also added
`variably_modified_bindings` and regression assertions.

The first P0 increment subsequently completed that narrow compatibility check.
It recognizes only a direct value operand, with redundant parentheses, and
marks captured runtime-bound provenance incomplete. It does not synthesize the
missing dependence and is explicitly scheduled for removal in P3/P4. On the
same dirty snapshot, `cargo check -p cindergraph`, strict Clippy, the focused
Rust regression, an editable-extension rebuild, and both focused Python
`typeof`/VLA tests passed. This removes the compilation blocker but does not
turn the shared pending tree into an attributable green baseline.

The last completed broad prior-turn run reported 654 active Rust core tests passing
and 3,844 Python/design tests passing, with one Rust ignore, ten Python skips,
and three deselections. Those results precede the interrupted patch. They do
not establish the current source's correctness or compilability.

The installed editable native extension was still importable. Its SHA-256 was:

```text
34e9f511e55f382bff616a3c50217f52836e959c171c2fdf1e581d851d21cc94
```

It was loaded from `python/cindergraph/_native.abi3.so`. Review probes against
that artifact reproduced:

| Source | Observed result | Interpretation |
| --- | --- | --- |
| `int f(int n){int a[n];return sizeof(a);}` | Complete summary, parameter 0 reaches return | Direct bound provenance is already represented |
| `int f(int n){int a[n];typeof(a) b;return sizeof(b);}` | Complete summary, no parameter flow | Captured type loses the bound dependency; false completeness |
| `int f(int x){return x-x;}` | Complete summary with parameter flow | Current flow means structural dependence, not mathematical sensitivity |

These observations describe that extension, not a successful rebuild of the
interrupted source. The source confirms the structural deficiency: `CType`
stores spelling, pointer depth and array rank, but no captured bound identity.

### What today's work accomplished

| Work family | Evidence and result | What remains foundational |
| --- | --- | --- |
| Extraction, Rust/PyO3 separation, package naming | Extraction history and [relationship](glaurung.md) | One implementation owner and a tested Glaurung adapter |
| Call-site, return and binding identity | [Initial review](https://github.com/mjbommar/cindergraph/blob/main/review/design-review-2026-09-14.md), QA 1–5 | Explicit function, expression, argument and value IDs |
| Recovery, globals, query ambiguity | QA 8, 14, 21, 29–30, 33–35, 39, 44 | Structured coverage reasons and consistent result envelopes |
| Comma effects, initializers, unevaluated operands | QA 22–26, 36–37, 47–48 | Shared evaluation rules and explicit value/effect separation |
| Types, parameters, VLA provenance | QA 49, 56–57, 62–69 and later enum/`typeof` work | Structured declarators, lexical resolution and runtime type instances |
| Aliases, address-taking and pointer mutation | QA 5, 72–83, 86–87 | Typed places, explicit constraints, unknown memory and lifetime rules |
| Scaling | QA 9–11, 27, 41, 59–60, 69, 73, 81–86, 88–91 | Preserve indexed/worklist algorithms behind correct operation semantics |
| Robustness and public data integrity | QA 16–20, 31–35, 42–45, generated scalar/pointer populations | Separate structural invariants, semantic oracles and coverage measurement |
| Documentation and distribution | QA 6–7, 12–15, 28, 32, 38, 40, 46, 50–55, 58, 61, 71 | Finish at a stable release candidate; package success cannot certify analysis |
| Later control/effect repairs | Current `events.rs`, `memory.rs`, `interproc.rs` and their tests | A common executable graph; remove duplicated pruning and builtin interpretation |

The scaling work includes dense value-only bitsets, local event indexes,
worklists for pointer copies and callers, interval indexes, and a linear-CFG
fast path. Current source also contains an acyclic fast path. Preserve it only
after checking it against a simple reference solver; QA 91 alone does not
establish provenance or correctness for every later edit.

The repeated artifact rebuilds did find genuine distribution issues. Their
frequency was disproportionate to semantic progress once the core defects
became the active task. Retain the useful checks and run release validation
at candidate boundaries.

## 3. Root causes and replacement responsibilities

| Root cause | Current evidence | Required responsibility |
| --- | --- | --- |
| Syntax omits distinctions consumers need | `parse/tag.rs`: opaque parameter/type/builtin/aggregate regions; `parse/decl.rs`: balanced-token `typeof` | Parser preserves declarations, type operands and expression children |
| Name resolution happens repeatedly and incompletely | `TranslationUnitSymbols`, `ScopedName`, parameter scans, name-keyed summaries | One declaration/scope service with precise visibility and symbol identity |
| Display types drive semantic decisions | `CType` and `types.rs`; repeated pointer-depth tests | Structural types, qualifiers at each layer, target layout and type instances |
| Source spans stand in for evaluation | `effect_at`, `writes_containing_use`, `passes_calls`, `node_for_span` | Operations with explicit value inputs, sequence relations and block ownership |
| Control/evaluatedness has several owners | CFG `is_unevaluated` versus dataflow `unevaluated_spans`, literal/tail/switch pruning | One semantic control and evaluation plan consumed by all analyses |
| Value, address and memory are mixed | `AddressTaken` in a definition-shaped table, later projected weak writes | Separate value definitions, places, address operations and memory effects |
| Unknownness is opt-in | Completeness booleans start true; special cases turn them false | Positive coverage accounting and reason-carrying unknown operations |
| Results conflate different claims | `Yes`, slices without coverage, `same_shape`, incomplete summaries | Explicit may-dependence and negative-result contracts |
| Tests often compare shape or the previous implementation | Corpus invariants, old/new digests, many parameterized cases | Independent semantic obligations plus interaction coverage |

`events.rs` has grown to 3,497 lines in this snapshot. Its size is a symptom:
it now contains parsing, binding, type interpretation, evaluatedness, control
pruning, effect lowering and graph placement. Splitting those lines without
changing who owns each fact would preserve the defect pattern.

Two particularly consequential current mismatches:

1. CFG expression handling treats type forms and `sizeof` as unevaluated in
   `csource/cfg/expr.rs`, while dataflow separately synthesizes VLA effects.
   A declaration without an initializer is generally omitted by CFG statement
   emission, although a VLA bound can execute. Events that cannot be placed by
   containment can fall back to entry. Reaching definitions cannot repair a
   missing execution point.
2. Provenance infers expression inputs from enclosing spans and filters them
   through call/comma regions. This repeatedly confuses a discarded result
   with effects that still execute. A direct input edge would make the
   distinction structural.

## 4. Contracts to settle before implementation

### 4.1 What a dependency answer means

The primary dependence product is a conservative **may-dependence graph**:
an edge indicates that the abstraction permits influence. It does not prove
that a feasible execution follows the whole path, or that changing an input
changes the output. `x-x` is a useful test of this distinction, not a required
algebraic optimization for this project.

Define and document:

- `FoundMayPath`: a path in the modeled dependence graph, with edge provenance.
- `NoMayPath`: no path, with all relevant semantic obligations covered and the
  analyses converged under the declared language/target assumptions.
- `Unknown`: a gap prevents that absence claim.

The legacy `yes/no/unknown` spelling can remain as a documented adapter. `yes`
must not be described as an executable witness. A slice is a conservative set
relative to the same assumptions; an incomplete slice is not grounds for
deleting omitted code. Path feasibility belongs to an optional later client,
including Glaurung's existing solver integration.

Unknown operations may retain discovered paths. They must invalidate relevant
negative claims. Adding uncertainty must never change `Unknown` into `No`.

### 4.2 Language, target and recovery

Introduce explicit `AnalysisOptions`: language version, GNU extension policy,
input mode, target data layout, and resource budgets. Start verification with
C11/C17 and the GNU constructs already advertised. Record C23 forms separately;
recognizing a spelling is not full C23 support.

Keep the pure Rust runtime and tolerant handling of decompiler text. Compilers
are development oracles, not required runtime dependencies. Preprocessed input
and recovered decompiler input remain supported use cases. Raw directives or
missing header types must produce explicit uncertainty unless resolved by
supplied context; do not silently invent preprocessor configurations.

Parsing success, syntactic recovery, semantic resolution, abstract-analysis
coverage, and proof of executable behavior are different facts. No-diagnostic
parsing is not semantic validation.

Target-dependent operations need a target layout or an explicit unknown.
Implement integer promotions and constant evaluation with C widths and signedness;
never use host Rust arithmetic as an implicit C language rule. Undefined,
unspecified, and implementation-defined behavior require separate treatment.
Compiler disagreement by itself is not proof of undefined behavior.

### 4.3 Graphs and coordinates

Keep one executable CFG for semantic analyses. Preserve source-oriented syntax
views for metrics and the separate comparison/parity projection. If correcting
execution changes a public metric or graph shape, document that change and
provide a named projection when legacy shape is needed. Do not retain a second
executable graph with different language rules.

Each graph result must identify its graph kind and source revision; a node ID
from a projected CFG must not be accepted as an executable-CFG node silently.

Source ranges refer to the exact input buffer. Normalization and invalid-byte
replacement need an origin map or an explicit statement that coordinates refer
to transformed text. Source spans remain essential for diagnostics and UI, but
they cease to establish symbol identity or execution order.

## 5. Target architecture

```text
SourceUnit + options + origin map
        |
Token buffer + tolerant syntax arena
        |  parser consults lexical declaration context for C ambiguities
Resolved semantic unit: declarations, structural types, typed expressions
        |
Evaluation lowering: value inputs, effects, bound captures, executable CFG
        |
Memory constraints + reaching definitions + control dependence
        |
Call-site summaries + may-reachability + slices + coverage reasons
        |
Rust result objects / PyO3 conversion / Python facade / graph projections
```

Syntax metrics and source views may read the syntax arena directly. Parity
remains a projection. Neither supplies semantic rules to the pipeline above.

Suggested module ownership, introduced incrementally:

```text
csource/parse/       declaration/type/expression grammar and recovery
csource/semantic/   ids, scopes, declarations, types, expressions, coverage
csource/eval/       expression lowering and executable graph construction
csource/dataflow/   lattices, points-to constraints, provenance and summaries
csource/export/     output projections and source mapping
syntax/             reusable arenas, source buffers and graph algorithms
```

The final boundaries matter more than these filenames. Each migration step
must remove the old ownership of the migrated rule.

### 5.1 IDs and ownership

Use typed arena IDs for `SourceUnitId`, `DeclId`, `ScopeId`, `TypeId`, `ExprId`,
`FunctionId`, `BlockId`, `OpId`, `ValueId`, `ObjectId` and `BoundSlotId`.
Keep IDs compact, deterministic within one immutable analysis snapshot, and
checked at public boundaries. Cross-snapshot queries require source identity;
arena indices are not stable identities across edits.

An `AnalysisUnit` owns source, syntax, declaration/type arenas, diagnostics and
derived caches. Cache keys include source revision and analysis options. Rust
and Python graph, summary and slice APIs reuse the same unit, avoiding repeated
parses that can produce mismatched node populations.

Intern spellings for lookup and display. A spelling can resolve to several
declarations in different scopes; it is never the declaration's identity.
Function redeclarations share a resolved entity where valid, while distinct or
ambiguous definitions remain separately represented. Name-based convenience
queries retain explicit ambiguity errors.

### 5.2 Parsing and lexical resolution must cooperate

A post-pass over the current lossy AST is insufficient. C grammar decisions
such as `T * x;`, casts, and parenthesized calls require declaration context
while parsing.

Extend the existing explicit-stack parser with structured parameter declarations,
declarator operators, enum enumerators, type names, `typeof`, `sizeof`, `_Generic`
associations, and builtin operands. Preserve all source ranges and recovery
nodes. Flat operator chains may remain compact syntax storage; semantic lowering
must give their operands explicit associativity and result identity.

Provide a small shared lexical declaration service that the parser consults for
typedef-name classification and that semantic resolution enriches. This is
ordered symbol introduction during parsing followed by type completion, not a
dependency from parsing onto dataflow. Unknown lookup returns an ambiguity or
recovery node; it must not fabricate a declaration or call.

Represent ordinary identifiers, tags, function-wide labels, and aggregate
members in their correct namespaces. Track exact declaration points, parameter
and prototype scopes, block and loop scopes, redeclarations, and recovery-tainted
visibility. Nested function-pointer prototype declarations must not escape into
the surrounding function. Roll back speculative bindings when recovering from
an abandoned parse branch.

Enums store `Enumerator { decl, initializer, constant_result }`. Their names
must come from grammar nodes, not scanning from an `enum` token to a later brace.
Semantic declarations inside type operands must also enter their proper scopes.

### 5.3 Structural types and runtime type instances

Retain the public `CType` spelling summary as a compatibility/display view.
Introduce an internal type graph, approximately:

```text
Qualified(TypeId, qualifiers)
Builtin(kind)
Pointer(pointee)
Array(element, bound)
Function(result, parameter_types, prototype_kind, variadic)
Record(record_decl) / Enum(enum_decl)
Alias(typedef_decl, target)
Unknown(reason)

bound = Constant(value) | Runtime(bound_slot) | Incomplete | PrototypeStar
```

Qualifiers belong to individual type layers. Function parameter adjustment,
array-to-pointer conversion and lvalue conversion are explicit operations.
`int *a[4]` and `int (*p)[4]` have different type graphs; star counts and array
ranks cannot encode their ordering. Parameter types preserve both the written
type and the adjusted type used in the function body.

Do not globally intern runtime values into reusable type templates. A runtime
array type instance refers to a captured `BoundSlotId` established at a particular
declaration/evaluation point. Structurally identical declarations executed at
different times may capture different values. Use iterative traversal and
cycle guards for aliases and recursive record references.

Required behavior:

```c
int f(int n) {
    int a[n];        /* capture the current n */
    n = 1;
    typeof(a) b;     /* share the captured type size */
    return sizeof(b);
}
```

The return must depend on the original parameter through the captured bound,
not on the later assignment and not on uninitialized elements of `a` or `b`.
A typedef of a VLA captures the relevant bound at its declaration as well.
`sizeof` consumes type-size values; ordinary element loads consume object memory.

### 5.4 Typed expressions and evaluation

Each resolved expression has a type, value category, explicit operands and
resolution status. Lowering produces a value and an effect plan separately.

Use an evaluation context containing at least:

- phase: runtime, constant-expression checking, type formation or unevaluated;
- execution condition: unconditional, branch-conditioned, optionally evaluated
  by the language, or unknown;
- value disposition: consumed or discarded;
- sequencing relation to sibling operations;
- semantic issues that prevent complete lowering.

These are separate dimensions. A single `evaluated: bool` cannot express a
discarded comma operand that still writes memory, or type formation inside a
context that does not evaluate an ordinary expression value.

Centralize the rules for comma, assignments, increments, short-circuit operators,
conditional expressions, function arguments, initializers, `sizeof`, `_Alignof`,
GNU `typeof`, `_Generic`, statement expressions and supported builtins. Resolve
`_Generic` selection from the controlling type and applicable conversions before
lowering the selected value expression. Unknown selection must retain uncertainty.

Unspecified argument order is not left-to-right source order. Represent an
unordered evaluation group and conservatively combine permitted states. Detect
unsequenced conflicting accesses where supported; otherwise report the gap.
If exploring orders exceeds budget, widen to unknown rather than selecting one.

Builtin descriptions use resolved callee identity, arity, operand evaluation
rules, result dependence and memory effects. A matching spelling alone must not
override a source declaration or a function-pointer binding. Unmodeled inline
assembly and cleanup effects become explicit unknown effect operations.

### 5.5 Operations and CFG construction

Lower into operations such as:

```text
Constant, Load(Place), Store(Place, ValueId), AddressOf(Place)
Unary, Binary, Convert, Select/Merge
CaptureBound(ExprId), SizeOf(TypeInstance)
Call(Callee, arguments, result, effects)
Branch, Jump, Return, UnknownEffect
```

Every operation has an owner expression/declaration, block, diagnostic span,
and explicit input values. The executable CFG is built from this same lowering.
There are real blocks for evaluated VLA declarations even without an initializer.
Every read/write/call is attached during emission; unmatched events cannot fall
back to entry. Parameter entry definitions are explicit exceptions by design.

Gotos, labels and switch dispatch are resolved before pruning unreachable
operations. Prune on the finalized executable graph, preserving jumps into
nested constructs. Model non-returning loops explicitly. Specify the
post-dominance convention for nontermination and synthetic exits so it cannot
invent ordinary exits or lose relevant control dependence.

Within blocks, operations carry semantic sequence order. `Definition.effect_at`
may survive in a legacy export, but byte offset is no longer the solver's clock.
Provenance follows operation input edges and call results; remove span-based
`passes_calls` and `writes_containing_use` from semantic decisions.

### 5.6 Places, points-to analysis and memory

Represent `Place` as an object plus typed projections: field, array element,
dereference and supported offset. Separate pointer values, object addresses,
object contents and VLA size metadata.

The first place-identity increment is implemented for resolved scalar objects.
Evaluation operations and direct declaration operands carry a dense `PlaceId`
derived from their already-resolved `SymbolId`. Dataflow consumes that identity
directly when mapping an operation to its binding; declaration spans are
retained only as source coordinates and compatibility metadata. A shadowed
object therefore cannot alias its same-spelled outer object through span or
name matching, while accesses in separate evaluation roots reuse the same
place. This is not yet the full `Place` algebra below: projections,
dereferences, globals, unknown memory, and explicit address/load/store effects
remain open.

Direct address formation of a resolved scalar place is now represented by an
explicit `AddressOf` operation. It produces an address value without consuming
the object's stored value, and the dataflow adapter projects it to the existing
address/escape fact exactly once. This removes one syntax-rescanning owner while
leaving projected addresses and dereference loads/stores on the conservative
legacy path until their place kinds exist.

Scalar dereference now lowers to an explicit `LoadScalar` whose address input
is the produced pointer value. Operation assembly retains the lowering order,
so nested address producers are materialized before their load rather than
being reordered by overlapping source spans. Loads now feed their address
`ValueId` through the common pointer-source walker: conditionals union branch
targets, comma expressions select only their result operand, and arithmetic
keeps operand may-targets while adding `Unknown`. The points-to pass projects
the resulting pointee uses and excludes the operation-owned access, including
transparent parenthesized spans, from syntax discovery. Dereferences in VLA
bounds stay on the incomplete compatibility path rather than turning
syntactic recognition into a false completeness result.

Plain indirect assignment now lowers to `StoreScalar(address, assigned)`. Its
two value dependencies precede the store, and the unsequenced-access check
treats its unresolved target as potentially aliasing every scalar read or write
unless ordering or mutual exclusion is explicit. The transparent `*&place`
case canonicalizes to the same direct-place path for plain assignment, compound
assignment, and prefix/postfix increment. These forms preserve direct-object
identity, read the prior value when required, and avoid a fictitious address
escape or memory effect. `StoreScalar` also carries
the exact written-place span. When its address is a direct pointer read, the
operation supplies that occurrence to target selection and excludes the store
from compatibility syntax discovery, producing one projected memory write per
possible pointee. Computed indirect writes now share the load-side
pointer-source walker. Exact
conditional and comma addresses project their complete may-target set, while
arithmetic retains observed targets and an explicit unknown alternative.
`StoreScalar` also follows its assigned `ValueId`, allowing exact local
second-order writes to update pointer-target constraints without an RHS source
scan. The update remains weak; an uncertain assigned value retains known
targets and marks the written pointer unknown. Assigned-value traversal is
deferred until the resolved targets include a pointer object, so ordinary
scalar stores do not pay for unused second-order provenance.

Pointer-valued scalar initialization and direct assignment now produce a
separate operation-derived constraint stream. The evaluation plan follows
`ValueId` producers and classifies each possible source as a local address,
direct pointer copy, pointer load, or unknown value. Conditional alternatives
are unioned. Transformations such as arithmetic preserve operand-derived
may-targets but also introduce `Unknown`, so precision loss cannot become a
false negative. Dataflow maps stable `PlaceId`s to bindings and skips the old
RHS range scan for each owned pointer write. Syntax discovery remains only for
roots the evaluator rejects.

The operation layer now has a common `SemanticPlace` attachment. Scalar reads,
writes, and address formation attach their declaration-backed place; loads and
stores attach an interned projected place. A projected dereference is keyed by
the address `ValueId` and stored in an evaluation-plan table, while its span is
diagnostic metadata rather than object identity. Direct load/store target
discovery consumes this place table. This establishes the identity boundary
needed to add field, element, global, and unknown-memory places without adding
more syntax-derived aliases.

Pointer-member and indexed reads are the next concrete clients. `p->member`
lowers to a field place keyed by the evaluated base `ValueId` and member name;
`base[index]` lowers to an element place keyed by both evaluated values. Both
are `LoadScalar` operations, so nested base/index reads, writes, calls, guards,
and ordering remain in the common value graph. The memory adapter continues to
mark them incomplete until type layout, decay, and points-to projection can map
them to storage. Projection bases now preserve whether storage is selected from
an evaluated pointer value or a direct object place. Simple `object.member`
reads and plain writes therefore project from the aggregate `PlaceId` without a
fictitious whole-object scalar read. A projection base can itself name an
interned projected place, so nested direct and mixed field chains are built
inside-out without loading aggregate intermediates. Indexed aggregate
intermediates remain open. Direct element lowering now consults the structural
type graph: local array objects project from their `PlaceId`, whereas array
parameters project from their adjusted pointer value. Record bodies now expose
grammar-owned member declarations using the same structural declarators as
ordinary declarations. Stable record identities permit recursive member types,
while block/record extents prevent an inner named tag from leaking beyond its
lexical scope. A simple member chain can therefore carry its field `TypeId` into element
lowering: `object.values[i]` and `pointer->values[i]` use a projected field
place as their element base without loading the complete array. This is member
type resolution, not layout: exact tag scope, anonymous parameter records,
bit-field metadata, offsets, unknown/anonymous union overlap, and complete
field/element points-to projection remain separate obligations.
Element-type propagation now composes through array and pointer layers as well.
For `matrix[i][j]`, the first index produces an `Element` place whose structural
type is still an array; the second index projects from that place and only the
final scalar is loaded. The same rule applies beneath a record field. No row or
array aggregate is manufactured as a scalar `ValueId`.
Projected storage now crosses the public dataflow boundary without being
folded back into scalar bindings. The function-local `memory_regions` table
interns binding roots, field children, and conservative all-element summaries;
`memory_accesses` associates evaluation-plan loads/stores with those IDs and
labels direct-object identity as exact versus pointer/element identity as
may-alias. Thus `object.field` and a locally resolved `pointer->field` share a
region, while distinct index expressions safely overlap in one element
summary. A distinct region reaching-definition solver now strongly kills on
exact writes, weakly joins may-alias writes, unions branch states, and reaches
a fixed point across loop back-edges. Public memory definition/use tables and
edges expose that result without contaminating scalar bindings. This still
The region table also publishes containment and known union-member overlap.
Union siblings participate in reaching flow and exact cross-member kills;
ordinary struct siblings do not. Containment remains evidence rather than a
kill rule until partial aggregate writes can be fragmented soundly. This still
By-value aggregate parameters now seed accessed subregions with explicit entry
definitions. Their fields can contribute positive return provenance until a
strong overwrite kills that incoming definition; branch-local overwrites join
correctly. Direct projected bases are storage identities and are consequently
suppressed from the legacy scalar-use adapter, fixing the false whole-object
read previously emitted for `s.field`. This still leaves
`UnknownMemoryEffect` in place because byte layout, general aggregate
initializers, escaped regions, calls and unknown pointees are not complete.
Pointer-like call arguments now produce weak local call-clobber definitions
for every accessed region reachable from a known target; known scalar pointees
receive the analogous weak compatibility write. This makes potential callee
mutation visible to reaching flow while retaining an unknown-memory issue
until interprocedural effect summaries exist. Ordinary scalar arguments do not
poison memory coverage. Local arrays decay through a distinct `DecayArray`
operation, preserving their storage identity without misclassifying decay as
a scalar read or source-level `&` operation.
The same region edges are consumed by DDG/PDG export and backward slicing.
This keeps projected-memory reasoning on the common dependence spine rather
than requiring a special query path.

Plain pointer-field and element assignments now use `StoreScalar` with the
same `Field` and `Element` place identities as reads. The operation inputs are
base, index where present, and assigned value, so ordering and unsequenced
access checks no longer infer those dependencies from the assignment span.
Without layout/decay projection the memory adapter deliberately records an
unknown effect and emits no fabricated field definition. Compound projected
assignment and prefix/postfix increment/decrement now lower as one prior load,
one replacement computation, and one store over the same place. Address and
index expressions are shared rather than re-evaluated. `StoreScalar` carries
the prior value, assigned value, and whether the enclosing expression returns
the pre-write or post-write value; known pointer targets therefore retain one
memory use/write pair while unresolved fields and elements still fail closed.

Generate explicit points-to constraints from operations:

- address: `p = &object`;
- copy: `p = q`;
- load: `p = *q`;
- store: `*p = q`;
- escape/unknown effect and supported offset transformations.

Use a monotone inclusion analysis with worklists and dependency indexes. The
domain must distinguish no facts yet, uninitialized storage, null, known targets,
and unknown/external targets. An empty target set cannot mean all five things.
Keep a separate must-initialization analysis over the executable CFG.

Initial memory stores may remain weak updates. Permit strong updates only with
an explicit uniqueness/lifetime proof at that program point; a singleton in a
flow-insensitive target set is not automatically such a proof. Unknown stores
must affect the unknown memory region and relevant escaped objects, or invalidate
the corresponding negative result. Fields, arrays and pointer arithmetic extend
this model through typed projections, not additional token scans.

Address-taking is an address operation, not a value definition. Keep the legacy
`AddressTaken` record only at the adapter boundary. Model volatile/atomic effects,
escape, object lifetime and caller-visible memory before making dead-store or
unused-object claims about them.

### 5.7 Reaching definitions, summaries and resource bounds

Retain reaching definitions initially; adopting full SSA or MemorySSA is not a
prerequisite. Expression `ValueId`s describe producer/consumer relations, while
mutable object storage still uses the existing reaching-definition model.

The reference transfer is `OUT = GEN union (IN minus KILL)`, with ordered
per-operation reads and writes and explicit weak-update semantics. Keep dense
value-definition bitsets, indexes and chain/DAG fast paths only where equivalent
to that reference on the same operations. A work bound must report exhaustion;
partial convergence cannot retain a complete flag.

Build summaries by `FunctionId` and `CallSiteId`. Store explicit argument/result
value mappings, scalar return dependencies, global/object effects, escapes,
termination information where known, and coverage issues. Process call-graph
strongly connected components and propagate changes to callers via worklists.
Recursion requires a finite lattice or widening with an explicit reason.

Caller-visible memory summaries are a required later phase of this plan, not a
reserved enum variant counted as implemented support. Keep global object
identities and abstract parameter-pointee regions so writes can be instantiated
at actual call sites. Initial conservative summaries are acceptable if they
include possible effects and report the limits of precision.

Document cost in terms of tokens, operations, CFG edges, definitions, points-to
facts, call-summary facts and emitted dependence edges. Do not promise linear
analysis where the output itself can be quadratic. Apply budgets to parsing,
resolution, type construction, unordered evaluation, memory solving, summaries
and export; report which budget ended the analysis.

### 5.8 Coverage and public APIs

Replace scattered mutable booleans as the source of truth with a coverage ledger.
Each relevant semantic node has a disposition: lowered with a supported rule,
proven non-runtime under resolved semantics, represented conservatively, or
unresolved with an issue. Adding a syntax kind must require a disposition.

Example issues: `UnknownType`, `AmbiguousName`, `RecoveredDeclaration`,
`UnmodeledEvaluation`, `UnknownMemoryEffect`, `ExternalCallee`,
`UnsequencedAccess`, `TargetLayoutRequired`, `BudgetExceeded`.
Each issue has an origin and affected dimensions. Start propagation
conservatively at function/TU scope; narrowing issue scope needs evidence.

Derive `recovery_free`, `effects_complete`, `memory_complete`, `vla_complete`
and summary `complete` from these records for compatibility. Default/no-context
construction is unknown. No positive coverage entry is an automatic proof that
its implementation is correct: independent tests still establish its support.

Add typed Rust/Python results containing payload, diagnostics, semantic issues,
language/target assumptions, analysis revision and graph identity. Slices and
exports carry the same coverage information. Generate Python signatures and
result types from maintained binding/schema metadata; do not maintain handwritten
native stubs or erase the contract with pervasive `Any`.

## 6. Migration sequence and acceptance gates

Each phase has a concrete exit condition. Use reviewable increments that change
one ownership boundary at a time and replace all consumers of the migrated fact.
Track tests and removed heuristics in a phase checklist. A passing old/new digest
alone cannot close a phase that changes known incorrect behavior.

### P0 — Establish a reproducible baseline and defect ledger

1. Inventory pending paths and identify the interrupted captured-VLA patch.
2. Restore a compiling baseline by isolating/removing only that unfinished
   attempt, or replacing it within the first semantic slice. Preserve its
   counterexample as a recorded failing regression; do not add a stub to return
   an empty result or weaken the assertion to manufacture a pass.
3. Record source hashes, dependency lock, toolchain, build profile and native
   extension hash together. Store the known defects outside pass-count claims.
4. Group today's tests by semantic obligation, including compiler-backed,
   abstract-model, metamorphic, structural, API and packaging evidence.
5. Confirm the may-dependence, coverage, dialect and graph contracts above.

Exit: a reproducible build, an attributed baseline, and an explicit list of
known failing obligations. No claim that the baseline is generally correct.

### P1 — Semantic unit, identities and issue propagation

Introduce the owning analysis unit, typed IDs, issue ledger and result envelope.
Route existing analyses through the unit without changing their algorithms yet.
Add an inventory of legacy semantic fallbacks with issue reasons.

Progress: P1 introduces `SemanticIssueKind`, affected
`CoverageDimension`s, optional issue spans, and a per-function
`semantic_issues` ledger. Recovery, unmodeled effects, unknown memory effects,
and unmodeled type-driven values now cross the Rust/Python boundary as stable
reasons. The existing booleans are synchronized at one analysis boundary for
compatibility. `AnalysisUnit` now owns exact source text, syntax, token spans,
merged diagnostics, and executable CFGs. Dataflow consumes that unit once and
emits `SourceUnitId`, dense `FunctionId`, and `ANALYSIS_REVISION`; summaries
preserve those identities and explicitly drop ambiguous duplicate function
identity. Some non-semantic convenience/export entry points do not yet reuse
the unit. Event lowering now emits localized `UnmodeledEffect` and
`UnmodeledTypeValue` issues, and the points-to pass emits localized
`UnknownMemoryEffect` issues. Parser/CFG recovery records each diagnostic
origin while conservatively qualifying all dimensions of the function. The
legacy event-level effects/VLA flags and direct memory-flag mutations have been
removed; public compatibility booleans are derived at the analysis boundary.
General CFG and control-dependence results now carry
`executable_cfg` identity, and slicing builds CFG/dataflow from one unit and can
validate source, function, graph-kind, and analysis-revision coordinates.

Exit: every public analysis can report its source/graph identity and coverage;
missing context or budget exhaustion cannot silently become complete. Duplicate
function and unresolved-name behavior is consistent across all entry points.

### P2 — Structured declarations and lexical resolution

Add parser nodes and the shared lexical declaration service. Migrate parameter
recovery, enum collection, typedef lookup and declaration/cast disambiguation.
Cover named/unnamed parameters, grouped declarators, nested prototypes, tag/member
namespaces, declaration points and recovery rollback.

Progress: the parser now emits one grammar-owned `ParamDecl` child for every
outer declaration in a `ParamList`. Its bounded scanner preserves nested
function-pointer, array, and anonymous-aggregate commas while retaining the
existing missing-closer recovery points. Event binding and source-type recovery
consume one shared `ParameterDeclarations` result per function, and its groups
come directly from those syntax nodes. Parameter name/typedef disambiguation,
`ParameterDeclarator`, and the translation-unit typedef/enumerator/function
index now live in `csource::semantic::declarations`; `AnalysisUnit` constructs
and owns that index once. The second independent parameter-name scan in
`types.rs`, its public helper, `outer_parameter_groups` delimiter reconstruction,
dataflow's translation-unit symbol-table ownership, and metrics' independent
top-level-comma parameter counter have been removed. Source metrics now derive
arity from the same `ParamDecl` children, with `(void)` and `...` handled as
explicit C conventions rather than delimiter arithmetic.
Enum bodies now carry explicit `EnumBody` and `Enumerator` nodes, including
outer parameter enum specifiers. The semantic declaration index collects those
nodes once for file and lexical visibility; both `enum_constants` and
`parameter_enum_constants` token scanners have been removed from event
collection. Nested function-prototype enumerators remain excluded from the
outer parameter scope by construction. The semantic declaration index now
classifies every typedef declarator by exact declaration span, and event/type
collection share semantic identifier extraction; the former event-layer
`typedef_declarator_spans` and `name_of` implementations are gone. A malformed
enum body also recovers at the declaration semicolon, preventing a missing `}`
from consuming later definitions. `AnalysisUnit` now also owns one
`FunctionResolution` per executable function: dense `SymbolId`s, value/typedef/
constant declaration kinds, exact declaration points, and nested compound/`for`
scope ownership. Ordinary `NameRef` binding in dataflow resolves through this
semantic index and maps semantic symbols to legacy `Binding`s only at the
analysis adapter. Opaque VLA/type token recovery now queries that same service
at each token's source position; the event-local `Scope`, `ScopedName`, and
`resolve` implementation have been deleted. `FunctionResolution` also records
each symbol's declarator span, so an `ArraySuffix` obtains its owning value or
typedef declaration structurally instead of relying on the last name observed
by a parallel traversal. This closes the duplicated lexical-ownership part of
P2; structural type identity and bound provenance remain P3 work.

Exit: parameter arity/identity and scope come from declaration nodes. Remove
`outer_parameter_groups`, `parameter_in_group`, `parameter_enum_constants`,
`enum_constants` and redundant typedef scans from event collection for migrated
forms. Known syntax no longer becomes unresolved simply because of parentheses.

### P3 — Structural types and constant/type evaluation

Implement type graphs, layer qualifiers, alias resolution, parameter adjustment,
target-dependent constants and bound slots. Preserve spelling views. Introduce
typed `sizeof`/`typeof` expressions and explicit unknown type results.

Current increment: declarator syntax now preserves `PointerOperator` nodes and
`ParenthesizedDeclarator` nesting. The semantic layer owns a dense per-function
`TypeId` graph with ordered base, pointer, array and function constructors, and
array bounds are distinguished as constant, runtime, incomplete or prototype
star. `AnalysisUnit` builds and owns this graph once alongside declaration
resolution. The legacy public `CType` is now projected from the graph for local
declarations, so the graph participates in the active analysis path rather than
being a disconnected model. In particular, `int *a[4]` is represented as an
array of pointers while `int (*p)[4]` is a pointer to an array. Parameter
declarators, qualifier layers, alias targets, bound-slot identity and typed
`sizeof`/`typeof` consumption remain to be migrated before this phase exits.
Named parameters now enter the same graph with distinct written and adjusted
`TypeId`s: outer array and function parameter types adjust to pointers while
function-pointer parameters remain unchanged. The old parameter-specific
`pointer_depth`/`array_rank` scan has been removed, and `sizeof` ambiguity in
dataflow consults the adjusted structural type. A regression establishes that
`sizeof(matrix)` for `int matrix[4][8]` does not read the parameter value while
the compatibility view still preserves the written two-dimensional spelling.
Qualifier ownership is now structural as well. `PointerOperator` syntax owns
the qualifier tokens following its particular `*`, and `TypeNode::Qualified`
wraps either a base type or an individual pointer layer. Consequently
`const int *p`, `int *const p`, and `volatile int *const volatile p` no longer
collapse to the same internal type. The public `CType` qualifier booleans remain
a deliberately lossy spelling view during migration.
Local typedefs now produce `TypeNode::Alias` nodes carrying exact declaration
identity and a target `TypeId`. Type-driven pointer checks follow qualified and
alias layers iteratively with a graph-size guard, so malformed alias cycles
terminate without guessing a type. This fixes `sizeof` of a local pointer
typedef while keeping the compatibility view as the written alias spelling.
File-scope typedef declarations and chains now enter the same per-function type
graph before parameters and locals. The translation-unit index keeps their
exact spans and ordered visibility rather than a second name-to-offset summary,
so a later typedef cannot affect an earlier function. Parameters and locals
using a visible file alias now follow its target for type-driven decisions while
retaining their source spelling. Type construction no longer treats an
unresolved spelling or missing specifier as a resolved opaque base:
`TypeNode::Unknown` retains a reason, and missing alias targets and alias cycles
produce localized type issues. Those issues cross the dataflow boundary as the
stable `UnknownType` reason and qualify effects, memory, type-value, and summary
negative claims through the coverage dimensions. Compatibility completeness
flags are now derived from those dimensions rather than a hard-coded subset of
issue variants. Complete validation of C specifier combinations, target-aware
builtin types, and captured-bound propagation remain required. Runtime array
bounds now have dense `BoundSlotId`s in the function type snapshot rather than
embedding an untyped source span in `ArrayBound`. Each slot owns the exact
declaring symbol span and bound-expression span, and the active event path uses
the structural slots to classify variably modified bindings. This establishes
identity and ownership for later lowering. A simple value-form `typeof(a)` now
has an explicit structural node referring to `a`'s type and reuses its bound
slot identities through chained captures; event classification consumes that
structural fact. Basic bound-slot uses now begin the P4 lowering described
below; compound bound effects and general expression operations remain open.

Exit: pointer-to-array and array-of-pointer are distinguishable; all type-driven
decisions use `TypeId`/type instances. Captured bounds are represented without
pretending they initialize object contents. Typedef cycles/recovery terminate
with issues rather than recursive overflow or guessed types.

### P4 — First complete vertical slice: declarations through VLA returns

Lower scalar expressions, declarations, captured bounds, `sizeof`, `typeof`,
returns and the control required by their operands to explicit operations.
Connect those operations to reaching definitions and public summary results.

Progress: each runtime bound slot records the resolved value declarations used
by its expression. A later `sizeof` of the VLA or a simple `typeof` capture is
lowered as a use of the definitions that reached those inputs when the bound
was formed, rather than as a fresh read of the current variables. Thus capture
before `n = 1` retains the parameter dependency, while capture after the write
does not. Declarations with a possible runtime bound now have an executable CFG
node even without an initializer, fixing their ordering relative to earlier
statements instead of falling back to entry. The direct
`int a[n]; typeof(a) b; return sizeof(b);` slice now produces a complete
parameter-to-return path. Redundant parentheses around a value-form `typeof`
operand preserve the same capture, and both `sizeof` of a VLA typedef and an
object declared through that typedef reuse the captured slot. Conditional and
call operations and general scalar expressions remain open. For supported
direct bounds inside a loop, structural validation now proves the `FormBound`
operation's CFG owner lies on the repeated cycle rather than at entry or on a
one-shot projection. Side-effecting conditional arms and calls still keep P4
open.

The first `csource::eval` ownership boundary now exists. `AnalysisUnit` owns one
`EvaluationPlan` per function, aligned with its declaration resolution, type
graph, and executable CFG. The plan emits explicit `FormBound` and `ReadBound`
operations carrying dense `OpId`s, `BoundSlotId`s, declaration identities,
deterministic source order, and executable-CFG node ownership. Dataflow lowers
those operations and uses their placement; it no longer discovers
captured-bound consumers or ambiguous single-name `sizeof` operands itself.
The former `ambiguous_type_spans` and `lone_parenthesised_name` event-layer
helpers have been removed. A bound in a function parameter may be placed at
entry, while an unreachable body declaration with no executable CFG owner is
excluded instead of silently relocated to entry. General scalar expression
operations and sequence relations still need to join this plan before the rest
of the event heuristics can be retired.

Direct scalar bound evaluation now enters that operation stream as explicit
`ReadScalar` and `WriteScalar` operations ordered before `FormBound`, with the
same `BoundSlotId`, declaration identity, occurrence span, and CFG owner. The
active dataflow adapter consumes these operations for a direct identifier,
prefix/postfix increment or decrement, and assignment of an integer literal;
the corresponding `ArraySuffix` token-event path is bypassed. These forms no
longer manufacture `UnmodeledTypeValue`: assignment correctly kills the prior
parameter value, while increment reads the pre-write value captured by the
bound. Compound assignment by an integer literal now uses the same explicit
read-before-write sequence and is complete. Top-level comma bounds composed of
the supported scalar atoms are also lowered in source order. Crucially,
`FormBound.inputs` is distinct from the set of executed scalar operations:
`(n++, m)` records the mutation of `n` but captures only `m`, and `(n++, 4)`
records the mutation while capturing no value dependency. This removes comma
expressions made entirely from these forms from the legacy scanner without
turning discarded values into false `sizeof` dependencies. General binary
expressions and calls remain open. A flat side-effect-free binary bound whose
two operands are resolved identifiers or integer literals now lowers to one
`ComputeScalar` operation. Arithmetic, shift, bitwise, comparison and equality
operators retain their explicit operands, operator spelling, span, CFG owner
and bound identity; the result inputs, rather than a later token scan, drive
captured-bound provenance. Nested expressions and operators with conditional
evaluation remain outside this deliberately exact fragment. A side-effect-free
ternary with identifier or integer-literal arms now lowers to one
`SelectScalar` operation. It records
the condition and the two possible value inputs without representing both arms
as executed writes, and the conservative bound result may depend on the
condition and either non-constant arm. Side-effecting arms still require
explicit branch regions before they can leave the legacy fail-closed path.

A direct call with scalar identifier or integer-literal arguments now lowers to
one `CallScalar` operation before `FormBound`. The operation retains the callee
spelling, call span, resolved argument identities, CFG owner and `BoundSlotId`;
its resolved arguments therefore remain visible as inputs to a later captured
bound read. This is deliberately a partial semantic model, not a completeness
claim: until call-summary substitution and external-effect modelling join the
operation pipeline, every such operation emits localized
`UnmodeledTypeValue` and `UnmodeledEffect` issues. Consequently a summary may
report the known argument-to-return dependence while remaining explicitly
incomplete. More complex arguments continue through the legacy fail-closed
path rather than being flattened into a misleading call record.

`SelectScalar`, `ComputeScalar`, and `CallScalar` expose their resolved operands
through one operation-level `scalar_inputs` contract. The active dataflow
adapter consumes that contract once instead of maintaining a separate operand
walker for each expression variant. Evaluation plans now assign dense
`ValueId`s to declaration-backed source values and to every operation result;
each `EvaluationOp` owns explicit input IDs and one output ID. `FormBound`
consumes the preceding result for its `BoundSlotId`, and `ReadBound` consumes
that formation result. Bound provenance is projected by walking these producer
edges back to declaration values rather than by reading `FormBound.inputs`
directly. A discarded constant comma result intentionally leaves formation
with no value input, preventing an earlier side effect from becoming a false
captured dependency. Legacy expressions that have not yet been lowered enter
the value graph as explicit declaration-backed sources, preserving conservative
behavior during migration. Direct operand events and transitive result
dependencies remain separate: consuming a value edge does not manufacture a
second source read. Scalar operands now distinguish declaration-backed values
from integer constants explicitly. Constants retain spelling and occurrence,
occupy their own dense value inputs, and remain present in selection,
computation, and call argument order without creating false declaration reads
or captured provenance. A constant that is itself the result of a sequenced
bound segment now has an explicit `ConstantScalar` producer, so `(n++, 4)`
captures that constant result rather than either dropping the result edge or
mistaking the preceding mutation for the bound value. Literal assignment and
compound-assignment operands are explicit write inputs. Scalar writes also
state whether their enclosing expression yields the pre-write or post-write
value: postfix increment routes `FormBound` from the preceding read result,
whereas prefix increment, assignment, and compound assignment route it from
the write result. Storage mutation and expression result are therefore no
longer conflated in the value graph. Nested side-effect-free binary expressions
now lower recursively with C operator precedence into a post-order operation
graph. An intermediate operand is a producer reference resolved to the earlier
operation's `ValueId`, not a flattened list of leaf names; `FormBound` consumes
only the root result while provenance reaches declarations through the value
edges. Parentheses and mixed-precedence arithmetic therefore preserve tree
shape, and every input ID precedes its consumer output. Pointer unary
operations and side-effecting binary operands still need general expression
producers before the cutover is complete. The side-effect-free scalar unary
operators `+`, `-`, `!`, and `~`
now lower as
`UnaryScalar` producers and may consume either a declaration/constant source or
an earlier nested result. Pointer dereference and address-of remain outside this
path intentionally: they require typed places and memory effects rather than a
scalar dependency approximation.

One call may now appear anywhere inside the recursively lowered pure scalar
tree, and each call argument may itself be a unary/binary expression. Argument
roots feed `CallScalar` by `ValueId`; later scalar operations consume the call
result in turn, so `opaque(n + 1) * 2` has a topological computation-call-
computation chain rather than flattened argument spans. Calls are ordered by
their completed extent, ensuring argument producers precede the call. The call
continues to emit explicit type-value and effect uncertainty. Expressions with
multiple calls deliberately remain on the fail-closed path because their
relative evaluation order is generally unspecified in C; lowering them needs
an unordered/indeterminately-sequenced relation, not an arbitrary source order.

Conditional selection accepts unary/binary subgraphs, increments, and one
nested call across its condition and arms. `SelectScalar` consumes the three
root `ValueId`s, and `FormBound` consumes only the selector result; leaf
provenance remains reachable through producer edges. Arm operations carry an
`ExecutionCondition` resolved to the corresponding CFG edge. A scalar write is
therefore placed only on its taken arm; calls retain conservative effect
incompleteness until the general call-effect model consumes these regions.

Pure `&&` and `||` bounds lower to a dedicated `ShortCircuitScalar` producer
rather than an ordinary binary computation. The operation consumes the left
and right root `ValueId`s plus an explicit language-defined bypass constant:
zero for `&&`, one for `||`. Computation, increments, and one nested call inside
the right operand retain guarded operations. A guarded write is placed on the
taken arm, so reaching definitions preserve the incoming value along the
bypass edge instead of treating the write as unconditional. Calls remain
effect-incomplete and cannot certify a negative result; their complete memory
and call effects still belong to the common operation pipeline. Literal-dead
call regions are filtered before this qualification, so `1 || opaque(n)` does
not acquire uncertainty for a call the language proves cannot execute.
Assignment expressions in the bound slice consume an explicit assigned RHS.
Compound assignments additionally consume the target's explicit prior value
before the RHS, rather than inferring it from whichever slot operation happened
to be most recent. Computed RHS producers precede the write, postfix increments
select the pre-write value, and plain assignment provenance follows the
replacement value rather than the replaced target.
Operation inputs are now an ordered edge list rather than a mathematical set.
Construction no longer sorts or deduplicates them: operand position and
multiplicity survive for non-commutative operators, repeated operands and call
arguments. Read-modify-write operations put the prior stored value before the
assigned operand. This invariant is required before any condition input can
reliably own control.

Expression-root identity is now independent of `BoundSlotId`. Supported pure
scalar initializer and return expressions receive dense `EvaluationId`s after
the function's bound evaluations and end in an explicit `FinishExpression`
operation. That consumer records whether the value initializes a resolved
declaration or leaves through a particular return statement, consumes the
expression producer by `ValueId`, and shares its executable CFG owner. This is
the first ordinary-expression client of the value graph. Dataflow consumes
the supported subset: exact legacy read occurrences are replaced by reads
from operation inputs at their operation-owned CFG nodes, while an initialized
declaration takes its CFG node and visibility point from `FinishExpression`.
Scalar assignments, compound assignments, and increments within those roots
also replace their exact legacy promoted definitions. Their read-before-write
inputs, completion offsets, and guarded CFG-arm placement come from the
operations. Event emission uses only direct operation inputs: a produced value
may retain scalar provenance for dependency tracing, but consuming it at a
join does not execute the original read again. Unsupported roots remain wholly
on the legacy path rather than mixing two authorities. A supported ordinary
root may now contain calls when no scalar write shares
the root. Direct argument reads come from operation inputs, and direct-return
identity is the explicit edge from the `CallScalar` output to a return
`FinishExpression`, replacing the corresponding syntax inference. The
existing `CallRecord` remains the effect bridge to defined-callee, intrinsic,
and external-policy summaries, but migrated `CallScalar` operations now
construct those records directly. The syntax call walk no longer independently
decides their callee, ordered argument extents, direct bindings, or return
identity. Calls through names resolved to local or parameter objects are
declined rather than misclassified as direct; they retain the indirect-call
fallback. Bound calls alone retain the VLA adapter's
deliberate type/effect uncertainty. Value dependencies and guard conditions now
emit explicit `SequencedBefore` relations. Calls without an order path are
`IndeterminatelySequenced`; calls in opposite conditional arms are
`MutuallyExclusive`. Nested, sibling, and alternative calls can therefore use
the common plan without inventing source order. Call/write pairs are also
admitted when operation dependencies prove the call feeds the assignment or
the assignment feeds a call argument. Opposite conditional arms are admitted
from their mutual-exclusion relation and retain guarded CFG placement. Pairs
without either proof remain on the qualified legacy path. Recursive scalar
leaves now lower to explicit `ReadScalar`
producers, which keeps sibling operand reads distinct from the later operation
that consumes both values. The admission check consequently rejects same-object
read/write or write/write pairs without dependency order, short-circuit order,
or mutual exclusion. Such roots emit a localized `UnsequencedAccess` issue and
cannot certify effect completeness; sequenced and mutually exclusive forms stay
on the common path. Comma expressions now lower through `SequenceScalar`, whose
discarded operand contributes execution order but not value provenance. The
same recursive lowering serves VLA bounds, initializers, and returns; the older
bound-only scalar implementation has been removed. Consequently `(a(), n)`
retains the call effect while only `n` feeds the consumer, and `(n++, n)`
explicitly orders the increment before the returned read. Expression statements
now use `EvaluationPurpose::Discard`: supported scalar statements replace their
legacy read/write/call events without treating the final value as consumed, and
unsequenced conflicts receive the same localized qualification as initializer
or return roots. Expression token lookup is bounded by binary search over the
ordered token-span table rather than rescanning the translation unit per root.
Control conditions for `if`, `while`, `do`/`while`, `for`, and `switch` now use
the same root contract through `EvaluationPurpose::Control`. The purpose names
the owning statement and control kind, while `FinishExpression` consumes the
condition's exact produced value. `for` lowering selects only the `ForCond`
grammar child for control. Its expression-form `ForInit` and `ForStep` children
now become separate discarded-value roots with explicit lifecycle phases and
their existing distinct CFG owners. A C99 declaration-form initializer remains
an `Initialize` root for the declared object rather than receiving a second
clause owner.
Dataflow replaces supported condition occurrences with operation-owned reads,
writes, and calls, and an unsequenced conflict in a condition receives the same
localized incomplete-effects result as one in an initializer, return, or
discarded expression. These operations are currently placed on the existing
structural CFG condition node. CFG construction does not yet consume the
operation plan, so the one-spine architecture is advanced but not complete.
GNU computed-`goto` operands now enter the same plan with an explicit
`IndirectDispatch` purpose. Because the parser represents `goto *expr` as an
outer unary node, lowering removes exactly that statement-level marker before
processing `expr`; it does not pretend the marker is a memory load. Supported
operand reads, calls, writes, and ordering replace their legacy events, while
an unsequenced conflict records localized effect incompleteness. A direct
`goto label` has no evaluated operand. Dispatch target-set resolution remains
a separate semantic consumer until CFG construction itself consumes values.
`AnalysisUnit` now initializes evaluation plans through `OnceLock`, just as it
already did for dataflow and summaries. Construction still owns the exact tree,
CFG, resolution, and type inputs, but AST and CFG-only exports no longer lower
unused operations. On the determinism corpus worker this reduced a measured
debug-build run from 30.04 seconds to 27.95 seconds; the timeout gate, not that
single local number, remains authoritative across machines.

The evaluation plan now assigns dense `GuardId`s to the true and false regions
of `SelectScalar` and to the conditionally evaluated right region of `&&` or
`||`. Each guard records its owning join operation, exact condition `ValueId`,
polarity and source extent. Every operation states an `ExecutionCondition`:
unconditional or guarded by an ordered list of predicates. Nested conditionals
accumulate both outer and inner guards. Guard polarity is resolved against the
general CFG's stable true/false edge kinds when the corresponding condition
node exists; the guard retains the branch and arm-entry node identities.
Missing control is represented as `GuardControl::Unresolved`, never silently
treated as unconditional. Local `ArraySuffix` bounds are now grammar-owned
expression children: the general CFG sees their `&&`, `||`, and `?:`, and pure
bound guards resolve to concrete branch and arm-entry node identities. The
parser suspends declarator parsing onto its bounded task stack while the common
expression grammar owns the bound, preserving the no-native-recursion
invariant. Evaluation-plan events are exclusive inside successfully lowered
bounds, and source-occurrence deduplication prevents overlap with the
conservative adapter when lowering declines an ambiguous expression. Parameter
declarator interiors and type-name bounds are still token-owned. Short-circuit
bypass constants and guarded scalar writes are now explicit; guarded calls
retain their operation and conservative incompleteness. The next P5 step is to
make joins reusable beyond bound-specific select/short-circuit operations and
move general call effects off the legacy event stream.

The first demonstration must include all of:

- direct `a[n]`, typedef capture, `typeof(a)`, and capture followed by `n=1`;
- VLA type formation that increments `n`, including a declaration without an
  initializer and a declaration in a loop;
- unevaluated scalar `sizeof(n++)` and nested contexts;
- conditional and comma bounds with real sequencing;
- parameter array adjustment and nested prototype scope;
- unknown bound calls producing visible uncertainty.

Exit: `int a[n]; typeof(a) b; return sizeof(b);` reports the correct dependence
through a captured bound, with independently supported coverage. This phase
cannot be closed merely by changing its flag to false. Remove the migrated
`extent_*`, `typeof_vla_type_operands`, and duplicate evaluatedness paths.

### P5 — Common expression, effect and CFG lowering

Extend the operation pipeline across assignments, increments, initializers,
calls, short-circuit expressions, statement expressions and supported builtins.
Resolve labels and dispatch, then prune the finalized graph. Define the
nontermination/post-dominance contract. Support unknown operations explicitly.

Exit: event collection and provenance no longer infer semantic inputs or
ordering from byte containment. Dataflow and CFG share evaluatedness. Comma,
initializer, builtin and nested-goto regressions pass through the new path.
Legacy source graph projections have documented mappings and changes.

### P6 — Typed memory constraints and solver consolidation

Generate points-to constraints from typed operations. Migrate local object,
pointer-copy/load/store, address, escape, initialization and projection handling.
Retain existing weak-update guarantees. Compare the optimized solver paths with
the independent reference and profile established scaling workloads.

Exit: all pointer regression families use the shared operations, and all
observed concrete writes are included in the may-dependence results. Unknown
targets, lifetimes and unsupported projections cannot certify absence. Remove
expression rescanning from `memory.rs`.

### P7 — Interprocedural effects and query contracts

Replace name/span-based summary application with call-site/value identity.
Add parameter-pointee and global memory effects, recursive SCC solving and issue
propagation. Convert slicing/reachability to coverage-bearing results.

Exit: ordinary caller-visible stores are represented across calls; known
incomplete callees contaminate relevant absence claims; duplicate names and
indirect calls retain correct identity/uncertainty. A found may-path has an
explanation, and `NoMayPath` has checked coverage requirements.

### P8 — Cutover, documentation, consumer migration and publication

Remove obsolete scanners and duplicated semantic policies after all replacement
paths are covered. Reconcile README, reference pages, examples, generated stubs,
compatibility adapters and support claims. Replace accumulated per-bug capability
prose with a maintained language/analysis support matrix.

Migrate Glaurung first against a pinned local/Git candidate, retaining its LLIR,
solver and KB adapters. Inspect its lowering types and resolver for reusable
rules before writing adapter logic; do not import its application dependencies
into the standalone core or treat its interpreter as an independent C oracle.

Build crate/wheels/sdist once for the reviewed candidate; run the existing
artifact and platform gates. Publication and Glaurung's dependency migration
have separate evidence and completion records; neither is a circular prerequisite
for obtaining the other's registry artifact.

Exit: one owner of source semantics, no legacy scanner fallback for migrated
features, reviewed API changes, reproducible candidate evidence and accurate
publication/consumer status. A full CPG/query language remains a separately
specified product surface; this plan does not declare Joern parity by inference.

## 7. Validation strategy: test the laws and their combinations

### Independent evidence layers

1. **Grammar and identity:** inspect declaration/type/expression structure;
   compare valid cases with pinned Clang AST facts after normalizing compiler
   IDs and implicit nodes. Exact Clang AST layout is not the public contract.
2. **Execution semantics:** compile checked-in, well-defined C fixtures with
   GCC and Clang at O0 and O1; record dialect, target and compiler versions.
   Observe side effects and results. A runtime execution samples behavior; it
   cannot prove absence of all dependence or all undefined behavior.
3. **Abstract semantics:** maintain small independent interpreters for restricted
   scalar, sequencing, bound-capture and alias fragments. Exact equality is
   appropriate only for fragments where the oracle is exact. For may-alias
   results, require inclusion of concrete dependencies and measure excess edges
   separately.
4. **Algorithms:** test joins, transfers, monotonicity, fixed points and graph
   reachability independently of C parsing. Compare chain/DAG optimizations with
   a simple reference on generated graphs and explicit operation sequences.
5. **Metamorphic behavior:** alpha-renaming, safe parentheses, qualified type
   aliases, and permitted declaration rewrites preserve semantic facts after ID
   remapping. Do not assume arbitrary statement/declaration reordering is legal.
6. **Recovery and budgets:** malformed/decompiler input must terminate within
   budgets, preserve source ranges and surface uncertainty. Function/edge
   presence and deterministic serialization are structural evidence only.
7. **API and artifacts:** assert Rust/Python schema consistency, graph ownership,
   diagnostics, source mapping and independent installed-package behavior.

Run compiler cases only where the language permits the construction. Runtime
oracles use safe dimensions, initialized objects and defined arithmetic; separate
invalid/unsequenced cases into diagnostic expectations. Pin upstream test inputs
and licenses if importing compiler regression fixtures. No external Joern or
DecBench run is required for this semantic programme.

### Required interaction matrix

Cross language constructs with evaluation context and analysis consumer:

| Axis | Required representatives |
| --- | --- |
| Declaration identity | file/block/for scope, shadowing, tags, members, enums, prototypes, duplicates, missing headers |
| Type shape | scalar, qualified pointers, arrays, pointer-to-array, typedefs, functions, incomplete and variably modified types |
| Evaluation | runtime, scalar `sizeof`, VLA `sizeof`, `typeof`, `_Alignof`, generic selection, discarded value, short-circuit arm |
| Ordering/control | comma, multi-declarator initialization, loops, nested labels, switch dispatch, unspecified operand order |
| Memory | local address, copies, reassignments, pointer loads/stores, unknown targets, escape, fields, elements, lifetimes |
| Consumer | CFG, definition/use edges, return summary, call reachability, slice, serialized report |

Use deterministic pairwise generation plus mandatory higher-order combinations
from today's failures. Every generator needs a validity rule, an independent
oracle appropriate to its claim, and shrinking to a persistent minimal fixture.
Persist counterexamples so seed changes do not erase them.

Mandatory regression obligations include:

- typedef-name ambiguity changes both parsing and binding correctly;
- `&x` has no stored-value read/write;
- `(p=&y,0); *p=x` preserves the assignment's effect;
- `int a=x,b=a` and `int a=0,b=x` remain distinct;
- nested dead-looking labels remain reachable through legal jumps;
- `sizeof` of a VLA uses its captured size after the original variable changes;
- storing array elements does not redefine the array's size;
- VM bound evaluation inside a loop is placed at the loop execution point;
- unknown calls/stores and resource exhaustion cannot yield an unjustified `no`;
- every emitted use/definition/call belongs to an explicit operation and block.

### Evidence and performance reporting

Report semantic obligations covered, unsupported constructs encountered,
false-positive and false-negative examples, and unknown-result rates by family.
Keep denominators for functions, fixtures and generated cases explicit. Thousands
of parametrized serialization checks are not thousands of independent C rules.

Retain existing benchmark shapes: wide parameters, long call chains, reverse
pointer copies, repeated stores/loads, repeated assignments, deep nesting and
mixed control. Compare identical source populations and build profiles; report
operations, emitted edges, peak memory and timings. Set regression budgets after
measuring the compiling baseline. Do not repeatedly rebuild release archives as
part of a local semantic edit unless the package boundary itself changed.

## 8. Completion criteria and immediate next work

This foundation is complete only when:

- migrated C syntax has one grammar/semantic owner;
- names resolve by declarations and scope, types have structural identity, and
  bound captures retain their runtime provenance;
- executable CFGs, value flow and effects share a lowering;
- source positions are labels, not semantic ordering or binding evidence;
- unknown operations and incomplete iterations remain visible in every relevant
  public result;
- local and cross-call memory effects satisfy their documented abstractions;
- independent semantic and algorithmic tests cover the interaction matrix;
- legacy token rescanners are removed for migrated features;
- docs, Rust/Python contracts and the Glaurung ownership boundary agree.

There is no claim here that finishing these steps proves arbitrary C correct.
It establishes a testable semantic architecture whose supported rules can be
extended without rewriting the same distinctions in every consumer.

Immediate implementation order: P0 baseline, P1 identities/coverage, P2 structured
declarations, then P3/P4 as the first end-to-end demonstration. Review that slice
before widening the operation model. Do not close the VLA issue by merely adding
another token scan or a permanent completeness downgrade.

## 9. References and interpretation

- [Original review](https://github.com/mjbommar/cindergraph/blob/main/review/design-review-2026-09-14.md) and
  [QA sequence](https://github.com/mjbommar/cindergraph/blob/main/review/README.md): local historical evidence, superseded
  where the current source differs.
- [Extraction provenance](../../EXTRACTION.md) and
  [Glaurung relationship](glaurung.md): source origin and ownership.
- [WG14 N1570](https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1570.pdf):
  C11 committee draft; use sections 6.2.1–6.2.3 for scope/namespaces,
  6.5 for expression sequencing, 6.5.3.4 for size/alignment operators,
  and 6.7.6.2–6.7.6.3 for arrays and function declarators. This is a draft
  reference, not a claim to cover later standards automatically.
- [GCC `typeof` documentation](https://gcc.gnu.org/onlinedocs/gcc/Typeof.html):
  operand evaluation depends on variably modified type, which is why type
  resolution must precede the evaluation decision. The page also distinguishes
  C23 `typeof_unqual`; versioned language behavior belongs in the options.
- [Clang internals](https://clang.llvm.org/docs/InternalsManual.html): useful
  reference for preserving source-oriented syntax, declaration identity and
  qualified/canonical types. The proposed Rust structures above are Cindergraph
  design choices, not a proposal to duplicate Clang's class hierarchy.

External references were consulted on 2026-09-14. The proposed architecture and
phase gates are engineering conclusions from this review. They are not results
established by those reference implementations.
