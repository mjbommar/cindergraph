# QA iteration 9: wide-signature summary costs

Added a deterministic `parameters` shape to `tools/bench_analysis.py`: one
function with 32, 128 or 512 parameters returning only the first parameter.
Added four seeded selected-parameter tests at widths 1, 32, 128 and 512;
these pass before and after the optimization. An initial test omitted the
existing `sink_parameter: None` output field; that test expectation was
corrected before changing production code.

The summary implementation now iterates parameter definitions once instead of
using a fresh filtered `nth` scan for every parameter. It also skips building
call-transfer provenance when no direct call exists. Indirect-call uncertainty
is still computed by the summary fixed point; this skips only a destination
set that cannot contain a named transfer.

Command: `uv run python tools/bench_analysis.py --shape parameters`.
Both measurements use Python 3.14.3 and the release `_native.abi3.so`, rebuilt
with `TMPDIR=/home/mjbommar/.cache/cindergraph uv run maturin develop --release`.
Baseline is `edf2777` plus pending iterations 6–8; after adds this optimization.
Seven batches of five calls follow a warmup; times include parsing and Python
conversion. These are local microbenchmarks, not a corpus-wide speed claim.

| Parameters | Source bytes | Before median summary ms | After median summary ms |
| --- | --- | --- | --- |
| 32 | 264 | 0.03198 | 0.02970 |
| 128 | 1060 | 0.16807 | 0.13767 |
| 512 | 4516 | 1.39000 | 1.02744 |

The first after run overlapped a Rust build and was noisy (512-parameter
median 1.2965 ms). The table uses the rerun after our build and lint processes
finished. Shared-machine variance remains; no timing threshold was added.
Per-parameter provenance still rebuilds shared indexes, so scaling remains a
performance frontier rather than a closed issue.

Validation: 507 Python/review tests passed, ten skipped, three optional Joern
cases deselected; 575 Rust tests passed, one ignored. Rust 1.88 Clippy and
format checks, Ruff and ty passed. Commands were `uv run pytest python/tests/
review/test_design_contracts.py -q`, `cargo +1.88.0 test --workspace
--all-features -q`, and the standard lint commands. Nothing was published;
all current changes remain local.
