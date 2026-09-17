# Changelog

All notable changes to Cindergraph will be documented here. The project uses
[Semantic Versioning](https://semver.org/) for released versions; pre-1.0 minor
versions may change APIs and serialized schemas.

## Unreleased

The DecBench parity projection receives the four corrections Glaurung made to
its embedded copy on 2026-09-13 and that the crate never had (found by
Glaurung's migration to the crate, its `source-001`, 2026-09-17; the port
record is `docs/benchmarks/glaurung-parity-corrections-2026-09-17.md`):

- `parity_chains` reads degree from deduplicated successor sets, so an empty
  `if` arm's parallel true and false edges no longer keep the condition block
  from contracting (Glaurung `828a41a9`);
- a syntactically constant-true loop (`while (1)`, `do … while (1)`,
  `for (…; 1; …)`; a nonzero integer literal through transparent parentheses)
  loses its infeasible false exit and keeps its header and cycle
  (`0266715f`). A clause-less `for (;;)` stays with `elide_empty_for_headers`;
  the two rules never both fire, and a loop with a reachable `break` projects
  to the same bytes under either spelling;
- a bare literal `if` test (`if (0)`, `if (1)`) costs no node; its fork moves
  to the predecessor and no arm is chosen (`68b39f18`);
- the duplicate loop header around a ternary in a loop test
  (`while (i < (x ? 14 : 8))`) is collapsed; materializing arms keep their
  expression nodes (`f9a5cbaa`).

`GranularityStats` gains `constant_loop_exits_elided`,
`literal_if_tests_elided` and `ternary_loop_branches_collapsed`;
`parity::nodes::expression_granular` takes the source text as its second
argument. The projection's bytes over the 930 functions of the recorded Joern
comparison are unchanged (no fixture has any of the four shapes); the
corrections are pinned by the ten tests carried from Glaurung plus one, and by
`tools/mutation_controls_parity.py`, under which reverting each correction
kills exactly its tests.

Additions to the graph export and the Python package, from the first day of
consuming Cindergraph as a solver front end
(`docs/improvement-list-2026-09-16.md`). Every change is additive: no existing
attribute, name or order changes, and the parity projection is untouched
except as described above.

- AST nodes carry the operator as written (`op`; `ops` on a flat chain) on
  `binary_expr`, `assign_expr`, `unary_expr`, `cond_expr` and
  `inc_dec_suffix`, read from the token gap so a comment between operands
  cannot corrupt it;
- expression nodes carry `type`, the C type after lvalue conversion, integer
  promotion and the usual arithmetic conversions (C17 §6.3.1.1, §6.3.1.8),
  computed by a new `semantic::expr_types` module from the structural types
  the semantic layer owns; `binary_expr` and `assign_expr` also carry
  `operand_type` (`operand_types` on a chain), the common type the operands
  convert to. Every rule either applies exactly or yields `unknown` --- an
  undeclared name, a call with no visible declaration, a header typedef that
  was never included, an enum under arithmetic --- and a pointer or array over
  an unresolved base keeps its shape (`unknown *`). The one platform
  assumption, LP64 widths for the signed-wider-than-unsigned rule, is stated
  in the module and the reference;
- `declarator` nodes carry `name`, `type`, and for arrays `element_type`,
  `array_bound` and `count`; `param_decl` nodes carry `type` (after array and
  function adjustment), `pointer_depth` and `name`;
- every AST, CFG, DDG, CDG and PDG node carries 1-based `line` and `column`
  (a byte column) computed from its byte span, and the reference now states
  that spans are byte offsets, not character indices;
- `FunctionCfg::expression_internal` lists the CFG nodes that exist only
  because a `&&`, `||` or `?:` was expanded, recorded by the emitter that
  expands them; CFG, CDG and PDG exports write `expr_internal` on every node
  so a consumer can collapse to statement-level control flow;
- `export_graphs(repr="ops")` (item 3): each function's evaluation lowering
  (`csource::eval::EvaluationPlan`, Milestone B) as a list of typed
  operations in evaluation order --- `const`, `load`, `store`, `binary`,
  `compare`, `unary`, `select`, `call`, `branch`, `return`, `sequence`,
  `bound`, `convert`, `unknown` --- each with its result C type, value
  `inputs`, CFG `block`, `root`, `guarded_by`, and byte span, plus `name`
  and `declared` on loads and stores. Types come from the same
  `semantic::expr_types` resolver the AST export reads, joined by span, and
  C's implicit conversions are written out as `convert` operations
  (`promotion`, `usual_arithmetic`, `assignment`, with `from` and `to`):
  `unsigned short s; s += 2` shows the promotion to `int`, the add in `int`
  and the assignment conversion back. Every root the lowering declines is an
  `unknown` operation carrying its `reason` (`unsupported_form`,
  `unsequenced_effects`, `braced_initializer`, `unplaced`), never a gap; a
  cast is such a root today, so `cast` conversions cannot appear yet. The
  sizing behind the design is
  [`docs/typed-operations-export-sizing-2026-09-16.md`](docs/typed-operations-export-sizing-2026-09-16.md).
  Underneath, `EvaluationPlan` now records its declined roots and
  `ExpressionTypes` keeps the result type of every prefix of a flat binary
  chain;
- `cindergraph.__version__`, read from the extension's crate version, the
  same string the wheel's metadata carries;
- `parse_source`, `fast_cfgs_from_source` and `parse_callgraph` raise a
  `ValueError` naming the parameter when handed C source text instead of a
  path, where pathlib used to raise `OSError: File name too long`;
- the export's ordering is documented as a contract (AST ids in preorder and
  ascending with span, edges grouped by parent with children in source order;
  CFG nodes entry-first, edges grouped by source) and pinned by tests in Rust
  and Python;
