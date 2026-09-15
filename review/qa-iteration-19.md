# QA iteration 19: GraphML whitespace normalization

The source-adapter probe first showed that its snippets normalize whitespace,
so a literal carriage return was not a useful end-to-end witness. Inspection
of the public Rust GraphView serializer found a distinct lower-level issue:
the common XML escaper emitted literal tabs, newlines and carriage returns in
both data and attribute values. XML line-ending/attribute normalization can
change these values even when the output is well-formed.

Added a Rust regression for graph name, label, attribute key and value fields.
It failed before the change. The shared XML escaper now emits `&#x9;`, `&#xA;`
and `&#xD;`, which preserve those characters through XML normalization. The
reference page documents this behavior. Prohibited-character dropping from
iteration 18 is unchanged.

The relevant specification rules are XML 1.0
[line-ending handling](https://www.w3.org/TR/xml/#sec-line-ends) and
[attribute-value normalization](https://www.w3.org/TR/xml/#AVNormalize).
This regression asserts the required encoding; it does not establish full
GraphML schema conformance or source-label losslessness.

Local validation, baseline `edf2777` plus pending QA increments:

- Rust 1.88 workspace tests: 578 passed, one ignored.
- Rust Clippy with warnings denied and formatting/diff checks: passed.
- Release ABI3 extension rebuilt for the Python regression suite.
- `uv run --no-sync pytest python/tests/ review/test_design_contracts.py -q`:
  947 passed, ten skipped, three optional Joern cases deselected.

No external evaluator, publication or remote CI run occurred. Changes remain
local and uncommitted.
