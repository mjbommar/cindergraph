# Source metrics

Metrics describe the recovered syntax and general CFG. They do not prove
program equivalence, exploitability or runtime performance. Inspect parser
diagnostics and function coverage before comparing reports, especially for
decompiler output. Partial recovery and the syntax-walk budget can leave partial
measurements; the raw shape record includes `truncated`.

```python
import cindergraph as cg

report = cg.analyze("int flat(void){return 1;} int choose(int x){return x?1:0;}")
assert report.hotspots(by="cyclomatic")[0].name == "choose"
columns = cg.feature_names()
assert all(len(row) == len(columns) for _, row in cg.features(report.source))
```

## Definitions

| Metric | Meaning in this implementation |
| --- | --- |
| `lines` | Physical span of a function, from first to last source line |
| `code_lines` | Lines on which at least one token begins |
| `blank_lines` | File-level lines containing only whitespace |
| `other_lines` | Nonblank file lines on which no token begins; includes comments and multiline-token continuations |
| `tokens`, `bytes` | Lexical token count and UTF-8 byte span |
| `cyclomatic` | `E - N + 2` on the entry-reachable general CFG, clamped to at least one |
| `cognitive` | Syntax-based decision and nesting score described below |
| `max_nesting` | Maximum nesting of control structures; plain braces do not add nesting |
| `max_loop_depth` | Maximum nesting of loops |
| `calls` | Call expressions, including indirect and nested calls |
| `callees` | Recovered directly named callees, sorted and deduplicated |
| `unreachable_statements` | Statements the structural CFG classifies as unreachable; no general constant folding |

Only LF starts a new line. CRLF is preserved in file input; a lone CR is not
counted as a new source line. Offsets address UTF-8 bytes, and dialect
normalisation changes the coordinate space. See [Python API](source-python.md).
The file line count is `LF count + 1`: empty input has one blank line, and a
trailing LF adds an empty final line. Code, blank and other buckets partition
this total; it is not the same convention as counting newline bytes with `wc -l`.

Cognitive complexity adds one plus nesting for `if`, `switch`, loops and
ternaries. `else`, `else if` and `goto` add one without a nesting surcharge.
Runs of like logical operators add one per run. Nesting increases in controlled
bodies, not headers. Recursive calls do not add a recursion penalty. This is
the implemented variant, not a claim of interchangeability with other tools.

Halstead volume, difficulty and effort use this parser's lexical operator and
operand classification. Compare them using the same Cindergraph version and
input preparation; different classifications produce different values.

## Ranking and comparing

`report.hotspots(by=..., limit=...)` sorts descending, using the function name
as tie-breaker. `limit=None` returns all functions, zero returns none, and a
negative count raises `ValueError`. An unknown metric raises
`ValueError`. `report.summary()` aggregates the recovered unit. `to_dict()`
provides ordinary Python data for storing results.

```python
before = cg.analyze("int f(int x){return x;}")
after = cg.analyze("int f(int x){if(x)return 1;return 0;}")
delta = cg.compare(before, after, metrics=["cyclomatic"])
assert delta["matched"][0]["name"] == "f"
assert delta["matched"][0]["deltas"]["cyclomatic"] == 1
assert not delta["added"] and not delta["removed"]
```

`compare()` matches by name and reports `after - before`. Totals cover matched
functions only. Added/removed names are reported separately; renames appear as
one of each. Duplicate function names on either side raise `ValueError` rather
than silently selecting a body. Repeated metric names also raise `ValueError`:
otherwise totals could count one column twice. An empty metric selection is
allowed and still reports matching, added and removed names.

`features()` exposes a fixed-order numeric vector; always pair rows with
`feature_names()` from the same installed version. It still parses and builds
graphs and is not a shortcut around analysis cost. For benchmark methodology
and observed limits, see the repository's `review/qa-iteration-*.md` records and
`tools/bench_analysis.py`.
