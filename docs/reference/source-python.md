# Python source analysis

Cindergraph provides a tolerant C parser, metrics, and several graph analyses.
It is pre-alpha. It does not execute C, resolve includes, or implement arbitrary
Joern queries. The general CFG and the comparison-oriented parity CFG serve
different purposes and should not be interchanged.

## Analyse source and inspect diagnostics

```python
import cindergraph as cg

code = "int choose(int x) { return x ? 1 : 0; }"
report = cg.analyze(code)
assert [function.name for function in report.functions] == ["choose"]
assert not report.diagnostics
assert report.source == code
```

`analyze()` returns a `SourceReport`. Its `functions` tuple preserves source
order and duplicate names. `diagnostics` contains recovered parser problems;
an empty function list alone is not proof of successful parsing. `to_dict()`
returns ordinary Python data suitable for JSON serialization. `raw` exposes
the underlying mapping; report wrappers are not immutable snapshots.

`analyze_path(path)` reads UTF-8 with replacement for invalid bytes and preserves
line endings. Offsets are half-open **UTF-8 byte** ranges into `report.source`,
not Python character indices. Slice `report.source.encode()[start:end]`.

Pass `dialect="decompiled"` to normalise supported decompiler spellings, or
`dialect="preprocessed"` for compiler output containing line markers. The latter
can empty an ordinary `.c` input. Offsets after normalisation address the
normalised text, not the original file. `normalize(code, dialect)` exposes the
same transformation. An unknown dialect raises `ValueError`.

`functions(code)` lists definitions without constructing graphs.
`control_flow_graphs(code)` returns general CFG dictionaries. These convenience
APIs, as well as the graph/dataflow APIs below, currently discard parser
diagnostics; inspect an analysis report for the same text when recovery matters.

The comparison-oriented decompiler adapter has a separate, explicit host
preprocessing boundary:

```python
from cindergraph import source_cfg

result = source_cfg.analyze_decompiled("""
#define DEFINE(name) int name(void) { return 1; }
DEFINE(generated)
""")
assert result.preprocessing.status in {"succeeded", "unavailable", "failed", "timed-out"}
if result.preprocessing.succeeded:
    assert result.provenance["generated"].origin == "expansion-generated"
```

`preprocess_decompiled()` can be called independently without importing
NetworkX. Its report records the exact analyzed text, status, selected compiler
and command, captured stderr, and whether include directives were removed.
Statuses are `not-needed`, `succeeded`, `unavailable`, `failed`, and
`timed-out`. Failure remains fail-open for tolerant recovery but is never
silent. `analyze_decompiled()` returns that report alongside GED-ready graphs
and per-function provenance. Provenance keeps `origin` (`source` or
`expansion-generated`) separate from `recovery_qualified`, because a generated
definition may also require parser recovery. The compatibility
`cfgs_from_decompiled()` function still returns only the graph mapping expected
by existing DecBench tooling.

## Reuse one analysis snapshot

```python
session = cg.AnalysisSession(code)
cfgs = session.control_flow_graphs()
flows = session.data_flow()
summaries = session.call_summaries()
assert {item["source_id"] for item in cfgs + flows + summaries} == {
    session.source_id
}
exit_node = cfgs[0]["cfg"]["exit"]
assert session.backward_slice("choose", exit_node, function_id=0)
name, graph = session.export_graphs(repr="pdg", format="json")[0]
assert name == "choose" and '"directed": true' in graph
```

`AnalysisSession` owns one native Rust `AnalysisUnit`. It parses and constructs
CFGs once, then lazily caches dataflow and call summaries. Use it when several
results must share an exact source snapshot; the existing module functions are
still the simpler interface for a single query. Each call returns fresh Python
containers, so modifying a result does not alter the native cache or a later
result. The session covers diagnostics, CFG, dataflow, summaries, backward
slicing and all five graph export representations. Metrics still use their
existing API. A slice rejects duplicate function names, invalid node IDs, and
an optional `function_id` that does not match the named function in this
snapshot.

For decompiler or compiler-preprocessed input, select preparation when creating
the snapshot:

```python
raw = "int f(int x @ eax){return x;}"
decompiled = cg.AnalysisSession(raw, dialect="decompiled")
assert decompiled.dialect == "decompiled"
assert "@ eax" not in decompiled.source
assert decompiled.control_flow_graphs()[0]["input_dialect"] == "decompiled"
```

The default dialect is `"ordinary"`. `source` is the exact prepared text whose
UTF-8 byte offsets all results address, and `diagnostics` is a tuple of the same
typed `Diagnostic` objects used by reports. An unknown dialect raises
`ValueError`; Cindergraph never silently guesses one from the text.

External calls are similarly explicit snapshot policy:

```python
code = "extern int hash(int); int f(int x) { return hash(x); }"
safe = cg.AnalysisSession(code)
assert safe.external_calls == "unknown"
assert not safe.call_summaries()[0]["complete"]

contracted = cg.AnalysisSession(code, external_calls="assume_pure_no_flow")
assert contracted.call_summaries()[0]["complete"]
assert contracted.call_summaries()[0]["flows"] == []
```

`"unknown"` is the default and preserves uncertainty for a named callee whose
body is absent. `"taint_return"` says every actual may influence the return but
keeps the summary incomplete because other effects remain unknown.
`"assume_pure_no_flow"` asserts both no caller-visible effects and a return
independent of all actuals. It can therefore justify negative reachability
answers. Select it only from a real API contract; Cindergraph does not verify
the assumption. Unknown policy names raise `ValueError`. CFG, dataflow, summary,
and typed reachability results carry `external_call_policy`, so persisted output
does not conceal the assumption that produced it.

## Traverse native graphs without NetworkX

```python
graph = session.native_graphs(repr="cfg")[0]
assert graph.name == "choose"
assert graph.source_id == session.source_id
assert graph.node_count == len(graph.nodes())
assert graph.edge_count == len(graph.edge_list())

entry = min(node["id"] for node in graph.nodes())
assert graph.out_degree(entry) == len(graph.successors(entry))
assert set(graph.descendants(entry)) >= set(graph.successors(entry))
```

`native_graphs(code, repr=...)` is the equivalent one-shot call. Both APIs
support `"ast"`, `"cfg"`, `"ddg"`, `"cdg"`, and `"pdg"`. The returned
`NativeGraph` owns read-only Rust topology and adjacency. It carries
`source_id`, `function_id`, `representation`, `graph_kind`,
`analysis_revision`, `input_dialect`, and `external_call_policy`.
`representation` is the requested `"cfg"`/`"pdg"`-style family;
`graph_kind` is the stable coordinate identity, such as `"executable_cfg"`,
and may be passed to APIs that validate graph-local IDs. Unknown node IDs raise
`KeyError`; an unknown representation raises `ValueError`.

`nodes()`, `edges()`, and `edge_list()` cross their result in bulk. Immediate
and transitive topology queries run natively and return deterministic tuples.
This avoids constructing NetworkX objects for ordinary traversal but does not
promise zero-copy Python arrays: requesting node or edge dictionaries still
allocates them.

Call `graph.to_networkx()` only for interoperability. It imports NetworkX
lazily and raises `ImportError` when the `graphs` extra is absent. The default
returns `DiGraph`, which collapses parallel edges with the same endpoints;
pass `multigraph=True` to preserve every edge in a `MultiDiGraph`. The
[native graph design](../architecture/native-graph-api.md) records requirements,
dependency decisions, and the reproducible performance baseline.

Each CFG also has an `indirect_dispatches` list. An entry names the dispatch
node, its successor target IDs, `exact` or `conservative` precision, whether the
expression may be invalid, and stable uncertainty reasons. Direct `&&label`
values and unmodified, unescaped `void *` label tables resolve independently and
report `exact`; an index without a range proof also reports
`index_may_be_out_of_bounds`. Other forms report `conservative` with a reason
and retain every function-local address-taken label.
Dataflow additionally carries `recovery_free`: it is false when any parser or
CFG diagnostic occurred anywhere in the translation unit. Warnings count too.
This conservative unit-wide flag does not localize recovery to a function.
`semantic_issues` exposes the corresponding structured qualifications. Each
entry names its affected analysis dimensions and may carry a byte span; a null
span means the entire function is affected. Unsupported evaluated effects,
memory accesses, and unresolved VLA provenance carry the syntax span that
caused the qualification. Recovery carries each diagnostic origin while its
coverage impact remains conservatively function-wide.

