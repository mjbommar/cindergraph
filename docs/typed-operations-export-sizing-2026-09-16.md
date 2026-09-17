# Sizing the typed-operations export, 2026-09-16

Item 3 of [the improvement list](improvement-list-2026-09-16.md) asks for an
export of Milestone B's evaluation lowering: operations with explicit value
inputs, widths, signedness, conversions and control positions. Before designing
it, this note answers, with file and line, what of Milestone B exists in the
tree at `6971e30`, and therefore what the export can be built on. Line numbers
are for that commit.

## What exists

Milestone B's lowering is `crates/cindergraph/src/csource/eval/mod.rs`, one
`EvaluationPlan` per function, built lazily and cached by the owning
`AnalysisUnit` (`semantic/mod.rs:265-280`, `evaluations()` behind a
`OnceLock`). The plan is a `Vec<EvaluationOp>` (`eval/mod.rs:537-548`): each
operation has a dense `OpId`, an owning `EvaluationId` (one per expression
root), a `cfg_node`, topologically ordered `inputs: Vec<ValueId>`, one
`output: ValueId`, an optional place, an `ExecutionCondition` (unconditional or
guarded by a list of `GuardId`s, `eval/mod.rs:190-193`), and its `TypeValueOp`
kind (`eval/mod.rs:216-341`).

Expression roots come from `ordinary_expression_roots`
(`eval/mod.rs:1673-1860`): scalar initializers (`Decl` with an expression
`Initializer` child), `return` operands, expression statements, computed
`goto` operands, the conditions of `if`/`while`/`do`/`switch`, and all three
`for` clauses. Each root gets an `EvaluationPurpose` (`eval/mod.rs:454-477`)
and ends in a `FinishExpression` consumer. VLA bounds are a separate root kind
(`FormBound`/`ReadBound`).

Within a root, `lower_scalar_expression` (`eval/mod.rs:2249-2781`) lowers, by
re-parsing the root's token run with its own precedence table
(`top_level_binary`, `eval/mod.rs:3153-3185`):

| form | operation | line |
| --- | --- | --- |
| integer literal | `ScalarOperand::Constant { spelling }` (a value with no producer, except the short-circuit bypass and the projected-increment `1`, which are `ConstantScalar` ops) | 2258-2267, 2384-2390, 2755-2761 |
| local or parameter name | `ReadScalar`; a local array name becomes `DecayArray` | 2268-2297 |
| `++x`, `x++`, `--x`, `x--` on a local | `ReadScalar` + `WriteScalar { kind: Increment, assigned: None }`; the `+ 1` is implicit | 2298-2336 |
| `++p->f`, `a[i]++`, `(*p)++` | `LoadScalar`, `ConstantScalar 1`, `ComputeScalar +/-`, `StoreScalar` | 2337-2404 |
| `a, b` | `SequenceScalar` | 2405-2432 |
| `x = e`, `x op= e` on a local | `WriteScalar { Assign \| CompoundAssign, prior, assigned }`; for a compound assignment the arithmetic is implicit in the write, no `ComputeScalar` | 2433-2477 |
| `*p = e`, `a[i] op= e`, `s.f = e`, `p->f = e` | `LoadScalar` (compound only), `ComputeScalar` (compound only), `StoreScalar` | 2478-2531 |
| `c ? a : b` | `SelectScalar`; each arm's operations are guarded | 2532-2558 |
| `f(args)` where `f` is not a local object | `CallScalar` | 2559-2601 |
| `p->f`, `s.f`, `a[i]`, `*p` | `LoadScalar` with a `ProjectedAccess` | 2602-2683 |
| `&x` | `AddressOf` | 2684-2704 |
| `-e`, `+e`, `!e`, `~e` | `UnaryScalar` | 2721-2737 |
| `a && b`, `a \|\| b` | `ConstantScalar` (the bypass value, 0 or 1) + `ShortCircuitScalar`; the right operand's operations are guarded | 2748-2768 |
| every other binary operator (`* / % + - << >> < <= > >= == != & ^ \|`) | `ComputeScalar { operator }` | 2769-2775 |

Guards are attached by span containment after placement
(`attach_conditional_regions`, `eval/mod.rs:982-1058`) and resolved to CFG
edges (`guard_cfg_edge`, `eval/mod.rs:3195-3222`). Language-defined order
(`SequencedBefore`, `IndeterminatelySequenced`, `MutuallyExclusive`) is in
`attach_evaluation_order` (`eval/mod.rs:855-980`).

## What is legacy or `unknown`

Anything `lower_scalar_expression` returns `None` for makes the whole root
`ScalarRootFailure::Unsupported`, and `EvaluationPlan::build` then **drops the
root without a record** (`eval/mod.rs:630`, `Err(Unsupported) => continue`).
The dataflow layer still handles it on the legacy event path
(`dataflow/events.rs`), but the plan has no `unknown` operation and no reason.
Measured on 2026-09-16 with a scratch test over the consumer's shapes, these
roots produce zero operations:

