# QA iteration 39: call-graph definition identity

Call-graph inspection found that parse_callgraph added edges from every recovered
body into a name-keyed NetworkX graph. Duplicate f definitions calling different
targets therefore became one f node with both outgoing edges. The sibling
parse_source adapter already refused ambiguous definition identity.

Factored its uniqueness check into a shared helper and applied it before
constructing a call graph. The error names the file and sorted duplicate names
and directs callers to analyze_path for recovery inspection. Duplicate requested
definitions now raise ValueError; a prototype plus one body is not a duplicate.
The parity fast-CFG adapter's separately documented selection policy is unchanged.

Two source-order variants failed before the fix and now pass. A prototype,
recursive call and named external callee control remains valid. The focused
query-identity file now passes 20 cases.

Validation on `edf2777` plus pending QA changes:

```bash
uv run --no-sync pytest python/tests/test_query_identity.py -q
uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q
uv run --no-sync ruff check python/cindergraph/compat/pyjoern.py python/tests/test_query_identity.py
uv run --no-sync ty check python/
git diff --check
```

Full suite: 2,563 passed, ten skipped, three optional Joern cases deselected.
Ruff formatting applied; lint, full Python types and whitespace gates passed.
Updated the adapter docstring and reference. No Rust change or Rust-suite rerun;
the existing release extension was used. No performance claim, commit,
publication, remote CI or external evaluator. Changes local/uncommitted; the
broader goal remains active.
