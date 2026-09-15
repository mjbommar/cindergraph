# QA iteration 34: unambiguous comparison populations and metric columns

Comparison review found two loss/counting hazards. Dict construction selected
the last function of each duplicate name, so name-based matching silently
discarded bodies. Separately, a repeated requested metric added its value twice
to totals while the per-function mapping retained one column. Neither could be
detected reliably from the resulting mapping alone.

`compare` now raises ValueError for duplicate function names in either report,
with the side and names in the message, and for repeated metric columns. This
intentionally changes the documented pre-alpha last-definition-wins behavior.
The caller must resolve ambiguous function identities; the library does not
invent a correspondence or silently change the scored population. Unique-name
matching, matched-only totals, and empty metric selection are unchanged.

Six new cases cover both report sides and duplicate-definition orders, repeated
metrics, and empty selection. Five failed before the fix and all pass afterward.
Updated the API docstring and metrics reference; searched the live documentation
for the superseded last-definition behavior.

Validation on `edf2777` plus pending QA work:

```bash
uv run --no-sync pytest python/tests/test_comparison_identity.py -q
uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q
uv run --no-sync ruff check python/cindergraph/source.py python/tests/test_comparison_identity.py
uv run --no-sync ty check python/
git diff --check
```

Six focused tests passed. Full suite: 2,338 passed, ten skipped, three optional
Joern cases deselected. Ruff formatting was applied to the changed facade;
lint, full Python type checking and whitespace checks passed. No Rust change
or Rust-suite rerun; extension remains iteration 30's release build. No runtime
C semantic claim, benchmark comparison, publication, commit, external evaluator
or remote CI. Changes local/uncommitted; broader goal active.
