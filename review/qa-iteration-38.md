# QA iteration 38: rendered API documentation and stale capability claims

The strict rustdoc gate passed initially, but reading the emitted API's source
documentation found stale capability descriptions that link checking cannot
detect. Corrected the dataflow overview's claim of no interprocedural flow to
describe bounded parameter/return summaries and unmodeled caller-visible memory.
Removed the claim that mutual recursion takes one extra round; the implementation
uses a revisiting worklist and a total evaluation budget. Documented recovery,
unresolved bindings, memory gaps and duplicate-source identity as uncertainty
sources, while retaining the distinction that an observed positive path can
coexist with incomplete analysis.

Also replaced the full-symbol-table equivalence claim with the actual local
lexical model and made explicit that summary completeness is not a general C
soundness certificate, including the known VLA gap. Updated standalone adapter
error messages to name Cindergraph and corrected its optional-dependency install
guidance from glaurung[graphs] to cindergraph[graphs]. Unsupported AST/DDG
behavior and exception classes are unchanged.

Validation on `edf2777` plus pending QA work:

- `RUSTDOCFLAGS='-D warnings' cargo +1.88.0 doc --workspace --no-deps`: passed,
  including a repeat after the final wording changes.
- `uv run --no-sync maturin develop --release`: extension rebuilt; subsequent
  Rust edits were documentation comments only.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,560 passed, ten skipped, three optional Joern cases deselected.
- `cargo +1.88.0 test --workspace --all-features -q`: 579 passed, one ignored.
- Focused Ruff check/format, `ty check python/`, Rustfmt and diff whitespace:
  passed.
- `rg` found no remaining occurrences of the corrected no-interprocedural-flow,
  one-extra-round or glaurung[graphs] claims in the inspected modules.

No algorithm or performance change. Historical comparison measurements were not
rerun or promoted to current evidence. No commit, publication, remote CI or
external evaluator. Changes local/uncommitted; broader goal active.
