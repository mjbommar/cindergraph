# QA iteration 41: type-sensitive sizeof lookup scaling

The binding-sensitive sizeof classifier searched the entire Use vector for
each pointer operand's terminal name span. Replaced repeated linear searches
with a lazily constructed BTreeMap from span to binding. Building only on the
first relevant dereference avoids this allocation for functions that do not
need it. Entry-or-insert preserves the original first-match rule for duplicate
spans in recovered input. This changes the lookup component from O(R*U) to
O(U log U + R log U), not the complexity of all dataflow passes.

Added `--shape sizeof-pointer` to `tools/bench_analysis.py` and three regression
sizes (32, 128, 512): repeated sizeof(*(*(p))) must leave no value flow and keep
memory coverage complete for a declared int**.

Local timings, CPython 3.14.3, same release extension path and generated input:

```bash
uv run --no-sync python tools/bench_analysis.py --shape sizeof-pointer
```

At 512 operands / 8,217 source bytes, median data_flow time was 1.132160 ms
before and 1.104319 ms after; call_summaries was 1.148989 and 1.104414 ms.
The analyze control also moved from 0.991928 to 0.971983 ms. These small changes
do not establish a strong end-to-end speedup; no general performance claim or
CI timing threshold is made. Each median uses seven batches of five calls after
a warmup. Baseline was `edf2777` plus QA through iteration 40; after adds this
lookup index. The new implementation was rebuilt with
`uv run --no-sync maturin develop --release` before measuring.

Validation:

- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- `cargo +1.88.0 clippy --workspace --all-targets --all-features -- -D warnings`:
  passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,566 passed, ten skipped, three optional Joern cases deselected.
- Focused Ruff check/format, `ty check python/`, Rustfmt and whitespace checks:
  passed.

No semantic scope expansion: VLA-size dependence and unresolved operand types
remain open. No publication, commit, remote CI or external evaluator. Changes
local/uncommitted; broader goal active.
