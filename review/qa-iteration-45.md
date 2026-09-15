# QA iteration 45: line accounting and coordinate transformations

Added `test_line_accounting.py` with 210 corpus cases, four empty/whitespace
boundary cases and a corpus-presence guard. Each fixture checks that code,
blank and other lines partition the total, the total equals LF count plus one,
and no bucket is negative. LF-to-CRLF conversion must preserve all four counts.
Prepending LF must add exactly one blank line, shift function start/end lines
by one, and retain function identity and measured length.

All checks passed; no engine defect surfaced. Clarified the reference's line
convention: empty input has one blank line, and a trailing LF contributes an
empty final line. This follows the existing LineIndex behavior rather than
changing measurements to match wc -l. Lone CR does not start a new line.

Validation on `edf2777` plus pending QA work:

```bash
uv run --no-sync pytest python/tests/test_line_accounting.py -q
uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q
uv run --no-sync ruff check python/tests/test_line_accounting.py
uv run --no-sync ruff format --check python/tests/test_line_accounting.py
uv run --no-sync ty check python/tests/test_line_accounting.py
git diff --check
```

Focused: 215 passed. Full: 2,794 passed, ten skipped, three optional Joern cases
deselected. Lint, format, type and whitespace checks passed. Used iteration
41's release extension; no Rust changes, rebuild or Rust-suite rerun. No
semantic-equivalence claim for arbitrary newline edits, performance comparison,
publication, commit, external evaluator or remote CI. Changes local/uncommitted;
broader goal active.
