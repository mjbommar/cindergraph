# QA iteration 42: cross-process hash-seed determinism

Repeated calls within one Python process cannot expose dependence on its hash
seed. Added `test_process_determinism.py`, which launches independent processes
with PYTHONHASHSEED 0, 1 and 42. Each process analyses all 210 checked-in corpus
files and hashes report dictionaries, dataflow, call summaries, control
dependence, and AST/CFG/DDG/CDG/PDG exports in JSON, GraphML, DOT and Mermaid.

Payloads are length-prefixed and JSON-encoded without sorting dictionary keys,
so serialization/insertion-order changes remain observable. A separate builtin
hash probe must differ across the three workers, verifying that the environment
actually affected hash randomization. Workers have a 30-second timeout each.
They run outside the repository cwd but explicitly load the checkout package;
this is not an installed-wheel isolation test.

Command:

```bash
TMPDIR=/home/mjbommar/.cache/cindergraph uv run --no-sync pytest python/tests/test_process_determinism.py -q -s
```

All three produced the same digest:
`32c92e46b04cb50d34aed0b9b8206d46f8fc6b8b9a16f45acb35d1950d034d29`.
The hash probes differed. The digest is recorded evidence, not a golden hash
that freezes legitimate future analysis changes; the test checks equality
between workers using the same current implementation.

Validation on `edf2777` plus pending QA work, using iteration 41's release
extension:

- Focused cross-process test: passed (7.37 seconds observed locally).
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,567 passed, ten skipped, three optional Joern cases deselected.
- Ruff check/format and ty check on the new test, plus diff whitespace: passed.

No new engine defect surfaced. No Rust changes, rebuild, Rust-suite rerun,
cross-platform determinism claim, performance comparison, publication, commit,
external evaluator or remote CI. Changes local/uncommitted; broader goal active.
