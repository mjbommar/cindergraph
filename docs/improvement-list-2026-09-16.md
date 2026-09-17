# Improvement list from the Axeyum integration, 2026-09-16

What consuming cindergraph as a solver front end taught us in one day. The
consumer is `python/examples/cindergraph_defects/` in the Axeyum repository:
it reads `analyze`, `export_graphs(repr="ast")` and `export_graphs(repr="cfg")`
for eight small C files, lifts each loop-free function into QF_BV with C's
integer semantics, and replays every solver witness under a sanitizer. Every
item below was either a round trip the consumer had to spend, or a gap the
consumer papered over on its own side and should not have. Items are ordered
by how much consumer code they would delete.

## Add

1. **The operator on `binary_expr`, `unary_expr`, `assign_expr` and
   `cond_expr` nodes in the AST export.** Today a node carries `tag`, `label`
   and `span` only; the consumer recovers `+`, `<<=`, `!` from the source bytes
   between two sibling spans. That works because the parser is deterministic,
   and it breaks the moment a comment or a macro sits between operands. An
   `"op"` field is the fix. (`crates/cindergraph/src/csource/export.rs:262`.)
2. **A resolved C type on every expression node.** `semantic/types.rs`
   already knows every declaration's type; the consumer re-implements integer
   promotion and the usual arithmetic conversions to get from there to the
   type of `hdr + body` or `len > 256`. Publish `"type"` on `name_ref`,
   `literal`, `binary_expr`, `cast_expr`, `cond_expr`; it is the same
   information Milestone B's evaluation lowering needs, and a consumer with
   one C-semantics rule wrong is a consumer with a wrong verdict.
3. **A typed-operations export.** The ROADMAP's Milestone B lowers expressions
   to operations with explicit value inputs and control positions. Export that
   as JSON (operation, width, signedness, explicit conversions, owner span). It
   is exactly the input a solver front end wants; the consumer's whole AST
   walk exists because it is not there yet.
4. **Array declarators as structure, not text.** `uint32_t table[16]` comes out
   as a `declarator` whose label is the text; the consumer regexes `name[N]`
   out of it. Export element type and count.
5. **Parameter declarations as fields.** `param_decl` is a label string
   (`"unsigned char *dst"`); the consumer splits type, pointer depth and name
   with a regex. Give it `type`, `pointer_depth`, `name`.
6. **`line` and `column` on exported nodes.** Every consumer re-counts
   newlines. Cheap to add, and it makes spans self-describing.
7. **A consumer contract for external facts.** Pointer capacities
   (`capacity(dst) = dst_len`) are the one thing a solver front end needs that
   the source does not say. The consumer invented a structured comment.
   Either adopt a documented annotation form or provide an API argument for
   attaching facts to a function's parameters.
8. **`__version__` on the Python package.** It is missing; a consumer cannot
   record which cindergraph produced a result.

## Fix

9. **Document that spans are byte offsets.** The consumer indexed the source
   as a Python string; an em dash in a comment shifted every later span by two
   and the lifter reported the operator `ody`. A one-line note in the export
   schema (and item 6) prevents that class of bug for every future consumer.
10. **`parse_source` given source text instead of a path** raises
    `OSError: File name too long` from `pathlib` (`python/cindergraph/compat/pyjoern.py:225`).
    Detect a non-path argument and say so.
11. **Expression-level CFG nodes for `&&`, `||` and `?:`.** The CFG for
    `if (idx <= 16 && u != 0)` has a `cond` for each operand and one for the
    whole; a ternary in an initializer becomes four nodes. Correct for a CFG,
    but a consumer enumerating statement-level paths has to know to collapse
    them, and nothing says which nodes are expression-internal. Mark them, or
    offer a statement-level projection.

## Improve

12. **Loop metadata beyond the back edge.** Back edges are flagged, which is
    enough to refuse loops honestly. To unroll them, a consumer needs the loop
    header's bound expression and whether it is constant; `for_cond` and
    `for_step` are in the AST already, so classifying "bounded by a literal"
    is cheap.
