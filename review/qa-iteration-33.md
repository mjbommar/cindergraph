# QA iteration 33: selected source paths cannot silently disappear

File-boundary inspection confirmed analyze_path/export_path already share byte
decoding, then found a discovery issue: recursive parse_source used is_file()
to filter matching `.c`/`.h` paths. A dangling symlink was silently omitted,
allowing an empty result despite an unreadable selected source. Both suffix
regressions reproduced this before the fix.

Classification now uses stat() and S_ISREG. A missing target or other stat/read
error is surfaced to the caller. Directories and special files remain excluded;
the fix does not indiscriminately open matching FIFOs/devices. Added a
source-named-directory negative control and documented the selection behavior.
Symlink tests skip only when the platform cannot create symlinks; neither
skipped in this local run.

Validation on `edf2777` plus pending QA work:

```bash
uv run --no-sync pytest python/tests/test_source_discovery_errors.py -q
uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q
uv run --no-sync ruff check python/cindergraph/compat/pyjoern.py python/tests/test_source_discovery_errors.py
uv run --no-sync ruff format --check python/cindergraph/compat/pyjoern.py python/tests/test_source_discovery_errors.py
uv run --no-sync ty check python/
git diff --check
```

Focused tests: three passed. Full suite: 2,332 passed, ten skipped, three optional
Joern cases deselected. Lint, format, full Python type check and whitespace gate
passed. No Rust change, rebuild or Rust-suite rerun; extension remains iteration
30's release build. No claim about inaccessible directories that glob itself
does not enumerate: this change covers selected paths only.

No publication, commit, remote CI or external evaluator. Changes remain local;
broader QA and semantic work remain active.