- loop metadata for a bounded unroller (item 12): `for_stmt`, `while_stmt`
  and `do_while_stmt` AST nodes, and the CFG `loop_header` (and its CDG/PDG
  twins) they become, carry `loop_kind`, `bound_kind` (`constant`,
  `parameter`, `runtime`, `none`), `bound_expr`, `induction`, `step` (`+1`,
  `-1`, `+N`), `init_value` and, when constant, `bound_value`. `constant`
  means the header fixes the trip count by itself (literal initializer, one
  relational comparison of the induction variable against a literal, a
  `++`/`--`/`+= literal` step, no other write to the variable); `parameter`
  is the same shape against a function parameter the loop never assigns;
  everything undecided is `runtime` --- every `while` and `do` included,
  since nothing fixes their start value --- never a guess. The classifier is
  `csource::export::loops`;
- one parse per source (item 13) is documented as the way to consume:
  `AnalysisSession` parses once and serves diagnostics and every export from
  that parse, byte-identical to the module-level functions, which each parse
  again (measured: four parser entries for `analyze` + three exports through
  the free functions, one through a session). Pinned by
  `python/tests/test_session_single_parse.py`;
- a defect conformance corpus (item 15): `tests/fixtures/defects/` vendors
  the twelve annotated defect/fix samples Axeyum's solver front end consumes,
  plus one loop sample, and `python/tests/test_defect_conformance.py` asserts
  for every function what a semantic consumer must see --- `op` on every
  operator node, a resolved `type` on every reference to a parameter or
  local, every `if` condition as a statement-level CFG `cond`, every loop
  header classified, every `ops` operation typed. `FunctionResolution` can
  now say whether a declaration is a parameter;
- a consumer contract for external facts (item 7), decided in
  [`docs/design/external-facts-2026-09-17.md`](docs/design/external-facts-2026-09-17.md):
  one grammar, two front doors. `// @cindergraph capacity(dst) = dst_len`,
  `strlen(s) = n` and `unroll = 8` line comments above a function (the
  consumer's `// axeyum:` marker is an alias, so the vendored samples work
  unchanged), or `AnalysisSession(code, facts={"f": {"capacity": {"dst":
  "dst_len"}, "unroll": 8}})` (also `export_graphs`, `export_path`,
  `native_graphs`). Both land as `facts` and `facts_source` (`comment` or
  `api`, one entry per fact) on `param_decl` and `func_def` in the `ast`
  export and on the parameter's `load`/`store` nodes in `ops`; the API wins
  over a comment for the same key. Every fact that cannot be attached --- a
  parameter the function lacks, a value that is neither a parameter nor a
  decimal literal, a non-pointer capacity target, a comment no function
  follows, a function name defined zero or two times --- is a `Diagnostic`
  on the session (the one-shot functions raise `ValueError` for an API fact,
  having no diagnostics channel), never a silent drop. The resolver is
  `csource::facts`; `AnalysisUnit::with_facts` and `AnalysisUnit::facts`
  are the Rust surface.

## 0.1.0 — 2026-09-15

Initial standalone extraction from Glaurung:

- tolerant parsing of ordinary and decompiler-shaped C with diagnostics;
- function metrics, general control-flow graphs and graph serialization;
- reaching definitions, dependence graphs, backward slicing and bounded call
  summaries with explicit recovery/memory completeness signals;
