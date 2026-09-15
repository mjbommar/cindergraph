# QA iteration 32: generated native calling signatures

The shipped native stub declared every function as accepting arbitrary
`*args: Any, **kwargs: Any`, despite all 15 exported functions providing usable
runtime signature metadata. That let missing arguments and misspelled keywords
pass static checking. Updated `tools/gen_native_stub.py` to use
`inspect.signature`, preserving names, defaults and parameter kinds, and emit
static methods representing module functions. Metadata failures now stop
generation rather than silently degrading to unrestricted arguments.

Regenerated `python/cindergraph/_native/__init__.pyi`; no stub was hand-edited.
Argument values and returns remain Any: this improves calling conventions,
not typed result schemas or value-type validation. Added two module-wide AST
tests checking the generated callable names/parameters/default counts against
the loaded extension and refusing unrestricted argument declarations. Both
tests failed before regeneration and pass afterward.

Direct type-checking experiment:

```bash
uv run --no-sync ty check --extra-search-path python /home/mjbommar/.cache/cindergraph/qa32_bad_calls.py
uv run --no-sync ty check --extra-search-path python /home/mjbommar/.cache/cindergraph/qa32_good_calls.py
```

The negative file calls `analyze()` with no text, `analyze(text=..., typo=True)`,
and `backward_slice(text, function)` without a node. Ty reports exactly two
missing-argument errors and one unknown-argument error. The positive file uses
valid positional/keyword calls and passes. These scratch inputs remain in the
cache for reproduction.

Validation:

- `uv run --no-sync python tools/gen_native_stub.py --check`: passed.
- `uv run --no-sync ty check python/`: passed.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  2,329 passed, ten skipped, three optional Joern cases deselected.
- Ruff check/format on the generator/test, generated-stub format check, and
  diff whitespace check: passed.

Baseline is `edf2777` plus pending QA changes; extension is the release build
from iteration 30. No Rust implementation or runtime signature change, so no
Rust rebuild or suite rerun this turn. No wheel rebuild, publication, commit,
external evaluator or remote CI. Value/result schema generation remains open;
the broader QA goal stays active.