## Dependence and slicing

```python
code = "int f(int x){int y=0;int *p=&y;*p=x;return y;}"
flow = cg.data_flow(code)[0]
assert flow["effects_complete"]
assert flow["memory_complete"]
assert any(d["kind"] == "memory_write" for d in flow["definitions"])
for event in flow["definitions"] + flow["uses"]:
    assert flow["bindings"][event["binding"]]["name"] == event["name"]

cfg = cg.control_flow_graphs(code)[0]["cfg"]
ret = next(node["id"] for node in cfg["nodes"] if node["kind"] == "return")
slice_nodes = cg.backward_slice(code, "f", ret)
assert ret in slice_nodes
```

`data_flow()` returns a list of per-function dictionaries. Definitions and uses
carry binding IDs, CFG node IDs and source spans. Edges join definition/use
indices. `unresolved_uses` and `dead_stores` contain indices into those respective
tables; they are syntactic analysis observations, not compiler error diagnoses.
Those tables use stable analysis order, not source order: projected memory events
can be appended after ordinary events. Order by the byte `start`/`end` spans when
source order matters, and retain the original table indices when joining edges or
the defect lists.
Projected storage is reported separately in `memory_regions`: each entry is a
binding root, named field, or conservative all-element summary linked through
its `base` ID. `memory_accesses` associates reads and writes with a region and
marks the association `exact` or `may_alias`. `memory_definitions` and
`memory_uses` split these events for the region lattice, and `memory_edges`
joins their table indices. Exact writes kill prior writes of the same region;
may-alias writes join without killing. These edges do not make an incomplete
memory result complete by themselves.
`memory_overlaps` names parent/child containment and known union-member
overlap. A cross-union edge carries distinct `definition_region` and
`use_region` IDs plus `overlap="union_members"`; a same-region edge reports a
null overlap. Containment is not yet a destructive transfer rule because a
field write cannot soundly erase the unaffected remainder of its aggregate.
An accessed field of a by-value aggregate parameter receives an
`incoming_parameter` memory definition at function entry. Direct returns from
that region contribute positive parameter-to-return summary provenance; a
definite overwrite kills it. The aggregate base itself is storage identity,
not a scalar read, so it does not appear in the ordinary `uses` table merely
because a field or local-array element was selected.
Passing a known local address or pointer target to a call adds a weak
`call_clobber` definition to its accessed regions; a later strong store kills
it, while a later read otherwise retains both the caller's prior write and the
possible callee write. Such calls keep `memory_complete=False` until callee
effect summaries can discharge the uncertainty. Scalar-only arguments do not
change memory coverage. Local array arguments decay without appearing as
ordinary scalar reads or explicit address-taking definitions.
`effects_complete` reports whether reads, writes and calls were fully lowered
into events. It is false for opaque
`_Generic`, `__builtin_choose_expr` and `__builtin_va_arg` expressions because
the parser retains their tokens but does not establish which value operand is
evaluated. Summary `complete` incorporates this signal, so a missing flow in
such a body is `unknown`, not a proven negative. Type-only
`__builtin_types_compatible_p` and `__builtin_offsetof` do not lower runtime
values and leave this signal true. Evaluated inline assembly also fails this
signal: its opaque operands and clobbers may access locals or arbitrary memory.
A GNU/Clang `cleanup(function)` declaration attribute also fails the signal
because its implicit scope-exit call is not represented. Inert attributes such
as `unused`, `aligned` and `deprecated` do not affect it.
`vla_complete` reports whether variable-length-array value provenance is fully
represented. It is currently false for a VLA typedef whose captured bound
cannot be carried through a later use of the typedef name; interprocedural
summary `complete` incorporates this signal. Direct local and outer parameter
array bounds recover uses of known lexical values; nested function-pointer
signatures do not leak their parameter names or bounds into the outer function.
Array-bound recovery excludes ordinary `sizeof` and `_Alignof` operands while
retaining the evaluated bound in a VLA type such as `sizeof(int[n])`.
An array-bound `_Generic` expression fails `vla_complete`; no association is
reported as a definite read because selecting it requires unresolved C type
compatibility.
Direct local array bounds are expression subtrees. Their `&&`, `||`, and `?:`
operators therefore contribute real CFG arms, and pure guarded computations
carry the corresponding edge identity in the evaluation plan. `&&` and `||`
also carry their explicit zero/one bypass value. Direct assignments and
increments are represented as ordered reads and writes; a guarded increment
affects only its taken CFG arm, so a later read retains both possible reaching
definitions. A guarded call is retained but still makes effect and VLA coverage
incomplete, so it cannot justify a negative result. Calls eliminated by a
literal `&&`, `||`, or conditional arm add neither an event nor uncertainty.
For assignment bounds, plain assignment provenance follows the right-hand
value; compound assignment consumes both the prior target and its computed
right side in evaluation order.
Parameter declarator interiors and type-name bounds remain conservative
token-owned surfaces.