- an owning Rust/Python analysis session that reuses one parse, CFG set,
  dataflow fixed point and summary set across queries, slicing and export;
- identity-preserving summaries and typed reachability claims with exact
  function IDs, may-path evidence, complete-negative coverage and explicit
  uncertainty reasons;
- abstract caller-owned regions for incoming pointer parameters, including
  copy-stable formal identity, explicit cross-formal may-alias relationships,
  and direct parameter-relative read/write effects in Rust and Python function
  summaries without conflating pointer address provenance with loaded values;
  complete pointee effects compose transitively through known direct callees
  and instantiate as caller-region reads and weak writes, while incomplete,
  indirect, external-unknown, and ambiguous calls retain conservative clobbers;
- explicit ordinary, preprocessed and decompiler input options applied before
  snapshot identity, with the prepared source and diagnostics retained;
- explicit external-callee policies on Rust and Python analysis sessions: the
  default preserves uncertainty, taint-return propagates conservative value
  influence without claiming completeness, and an opt-in pure/no-flow contract
  is recorded on results that may use it to justify negative answers;
- read-only Rust-backed Python graph views for AST, CFG, DDG, CDG, and PDG,
  with snapshot identity, bulk topology, compact forward/reverse adjacency,
  native traversal, preserved parallel-edge multiplicity, and explicit
  optional NetworkX conversion;
- a dependency-free robustness benchmark contract that validates immutable
  source identities, generated-case replay metadata, non-overlapping specimen
  slices, canonical manifest hashes, and exactly one explicit success or
  failure result per tool/specimen without denominator shrinkage;
- a reproducible 525-specimen clean, decompiler, random-damage, and controlled
  locality manifest plus a Cindergraph adapter that retains native evidence, validates
  exported graph integrity, reports recovery completeness conservatively, and
  isolates each specimen behind timeout, signal, and optional memory limits;
- fixed-denominator robustness summaries that revalidate every result set and
  keep unavailable damaged-input oracles distinct from successful recovery;
- a dependency-free Clang robustness adapter that retains raw and normalized
  partial AST evidence on ordinary compiler-error exits while leaving
  unavailable CFG and resource claims explicitly unset;
- a separately locked Tree-sitter C robustness worker and fail-closed
  supervisor, isolated from the Cindergraph crate and wheel dependency graphs;
- conservative lexical resynchronization after an unterminated block comment:
  the error remains coverage-visible, while a later top-level-function-shaped
  line can recover an unaffected neighbour;
- grammar-owned local array-bound expressions that expose conditional and
  short-circuit control to the CFG, resolve evaluation guards to concrete arm
  edges, materialize language-defined short-circuit bypass values, preserve
  conditional scalar writes on only their taken CFG arms, and prevent
  duplicate legacy dataflow events; bound assignments explicitly distinguish
  the prior target value from the assigned RHS and preserve computed-RHS
  provenance; evaluation identity is distinct from captured-bound identity,
  and supported pure scalar initializer and return roots retain explicit
  purposes, CFG ownership, and topological result inputs; dataflow consumes
  those roots for exact read/write placement and initializer visibility,
  replaces legacy promoted writes, and distinguishes direct evaluation inputs
  from transitive provenance so joins do not duplicate source reads; supported
  call-bearing roots derive call records and direct-return identity from
  operation/value edges, represent sequenced, indeterminately sequenced, and
  mutually exclusive call execution, and preserve existing intrinsic,
  defined-callee, and external-call policies; dependency-ordered call/write
  expressions and mutually exclusive conditional call/write arms now share
  this path without admitting unordered mixtures; recursive scalar reads are
  explicit producers, and unsupported same-object unsequenced conflicts emit a
  localized `unsequenced_access` coverage issue; comma expressions use one
  sequence operation across bounds, initializers, and returns, retaining
  discarded effects without adding discarded values to result provenance;
  supported expression statements now use the same operation path with an
  explicit discarded-result purpose; supported `if`, loop, and `switch`
  conditions likewise use explicit control purposes and operation-owned
  reads, writes, and calls, with `for` selecting its condition independently
  of its initializer and step; expression-form `for` initialization and step
  clauses have distinct operation-owned lifecycle phases while declaration
  initialization retains its declaration owner; evaluation plans are now
  lazily cached so AST and CFG-only consumers do not lower unused operations;
  GNU computed-`goto` operands have explicit operation ownership without
  misclassifying the statement's leading `*` marker as a memory load;
- stable scalar `PlaceId`s derived from resolved declaration identity, used by
  operation operands and dataflow binding lookup so source spans are no longer
  the semantic identity of modeled scalar storage;
