# QA iteration 31: seeded malformed-source API and serialization checks

Added `python/tests/test_mutated_source_totality.py`. It mutates actual checked-in
C fixtures rather than inventing analysis output. A pinned 211-file manifest
combines the 210 bundled corpus files and the compiler-checked sizeof fixture.
Seeds 0–255 cycle through that sorted population. Each input is capped at 8,192
characters, receives four deterministic replacement/deletion operations, and
every third seed is additionally truncated. Insertions include NUL, U+FFFE,
Greek text, CRLF, quotes, comment openers, punctuation and a preprocessor line.

Each case checks repeated analysis reports, dataflow, summaries, control
dependence and CFG results for equality. All five graph representations are
exported twice to JSON and checked for exact repeatability, unique node IDs and
valid edge endpoints. GraphML is independently XML-parsed, with graph-count
agreement against JSON. XML/JSON label equality is deliberately not asserted:
XML-illegal characters have documented lossy handling. This is a recovery and
serialization check, not a semantic oracle or proof of parser totality.

Validation:

```bash
timeout 60s uv run --no-sync pytest python/tests/test_mutated_source_totality.py -q --tb=short
uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q
uv run --no-sync ruff check python/tests/test_mutated_source_totality.py
uv run --no-sync ruff format --check python/tests/test_mutated_source_totality.py
uv run --no-sync ty check python/tests/test_mutated_source_totality.py
git diff --check
```

Focused suite: 257 passed (256 seeds plus corpus-presence guard). Full suite:
2,327 passed, ten skipped, three optional Joern cases deselected. Ruff and ty
passed after separating the report conversion from heterogeneous API results
so the type checker can resolve `to_dict`. No production code change or engine
failure was found. Rust was not rebuilt or retested; these tests used the release
extension from iteration 30, on `edf2777` plus pending QA changes.

Coverage inspection found 228 mutations retaining functions and 245 yielding
diagnostics, using:

```python
import runpy
import cindergraph as cg
m = runpy.run_path('python/tests/test_mutated_source_totality.py')
reports = [cg.analyze(m['mutated_source'](seed)) for seed in range(256)]
print(sum(bool(r.functions) for r in reports))
print(sum(bool(r.diagnostics) for r in reports))
```

No performance comparison, publication, commit, external evaluator or remote
CI. The batch is finite and bounded; deeper grammar-specific mutation and VLA
semantics remain useful next work. Goal remains active.