Bindings have dense IDs. `is_unresolved=True` means no local declaration was
recovered; `type` is then `None`. Distinct unresolved spellings have distinct
IDs. Declared types are source spellings, not resolved ABI widths.

Pointer depth and array rank are read from each declarator's own syntax extent;
operators in its initializer or a neighbouring declarator do not contribute.
Parameter groups are split at balanced top-level commas, so names inside a
function-pointer signature are not bindings in the enclosing function, and
each consecutive array suffix contributes to rank. A parameter consisting of
one identifier remains syntactically ambiguous when its typedef declaration is
unavailable: a preceding file-scope typedef is recognized, but a `size_t` from
an unprocessed header may be interpreted as a parameter name.
These remain source-level shapes rather than resolved C types: typedefs,
qualifier placement, function signatures, ABI widths and aggregate layout are
not semantically resolved. File- and block-scope typedef identities are tracked
where lexical value-versus-type identity matters, including a lone name inside
`sizeof`, but their underlying types are not expanded into a general C type
environment.

Named, known non-array scalar or pointer operands of `sizeof` do not produce
value reads, including `sizeof x` and nested parentheses. Compound operands
proven non-array by their outer operator (calls, increments, assignments,
binary and conditional expressions, and supported unary operators) also
contribute no reads, writes or calls. A direct `sizeof(*p)` also avoids value
and memory reads when the resolved local/parameter declaration proves that the
pointee is a built-in scalar or a pointer, with no array declarator. Parentheses
and repeated dereferences such as `sizeof(*(*(p)))` are supported when the
declared pointer depth suffices and the resulting type is known fixed-size. This is not
general unevaluated-expression support: array/member operands, more complex or
unresolved dereferences, and variable-length-array size
dependencies are not recovered reliably. Summary `complete` does not yet
detect these gaps; do not use it as a soundness guarantee for such expressions.

Local pointer targets are approximated from address-taking and pointer copies.
Indirect writes are weak updates: they add alternatives without killing other
possible targets. Compatibility scalar events identify a possible local target
by binding; the region/access tables retain structural field/element identity.
An incoming pointer parameter owns an abstract `parameter_pointee` root;
fields and all-element summaries below it remain caller-owned rather than being
confused with the local pointer object. Copies retain that formal identity, and
distinct formal roots have explicit `parameter_alias` overlap because their
call-time arguments may alias. `memory_complete=False` identifies accesses
that cannot be represented by either a concrete local or an abstract formal
root. A pointer must also be initialized on every CFG
path to a modeled access; a target learned from a later assignment or from
only one branch does not certify completeness. Even a true flag is not a
general C soundness certificate: alias sets are conservative, and
interprocedural memory effects and recovery diagnostics need separate
consideration.

Discarding an expression's result does not discard its side effects. Pointer
assignments in the left operand of a comma expression, such as
`(p = &value, 0)`, therefore contribute targets for later accesses. Unsupported
pointer arithmetic used by such an assignment still makes memory coverage
incomplete.