- direct scalar address formation lowered through the shared evaluation plan as
  `AddressOf(PlaceId)`, without falsely reading the object's stored value or
  duplicating the legacy address event;
- scalar dereferences represented as explicit `LoadScalar` operations over
  pointer values; direct pointer loads now drive conservative points-to and
  pointee-use projection from the operation record without duplicate syntax
  discovery, while unsupported and VLA-bound loads continue to fail closed;
- operation assembly now preserves producer-before-consumer lowering order for
  nested expressions instead of re-sorting overlapping operations by source
  span; computed pointer loads follow conditional and comma results through
  `ValueId`, retain arithmetic may-targets with explicit uncertainty, and own
  their parenthesized syntax without duplicate fallback discovery;
- indirect stores now resolve their address through the same operation-derived
  pointer constraints: conditional targets are unioned, comma targets use only
  the result operand, and pointer arithmetic preserves known may-writes while
  qualifying memory coverage; sequence-effect predecessors also keep comma
  stores from being rejected as falsely unsequenced;
- store constraints now retain pointer sources for the assigned `ValueId` as
  well as the address, replacing RHS interval scans for supported second-order
  writes; exact local address/copy/conditional values update pointer targets
  without false incompleteness, while transformed values preserve may-targets
  and propagate explicit unknown state; assigned provenance is evaluated lazily
  only for stores that may actually update pointer objects;
- plain indirect assignments represented as `StoreScalar` operations with
  conservative alias-aware ordering checks; direct pointer stores now select
  projection targets from the operation's pointer occurrence and emit each
  memory write once, while transparent `*&place` stores canonicalize to direct
  writes without a false address escape;
- pointer-valued scalar initializers and assignments now generate address,
  copy, load, and unknown constraints from `ValueId` producer edges; points-to
  analysis maps their stable places to bindings instead of rescanning RHS
  source ranges, retaining may-targets and explicit uncertainty through
  arithmetic and unary transformations;
- evaluation operations now attach a common semantic place: direct scalar
  operations name declaration-backed places, while loads and stores name
  interned projected dereferences keyed by their address `ValueId`; source
  spans remain diagnostics rather than memory-object identity;
- pointer-member and indexed scalar reads now lower as `LoadScalar` operations
  with field and element projected places keyed by their base/index `ValueId`s;
  these accesses retain explicit evaluation dependencies while remaining
  fail-closed until layout and target projection are available;
- plain assignments to pointer members and indexed elements now lower as
  `StoreScalar` operations over the same projected places, with explicit base,
  index, and assigned-value dependencies; memory projection remains
  fail-closed rather than fabricating unresolved field or element targets;
- projected-place bases distinguish evaluated pointer values from direct
  object places; simple `object.member` reads and plain writes now share the
  aggregate object's `PlaceId` without inventing a whole-object scalar read,
  while unresolved layout remains explicitly incomplete;
- projected places can themselves base a subsequent field projection, so
  nested direct and mixed chains such as `object.inner.field` and
  `pointer->inner.field` retain inside-out place identity without loading an
  aggregate intermediate;
- element lowering now uses structural declaration types to distinguish local
  array storage from adjusted array-parameter pointers; local `array[index]`
  reads and writes project from the array `PlaceId` without a fictitious
  whole-array scalar read;
- struct and union bodies now preserve grammar-owned member declarations whose
  declarators feed stable record/member types; record-tag bindings respect
  enclosing block/record extents, and array-valued fields consequently
  decay from a nested field place for both `object.field[i]` and
  `pointer->field[i]`, without a fictitious aggregate load;
- structural element types now propagate through every index in
  multidimensional local, parameter, and record-member arrays; intermediate
  rows remain nested element places and only the terminal scalar is loaded;
- public dataflow results now expose parent-linked memory regions and
  operation-owned memory accesses; direct fields share structural identity
  with fields reached through known local pointers, while array elements use
  an explicit may-alias summary rather than pretending to be scalar bindings
  or distinct offsets;
- a separate region reaching-definition lattice now publishes memory
  definitions, uses, and edges; exact writes are strong updates, may-alias
  writes are weak updates, branch alternatives join, and loop back-edges
  converge without conflating projected storage with scalar variables;
- DDG and PDG export plus backward slicing now consume region flow edges;
  DDGs retain memory definition/use nodes and structural region paths, while
  PDGs lift those dependences to the common executable-CFG node set;
