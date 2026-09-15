# QA iteration 18: GraphML character boundaries

Boundary probes found invalid XML when source labels contained U+FFFE or
U+FFFF. The existing XML escaper removed prohibited low control characters but
missed these two excluded BMP values. Eight of 30 new representation/character
cases failed before the fix, as independently detected by ElementTree.

The XML escaper now drops U+FFFE/U+FFFF under the existing lossy filtering
policy. This follows XML 1.0's [Char production](https://www.w3.org/TR/xml/#charsets).
It does not indiscriminately remove Unicode noncharacters: supplementary
U+1FFFF is allowed by that production and remains in the export.

Tests exercise AST, CFG, DDG, CDG and PDG across NUL, U+0001, U+FFFE, U+FFFF,
CJK text and U+1FFFF, parsing each produced GraphML document and checking edge
endpoints. AST tests explicitly verify preservation of the legal Unicode
characters. Reference docs disclose GraphML's lossy label filtering and point
to JSON when those characters must be retained.

Local evidence on the rebuilt release ABI3 extension:

- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  947 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 577 passed, one ignored.
- Rust 1.88 Clippy/format, Ruff/ty on the new test and diff whitespace checks
  passed.

This checks XML well-formedness and graph endpoint integrity, not full GraphML
schema validation or every downstream viewer. Baseline is `edf2777` plus the
pending QA work. Nothing published or pushed; no external evaluator invoked.