`control_dependence()` returns controller/controlled edges and post-dominator
information on general CFG IDs. `backward_slice(code, function, node)` follows
data and control dependence and returns sorted IDs including the seed. It
raises `KeyError` for an unknown function, `ValueError` for duplicate definitions
of the requested name, and `IndexError` for an invalid node in a uniquely
identified function. Name ambiguity is checked before graph-local node bounds.
Pass the graph's `source_id`, `function_id`, `graph_kind`, and
`analysis_revision` as keyword arguments when a node came from a separately
retained result; mismatched coordinates are rejected. Check memory coverage
before treating omitted nodes as independent.

## Calls and summaries

```python
code = "int sink(int y){return 0;} int f(int x){sink(x);return 0;}"
assert cg.reaches(code, "f", 0, "sink") == "yes"
assert cg.reaches("int f(int x){return x;} int g(int y){return y;}",
                  "f", 0, "g") == "no"
```

`call_summaries()` reports parameter count, scalar return/output flows,
parameter-relative `memory_effects`, `memory_effects_complete`, and `complete`
per function in source/`function_id` order. Duplicate names remain separate;
no body is overwritten or merged. `reaches()` follows actual call sites and argument positions,
not merely matching returned values. A memory effect has a zero-based formal
`parameter`, `read` or `write` kind, `exact` or `may_alias` precision, and a
path such as `[".field", "[*]"]`. Direct body effects and transitive callee
effects compose to a fixed point; complete effects are then instantiated
on concrete local or inherited formal-pointee regions in each caller. The
caller's memory accesses, definitions, uses, and edges therefore contain known
call effects rather than a generic clobber. `call_memory_arguments` exposes the
actual position, concrete binding targets, inherited formal origins, and
mapping completeness. Unknown, incomplete, indirect, or ambiguously named
callees retain a weak `call_clobber` and fail `memory_complete` closed. Taking
`&parameter` still names callee-local pointer storage rather than its caller's
pointee.

`reaches()` evaluates a named source parameter against a named sink function,
returning `"yes"`, `"no"` or `"unknown"`. Positions are zero-based. Missing
source functions return unknown. An out-of-range parameter returns no only
when the source summary is complete; otherwise its recovered parameter count
is not trusted and the result is unknown. Calls through locally bound function
pointers remain indirect.

Return provenance includes control dependence. Comma expressions contribute
their right operand's value while retaining
left-operand side effects and calls. Merely reading a value on a discarded
left operand does not make the comma result depend on it.

Prefer `query_reaches()` when the reason matters:

```python
answer = cg.query_reaches(code, "f", 0, "sink")
assert answer.claim == "found_may_path"
assert answer.path == (
    cg.ReachabilityStep(function_id=1, function="f", parameter=0),
    cg.ReachabilityStep(function_id=0, function="sink", parameter=0),
)
```

`ReachabilityResult.claim` is `"found_may_path"`, `"no_may_path"`, or
`"unknown"`. A found result carries one deterministic formal-parameter path.
A no-path result has `coverage_complete=True`. An unknown has stable
`uncertainty` reasons and the states reached before the analysis lost coverage.
This is scalar call/return provenance, not yet a value-level or memory-region
explanation.

When names are duplicated, use snapshot identities from CFG, dataflow or
summary results:

```python
duplicate_code = "int f(int x){return g(x);} int f(int x){return 0;} int g(int y){return y;}"
session = cg.AnalysisSession(duplicate_code)
assert session.query_reaches_by_id(0, 0, 2).claim == "found_may_path"
assert session.query_reaches_by_id(1, 0, 2).claim == "no_may_path"
assert session.query_reaches("f", 0, "g").claim == "unknown"
```