- region results now expose ancestor/descendant containment and structural
  union-member overlap; cross-member union writes reach reads and exact later
  member writes kill sibling definitions, while ordinary struct members stay
  disjoint and containment remains non-destructive pending aggregate
  fragmentation;
- accessed subregions of by-value aggregate parameters now receive explicit
  incoming definitions and contribute positive return provenance until killed
  by a strong write; evaluation-owned aggregate/array bases no longer leak
  into the legacy scalar-use table as fictitious whole-object reads;
- pointer-like call arguments now add weak call-clobber definitions for known
  local scalar and projected targets while retaining memory uncertainty;
  scalar-only calls remain memory-neutral, and local array arguments use an
  explicit decay operation rather than fake scalar-read or address-taking
  events;
- compound assignments and prefix/postfix increments over dereferences,
  fields, and elements now evaluate their place inputs once, explicitly load
  the prior value, compute the replacement, and store once; projected stores
  retain pre-write/post-write result semantics so postfix expressions yield
  the old value without losing the write;
- transparent `*&place` updates now share the direct-place path for plain and
  compound assignment and prefix/postfix increment, preserving `PlaceId`
  identity and avoiding false address-taken or indirect-memory facts;
- complete postfix-call enumeration and accurate direct-return classification,
  including fail-closed summaries for chained indirect calls such as
  `factory()()`; the inherited, permanently empty `CallRecord.results` and
  `CallSite.results` placeholders are removed rather than advertised as data;
- parenthesized calls to functions defined in the same translation unit retain
  direct-callee identity, while typedef-ambiguous external names remain
  conservatively indirect;
- a distinct `effects_complete` signal prevents opaque `_Generic`,
  `__builtin_choose_expr` and `__builtin_va_arg` expressions from producing
  falsely conclusive negative flow summaries, and likewise fails closed for
  inline assembly operands and clobbers and implicit `cleanup(function)`
  attribute calls;
- a narrow parity CFG for offline comparison fixtures;
- a pure Rust crate and a typed Maturin/PyO3 Python package;
- ABI3 packaging for CPython 3.12 and newer, subject to the tested platform
  matrix documented before release.
- manylinux 2.17 release targeting, deterministic wheel packaging, structural
  distribution/SBOM validation and fail-closed partial-publication recovery;
- release dispatches that build candidates without publishing; only a pushed
  `v*` tag can enter the protected registry jobs;
- workflow security checks with non-persistent checkout credentials, disabled
  release caches, serialized non-cancelling releases and a pinned Zizmor audit;
- strict vulnerability auditing of every hash-pinned dependency reachable
  through a published Python extra;
- fail-closed PyPI artifact gathering that rejects missing producers, duplicate
  filenames, mixed versions, producer/platform mismatches, internal wheel-tag
  mismatches and unexpected files before upload;
- isolated installed-wheel type checking that verifies representative public
  core and optional NetworkX APIs resolve to concrete downstream types rather
  than `Any`;
- one purposeful published extra, `graphs`; test tooling remains a development
  dependency instead of advertising an extra without a shipped test suite;
- PyO3 0.29 with locked advisory, licence, duplicate-dependency and source
  policy checks.
- maintained extraction and Glaurung-migration documentation that separates the
  copied baseline, standalone repairs and deliberately excluded integrations;
- source distributions that carry the extraction record and architecture
  documentation alongside the licence and release material.
- schema-validated Python project metadata and registry-validated Trove
  classifiers, including CPython and typed-package discovery signals, with the
  complete registry-facing contract rechecked inside both wheels and sdists.
- release-time validation and testing of the exact packaged Rust crate,
  including archive safety, normalized metadata and registry-only dependencies.
- end-to-end crate provenance from the reviewed workflow artifact through a
  byte-identical publication rebuild and post-upload crates.io checksum check.
- isolated installation and runtime smoke coverage for the optional `graphs`
  extra from both native wheels and the source distribution.
- explicit trusted-publisher attestations and post-upload PyPI reconciliation
  of every reviewed filename and SHA-256 digest.
- fail-closed PyPI rerun preflight that resumes only an exact partial subset,
  skips a complete identical release and rejects conflicting immutable state.
- pinned PEP 740 verification of every published artifact's signature and
  Trusted Publisher repository identity through PyPI's Integrity API.
- a 2 MiB compressed Rust-crate budget and explicit first-release bootstrap to
  crates.io OIDC trusted-publishing migration procedure.

Known limitations are maintained in the
[Python reference](docs/reference/source-python.md) and
[support matrix](docs/support-and-evidence.md). This entry must lose the
“unreleased” marker only after the release commit and artifact set are fixed.