- a cast, `(unsigned char) n` (no cast rule; `unary_operator`,
  `eval/mod.rs:3120-3123`, accepts only `+ - ! ~`);
- `sizeof(int) + n` (`sizeof` is only recognised as a standalone ambiguous
  name at `eval/mod.rs:641-667`, never as an operand);
- `glob + n` with `glob` at file scope (`resolve_at` yields no
  `SymbolKind::Value`, `eval/mod.rs:2268-2271`; file-scope objects have no
  `PlaceId`);
- `x > 1.5`, any float, character or string literal (`is_integer_literal`,
  `eval/mod.rs:3187-3193`);
- a call through a local function pointer (`calls_a_resolved_object`,
  `eval/mod.rs:1880-1894`);
- a braced initializer (`ordinary_expression_roots` only takes an expression
  child of `Initializer`, `eval/mod.rs:1694-1702`).

A root with two side effects and no dependency, short-circuit, or
mutual-exclusion proof between them is recorded (`unsequenced_roots`,
`eval/mod.rs:626-629`) but likewise carries no operations. An operation whose
span lands outside every CFG node is dropped silently by the placement
`filter_map` (`eval/mod.rs:679-686`).

## Widths and signedness: structure only

The plan carries **no types**. `ComputeScalar` has `operator: String`,
`operands` and a `span` (`eval/mod.rs:284-289`); a constant is its spelling
(`eval/mod.rs:406-409`); a `ReadScalar` names a `PlaceId` and a declaration
span. There is no `Convert` operation (the plan's own list at
`docs/architecture/semantic-foundation-plan.md:416-420` names one), no
promotion, no usual-arithmetic conversion, no assignment conversion. `s += 2`
on an `unsigned short` is `ReadScalar s` then
`WriteScalar { CompoundAssign, prior: s, assigned: "2" }`; nothing says the
addition happens in `int`.

The types exist elsewhere. CG-EXPORT's `semantic/expr_types.rs`
(`ExpressionTyper::compute`, `expr_types.rs:469-482`) assigns every AST
expression node a `CType` after lvalue conversion, promotion and the usual
arithmetic conversions (`binary`, `expr_types.rs:619-698`; `conditional`,
`expr_types.rs:701-730`; `integer_literal`, `expr_types.rs:1330`), and for a
`binary_expr`/`assign_expr` records the operand conversion type per operator
(`operand_types`, `expr_types.rs:444`). It is keyed by AST `NodeId`, and
its widths are LP64 (`IntKind::width`, `expr_types.rs:85-94`). The AST export
already reads it (`export.rs:462-575`).

## Per-function and deterministic

Yes to both. One plan per `FunctionCfg`, in function table order. Operation ids
are assigned in placement order over a `Vec` (`eval/mod.rs:692`); every
map is a `BTreeMap`; the CFG owner lookup sorts by `(width, index)`
(`eval/mod.rs:678`). Two builds of the same source give equal plans, and
the AST export it will be joined to is already covered by
`python/tests/test_process_determinism.py`.

## Decision for the export

Build `repr="ops"` on the plan, not on a walk of the typed AST, and get the
types by joining each plan operation to the AST expression node with the same
span, read through the same `ExpressionTyper` the AST export uses. The plan is
the evaluation model (order, guards, places, CFG owner); the typer is the type
model; the export adds nothing the two do not already say except the
conversion operations, which are derived mechanically from (value type,
conversion target type) pairs the typer already reports:

- a `ComputeScalar` operand whose value type differs from the typer's
  `operand_type` for that operator gets a `convert` (`promotion` when the
  target is the operand's own promoted type, else `usual_arithmetic`, with
  the promotion emitted first when both apply);
- the right operand of a shift is promoted on its own;
- a `WriteScalar`/`StoreScalar`/initializer whose assigned value type differs
  from the target's type gets an `assignment` conversion;
- a compound assignment and an increment are expanded to read, promote,
  `binary`, assignment-convert, store, since the plan keeps their arithmetic
  implicit;
- a cast is not in the plan, so `cast` conversions cannot appear yet; the root
  is exported as `unknown`.

One join gap: a flat chain `a + b - c` is one AST node, so the inner
`ComputeScalar` for `a + b` has no node to read a result type from. The typer
computes that prefix type and discards it (`expr_types.rs:527-535`); the
export needs it kept, which is a `Vec<CType>` beside `operand_types`, not a
second resolver.

To honour "never omitted", `EvaluationPlan::build` will record the roots it
declined with the reason it declined them (unsupported form, unsequenced
effects, unplaced operation, braced initializer), and the export writes one
`unknown` operation per declined root. This is the only change to the eval
module.

What this export therefore cannot yet show, because the lowering cannot: casts,
`sizeof` as an operand, file-scope objects, non-integer literals, calls through
function pointers, braced initializers. Each is an `unknown` with its reason.