`FunctionId` values belong only to the exact `source_id` snapshot that emitted
them. They are dense source-order coordinates, not persistent database IDs.
These are may-dependences: equivalent branches and algebraic cancellation are
not simplified. Incomplete
callees and memory coverage propagate to callers. Any unresolved binding,
including global reads or writes, makes the summary incomplete: global effects
are not yet transferred across calls. This can conservatively include names
that a richer declaration resolver could identify as constants. Local bindings
that shadow globals do not acquire this uncertainty. Duplicate definitions produce
an incomplete summary without merged positive flows. Its parameter count is the
maximum recovered arity, not a selected signature. A reachability query whose
source name has duplicate definitions returns `unknown`, including when source
and sink names are equal. A unique source parameter can reach its own function
by a zero-length path. The fixed point revisits
callers when a callee changes, with a total budget of 16 function evaluations
per input function. It conservatively marks summaries incomplete if that
budget is exhausted before convergence.
Any parser/CFG diagnostic makes summaries incomplete, preserving recovery
uncertainty even when a convenience API discards the diagnostic list. Full
cross-function memory effects remain open work; neither a flow nor
`complete=True` proves semantic equivalence. A witnessed may-flow can still
return `"yes"` despite incompleteness; an absent flow then returns `"unknown"`.

## Export and optional graph adapters

```python
import json

graphs = cg.export_graphs("int answer(void){return 42;}", repr="cfg", format="json")
name, body = graphs[0]
graph = json.loads(body)
assert name == "answer"
assert graph["directed"]
assert "nodes" in graph and "edges" in graph
```

Representations: `cfg`, `ast`, `ddg`, `cdg`, `pdg`. Formats: `dot`, `graphml`,
`json`, `mermaid`. Inspect `EXPORT_REPRS` and `EXPORT_FORMATS` for canonical
choices. Export returns `(function name, text)` pairs in source order, preserving
duplicate names. JSON uses node-link data with an `edges` key and allows parallel
edges. `export_path()` uses the same byte-preserving decoding as `analyze_path()`.
GraphML drops characters forbidden by XML 1.0, including most C0 control
characters and U+FFFE/U+FFFF. Use JSON when preserving those label characters
matters; GraphML labels are not a lossless source representation.
Legal tabs and line endings in graph text are emitted as character references
so XML whitespace normalization does not alter them.

### Export schema

Every representation is a node-link document: `nodes` (each with an integer
`id`, a `label`, and string attributes) and `edges` (each with `source`,
`target`, a `label`, and string attributes). Attribute values are strings in
every format, including numbers and booleans (`"16"`, `"true"`); parse what you
need. Attributes are only ever added between versions; an attribute listed
here keeps its name and meaning.

**Spans are byte offsets.** `span` is `lo:hi`, byte offsets into the analysed
source text (after any dialect normalization), end exclusive. They are not
character indices: slice the UTF-8 *bytes* (`source.encode("utf-8")[lo:hi]`),
not a decoded `str`, or every span after the first multi-byte character is
off. `line` and `column` are 1-based and computed from `lo`; the column counts
bytes from the start of the line, so it agrees with `span` on every input and
with an editor's column only on ASCII lines.

Common to every node of every representation: `span`, `line`, `column`.

`ast` nodes:

- `tag`: the syntax-node kind (`func_def`, `binary_expr`, `name_ref`, ...).
- `op`: on `binary_expr`, `assign_expr`, `unary_expr`, `cond_expr` and
  `inc_dec_suffix`, the operator token as written (`+`, `<<=`, `!`, `++`),
  `?:` for a conditional. A comment between operands is trivia and never
  changes it. A binary or assignment *chain* is one flat node per precedence
  level (`a + b - c` is one `binary_expr` with three operands); it reports its
  first operator in `op` and every operator, comma-separated in source order,
  in `ops`. `ops` appears only on chains with more than one operator.
- `type`: on expression nodes (`name_ref`, `literal`, `paren_expr`,
  `comma_expr`, `unary_expr`, `binary_expr`, `cast_expr`, `cond_expr`,
  `assign_expr`, `postfix_expr`, `sizeof_type`, `alignof_type`,
  `compound_literal`, `stmt_expr`, `builtin_expr`, `label_addr`), the C type
  of the expression after lvalue conversion, integer promotion and the usual
  arithmetic conversions (C17 §6.3.1.1, §6.3.1.8), spelled canonically
  (`unsigned int`, `unsigned char *`, `const char *`, `int[16]`,
  `struct point *`, `char[4]` for a string literal). Where the type cannot be
  established it is `unknown` --- an undeclared name, a call with no visible
  declaration, a typedef from a header that was not included (`size_t`,
  `uint32_t` in an unpreprocessed file), a `_Complex` operand, an enum under
  arithmetic --- never a guess. A pointer or array over an unresolved base
  keeps its shape (`unknown *`, `unknown[16]`). The one platform assumption is
  LP64 widths for the rule "the signed type can represent every value of the
  unsigned type" (`long` with `unsigned int` is `long`); everything else
  follows from rank alone.
