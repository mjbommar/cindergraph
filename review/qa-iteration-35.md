# QA iteration 35: comparison reversal and population invariants

Added a corpus-wide report-algebra check after tightening comparison identity.
Each of 210 checked-in C fixtures is compared with its first three quarters
(by Python character offset), including any recovered partial final function.
The test then reverses the comparison. This is not a semantics-preserving
transformation or a C correctness oracle: it deliberately changes the measured
population and sometimes the recovered metrics.

Assertions check unique input names, exact matching population, added/removed
set symmetry, identical matched ordering after reversal, swapped per-function
values, negated deltas, and totals independently summed from the matched source
reports. Thus a symmetric but wrong total that includes removed functions
cannot pass just by reversing cleanly. A presence guard requires all 210 files.

Coverage census: 147 pairs had nonzero matched deltas, 133 had removals, and
682 functions were matched in total. Reproduce from the repository root:

```python
import runpy
import cindergraph as cg
m = runpy.run_path('python/tests/test_comparison_corpus.py')
changed = removed = matched = 0
for path in m['FILES']:
    code = path.read_text(encoding='utf-8')
    result = cg.compare(cg.analyze(code), cg.analyze(code[:3*len(code)//4]))
    changed += any(v != 0 for row in result['matched'] for v in row['deltas'].values())
    removed += bool(result['removed'])
    matched += len(result['matched'])
print(len(m['FILES']), changed, removed, matched)
```

Validation on `edf2777` plus pending QA work, using iteration 30's release
extension (no Rust implementation changed this turn):

- `uv run --no-sync pytest python/tests/test_comparison_corpus.py -q`: 211 passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,549 passed, ten skipped, three optional Joern cases deselected.
- Ruff check/format and ty check on the added test; diff whitespace check:
  passed.

No implementation defect surfaced in this batch. The improvement is executable
coverage of matched-only totals and reversal across real recovered reports.
No Rust-suite rerun, performance claim, publication, commit, external evaluator
or remote CI. Changes local/uncommitted; broader goal remains active.
