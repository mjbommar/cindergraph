# QA iteration 44: report call-graph identity

After fixing the file adapter, checked the separate SourceReport.call_graph
helper. Its dictionary comprehension silently kept only the last duplicate
definition, dropping the other body's callees. Two definition-order cases
reproduced the issue before the fix.

The helper now raises ValueError with sorted duplicate names before constructing
the name-keyed mapping. The source report itself still preserves both recovered
definitions in its ordered functions tuple; defined_names intentionally remains
a set. Updated the docstring and Python reference with that distinction.

Validation on `edf2777` plus pending QA changes:

```bash
uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q
uv run --no-sync ruff check python/cindergraph/source.py python/tests/test_query_identity.py
uv run --no-sync ty check python/
git diff --check
```

Full suite: 2,579 passed, ten skipped, three optional Joern cases deselected.
Ruff formatting applied; lint, full Python types and whitespace gates passed.
No Rust change, rebuild or Rust-suite rerun; iteration 41's release extension
was used. No performance claim, publication, commit, external evaluator or
remote CI. Changes local/uncommitted; broader goal remains active.