13. **One parse per source.** The consumer calls `analyze`, then
    `export_graphs` twice; if that parses three times, an `AnalysisSession`
    that holds the parse and serves every export is the fix (one exists in
    the API; if it already does this, document it as the way to consume).
14. **Ordering as a contract.** Edge lists came out in document order and
    children sort by span; the consumer relies on both. State them, or a
    future change silently reorders every consumer's operands.
15. **A defect conformance corpus.** The eight defect/fix pairs in the Axeyum
    example are small, annotated, and have known verdicts. Vendoring them
    under `tests/` as "what a semantic consumer must be able to see" would
    catch export regressions that the metrics tests cannot.
16. **Finish Milestone H.** The ROADMAP says Glaurung still compiles an
    embedded copy, so fixes drift. Every item above lands twice until it does
    not.

## Status, 2026-09-17

Fifteen of sixteen done, in four lanes on top of `cfdeacc` (0.1.0). Every
change is additive: no attribute, name or order moved, and the Axeyum
consumer's results table is byte-identical before and after. Commits are on
`main` unless a branch is named.

| # | item | status | where |
|---|---|---|---|
| 1 | `op` on operator nodes | done | `a2c6c4b` (CG-EXPORT) |
| 2 | resolved `type` on expression nodes | done | `a2c6c4b`; `semantic::expr_types` |
| 3 | typed-operations export | done | `38d5b2f`, `e0c2f27` (CG-OPS): `repr="ops"` |
| 4 | array declarators as structure | done | `a2c6c4b`: `element_type`, `array_bound`, `count` |
| 5 | parameter declarations as fields | done | `a2c6c4b`: `type`, `pointer_depth`, `name` |
| 6 | `line` and `column` on every node | done | `a2c6c4b` |
| 7 | a consumer contract for external facts | done | branch `cg-facts-2026-09-17` (CG-FACTS), `b1c5376`: both forms, one grammar (`docs/design/external-facts-2026-09-17.md`). `// @cindergraph capacity(dst) = dst_len` / `strlen(s) = n` / `unroll = 8` comments above the function (`// axeyum:` is an alias) and `AnalysisSession(..., facts={...})` (also `export_graphs`, `export_path`, `native_graphs`) land as `facts`/`facts_source` on `param_decl`, `func_def` and the parameter's `ops` loads and stores; the API wins over a comment; every fact that cannot be attached is a `Diagnostic`. `csource::facts`, `test_external_facts.py` |
| 8 | `__version__` | done | `00a0e72` |
| 9 | spans are byte offsets, documented | done | `670530e` ("Export schema") |
| 10 | source text handed to a path parameter | done | `00a0e72`: `ValueError` naming the parameter |
| 11 | expression-internal CFG nodes marked | done | `948df1c`: `expr_internal` on CFG, CDG, PDG nodes |
| 12 | loop metadata beyond the back edge | done | branch `cg-finish-2026-09-17` (CG-FINISH): `loop_kind`, `bound_kind`, `bound_expr`, `induction`, `step`, `init_value`, `bound_value` on loop statements and `loop_header` nodes; `export::loops` |
| 13 | one parse per source | done | `cg-finish-2026-09-17`: measured (four parser entries through the free functions, one through `AnalysisSession`), documented under "Reuse one analysis snapshot", byte-identity pinned by `test_session_single_parse.py` |
| 14 | ordering as a contract | done | `670530e` ("Ordering") |
| 15 | a defect conformance corpus | done | `cg-finish-2026-09-17`: `tests/fixtures/defects/` (twelve vendored samples + one loop sample) and `test_defect_conformance.py` |
| 16 | finish Milestone H | **open** | this is Milestone H of `docs/ROADMAP.md` (section 12, "Glaurung migration"), tracked there, not here |