- `operand_type`: on `binary_expr` and `assign_expr` whose operator converts
  its operands, the common type the operands are converted to for `op` --- the
  consumer's `int < size_t` compares as `unsigned long` and has `type` `int`.
  For a shift it is the promoted left operand; for a plain `=` the assigned-to
  type; for a compound assignment the type the computation happens in. A
  chain carries `operand_types`, one per operator. `&&` and `||` convert
  nothing and carry no operand type.
- `declarator`: `name`; `type` (the written declared type) when the semantic
  layer resolved the declaration (locals, parameters, typedefs; not a
  function's own declarator or a record member); for an array, `element_type`,
  `array_bound` (`constant`, `runtime`, `incomplete` or `star`) and, for a
  constant bound, `count` (the outermost dimension).
- `param_decl`: `type` (the adjusted parameter type: an array or function
  parameter is a pointer, C17 §6.7.6.3), `pointer_depth`, and `name` when the
  parameter has one.

`cfg` nodes: `kind` (`entry`, `exit`, `stmt`, `cond`, `loop_header`, ...),
`expr_internal` (`true` on a node that exists only because a `&&`, `||` or
`?:` was expanded into control flow: the tests and arms of those operators and
a nested operator's own join inside a larger expression; `false` on the node
the statement, condition or `return` ends at, so collapsing every `true` node
into the node it flows to gives statement-level control flow), and the
`dispatch_*` attributes on an indirect dispatch. `cfg` edges: `kind`, `back`.
`cdg` and `pdg` nodes are the CFG's node set with the same ids and carry
`expr_internal` too, plus `depth`, `ipdom` and `reaches_exit`.

**Ordering.** Determinism is a contract: the same input always serializes to
the same bytes. Beyond that, the AST export promises document order: node ids
are assigned in preorder, so a node's id is smaller than every descendant's
and ids ascend with `span`; a parent's children have ascending, non-overlapping
spans; the `edges` list is grouped by parent in ascending parent id, and within
a parent the edges appear in the children's source order --- so reading an
operator's operands in edge order is reading them left to right. CFG, CDG and
PDG nodes are in construction order (entry first, exit second); CFG edges are
grouped by source node and stable within a group. DDG nodes are the
definitions, then the uses, then memory definitions and uses.

`cg.source_cfg.parity_cfgs(code)` returns the separately defined parity shape.
`cfgs_from_decompiled()` adapts it to NetworkX. Install the optional `graphs`
extra to use NetworkX adapters; ordinary analysis does not require NetworkX.

`cindergraph.compat.pyjoern` provides `fast_cfgs_from_source`, `parse_source`,
and `parse_callgraph`, also exposed through `cindergraph.source`. They accept
paths, warn on parser diagnostics, and support `strict=True`. The compatibility
adapter rejects Joern AST/DDG requests and unsupported timeout/graph modes.
Directory parsing uses `(name, absolute filename)` keys; duplicate definitions
within a file are rejected. Nodes carry identity and entry/exit flags, not JIL.
The name-keyed `parse_callgraph` adapter also rejects duplicate definitions
rather than combining independent bodies' outgoing calls into one node.
`report.call_graph()` rejects the same ambiguity rather than keeping only the
last body. The ordered `report.functions` collection still preserves both
definitions for inspection; `defined_names()` intentionally returns a set.
Selected `.c`/`.h` paths that cannot be statted or read raise filesystem errors,
including dangling symlinks. Directories and non-regular files are skipped.

See [metrics](source-metrics.md) for ranking, feature vectors and comparisons.
