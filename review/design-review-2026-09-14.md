# Cindergraph design and implementation review

Reviewed revision: `28ae864681d9ffd07dd43948147b01478bfe7617`, initially clean
`main`. Review date: 2026-09-14. Production code was not changed in this review.

## Verdict

The extraction builds and its main structural boundaries are coherent, but the
implementation does **not** currently meet its advertised analysis contracts.
The interprocedural API cannot be relied on for positive or negative flow
verdicts. Its complete/incomplete flag also overstates what was analyzed.
Graph and packaging tests passing did not establish those semantic properties.

The Rust core has no Python dependency and forbids unsafe code. The binding
crate separates extension linking from the ordinary Rust build. Python keeps
NetworkX optional. The general CFG and comparison/parity CFG are separate, and
the shared graph serialization layer supports all five representations without
duplicating four writers for each. Those are useful design choices to retain.

## Reproduced defects

All examples below run through the current Python public API. The implementation
locations identify the underlying Rust defects where applicable.

### P1: interprocedural reachability does not follow call sites

`crates/cindergraph/src/csource/dataflow/interproc.rs:407`, especially the loop
at line 436, visits *every* summary with enough parameters if the current
summary returns its parameter. No call edge is required.

- `int f(int x){return x;} int g(int y){return y;}` reports
  `reaches(code, "f", 0, "g") == "yes"`, despite having no calls.
- `int g(int y){return 0;} int f(int x){g(x);return 0;}` reports `"no"`,
  although `g` receives `x` directly.

Repair requires explicit call-site edges carrying argument positions and value
provenance. Track `(function identity, parameter position)` states, not only
function names. Summary return flow alone cannot reconstruct the call graph.

### P1: summaries use binding occurrence instead of reaching values

`interproc.rs:285-399` propagates binding IDs without honoring definition kills,
approximates a return as any node without a write, and excludes a binding from
return flow if it appears as an argument at *any* call site.

- `int f(int x){x=0;return x;}` incorrectly reports parameter-to-return flow.
- `int f(int x){if(x){} return 0;}` incorrectly reports that flow too.
- `int g(int y){return 0;} int f(int x){g(x);return x;}` incorrectly omits it.

Retain actual return nodes and per-expression call/result records. Compute
provenance from definition-to-use edges; scope identity alone is insufficient.
The independent scalar tests show that the existing intraprocedural solver
already distinguishes the last write in these simple cases.

### P1: uncertainty does not propagate from known incomplete callees

`interproc.rs:233-250` checks whether a callee name exists, but not whether its
summary is complete. For `g(y){return external(y);}` and
`f(x){return g(x);}`, `g.complete` is false and `f.complete` is true.
This contradicts `call_summaries()` documentation. Parser/CFG diagnostics are
also discarded before summaries are constructed in the Python bindings.

Propagate incompleteness through the fixed point and retain recovery and
unsupported-operation reasons. A clean `no` needs a defined supported fragment.

### P1: pointer dependence is missing from slices

For `int f(int x){int y=0;int *p=&y;*p=x;return y;}`, slicing the return
node 5 produces `[2, 3, 5]`, omitting node 4 (`*p=x`) and the parameter entry.
The store changes the returned value. Pointer and field stores are recorded
only as uses, so leaving previous definitions alive does not capture the new
dependence. `dataflow/mod.rs:66-78` incorrectly claims this loses no real edges.

Memory dependence needs an explicit conservative representation or an incomplete
result contract. The public slice description currently promises every node
that can affect the seed without that qualification. Do not use these slices
as evidence that excluded statements cannot affect the result.

### P2: duplicate function identities collapse inconsistently

Metrics and exports preserve duplicate definitions as lists; the compatibility
file adapter rejects ambiguity; `compare()` explicitly selects the last one.
`summarize()` instead combines flows from multiple definitions under one name
while retaining the last parameter count and declaring the result complete.
`int f(int x){return x;} int f(int x,int y){return y;}` becomes one complete
two-parameter summary with both parameters flowing to the return.

Use source identity internally and reject or mark ambiguous name-based queries.
Document the chosen policy for every convenience API.

### P2: path APIs disagree about byte coordinates

`python/cindergraph/source.py:757` reads with `Path.read_text()`, which translates
CRLF to LF; `analyze_path()` at line 799 decodes bytes without translation.
For the same CRLF file, `export_path(format="json")` differs from exporting
`analyze_path(path).source`. This silently shifts all following source spans.
Share the byte-preserving reader between file APIs.

### P2: unresolved binding sentinel violates the Python join contract

`data_flow()` documents `binding` as an index into `bindings`. For
`int a,b; int f(void){a=1;return b;}`, definitions and uses carry `4294967295`
while `bindings` is empty. Moreover the write to `a` is reported as reaching the
read of `b` because both share `Binding::FREE`.

Document and represent unresolved identities explicitly, preferably without
an unsigned integer masquerading as an index. Preserve distinct unresolved
names even if their types are unknown. The cross-global edge is an imprecision;
the undocumented out-of-range index is a concrete API contract defect.

### P2: a slice accepts a nonexistent node

`backward_slice("int f(void){return 1;}", "f", 999)` returns `[999]`.
The Python binding checks the function name but not its node range. Reject an
invalid seed before walking dependence edges. Consider ambiguity checks too.

## Packaging, documentation, and validation gaps

- The default Python tests run against an editable install on Linux 3.12 in CI.
  The release workflow builds artifacts but never installs/tests them, and its
  publish job depends on artifact jobs rather than test jobs for that tag.
  Local review verified Linux wheel/sdist installation; macOS and Windows
  execution remain unverified.
- `.github/workflows/release.yml:21` selects `macos-13`, which GitHub retired
  in December 2025. This is a concrete release blocker. See the
  [official retirement notice](https://github.blog/changelog/2025-09-19-github-actions-macos-13-runner-image-is-closing-down/).
- `cargo package -p cindergraph --list` includes README but neither LICENSE nor
  NOTICE. The Python wheel build includes both. Make the Rust package carry
  the same provenance files.
- Strict public Rust documentation fails: unresolved `LoopKind`, `Cfg`, and
  removed `crate::metrics::type_name::normalize_type` links; links to private
  implementation items; and an ambiguous `write` function/macro link.
- Python docstrings refer to absent `docs/reference/source-metrics.md` and
  `docs/reference/source-python.md`, and retain Glaurung installation/error
  text. The three Markdown files initially present do not replace those API
  contracts or explain the supported semantic fragment.
- `tools/gen_native_stub.py` emits `*args: Any, **kwargs: Any -> Any` for every
  function. Its check establishes exported name coverage, not signature or
  result consistency. `ty check` cannot validate most binding boundaries with
  those stubs. Use real signatures and typed result records at the boundary.
- Rust returns `Parsed<T>` but most Python graph/dataflow functions discard
  diagnostics. An empty result does not distinguish empty input from recovery
  failure. The facade needs a consistent result/diagnostic policy.
- Ten default Python tests are permanently skipped CLI tests. Three optional
  Joern tests are deselected. Neither counts as validated standalone behavior.
- Some Rust corpus helpers return silently on missing directories. The metrics
  test checks an aggregate file threshold while its second corpus path points
  inside the crate, although `tests/decbench_corpus` lives at workspace root.
  Assert each intended corpus exists and was traversed separately.
- `--no-default-features` currently disables nothing: the core declares no
  features. It proves the core builds independently of the Python crate, but
  is not evidence for multiple feature configurations.

## Tests and evidence

Local baseline on reviewed production revision:

| Check | Result |
| --- | --- |
| Rust 1.88 workspace tests, all features | 572 core passed, 1 ignored; 3 binding tests passed |
| Rust 1.88 Clippy with warnings denied | Passed |
| Rust 1.88 formatting | Passed |
| Core without default features | Passed |
| Python 3.14.3 default suite | 151 passed, 10 skipped, 3 deselected |
| Ruff and ty | Passed |
| Generated stub check | Passed; limited coverage as above |
| Rust package archive build verification | Passed; provenance files absent |
| Isolated Linux Python 3.12 wheel and sdist smoke tests | Passed |
| Strict public rustdoc | Failed |
| Review contract suite | **12 failed, 34 passed** |

`review/test_design_contracts.py` contains normal assertions, no xfails or
expectations that bless incorrect outputs. It is intentionally outside the
default `testpaths`; invoke it explicitly:

```sh
uv run pytest review/test_design_contracts.py -q
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
```

The passing review cases include an independent last-write oracle over 27
straight-line assignment combinations; six cross-API cases covering malformed
input, Unicode, loops, and duplicate names; and 100 seeded recovery mutations
checking all five graph representations for determinism, valid endpoints,
unique node IDs, source bounds, and consistent function ordering. These are
bounded structural checks, not a proof of complete C semantics or totality.

Rebuilt the release wheel and sdist into `target/review-dist/`. Installed the
wheel in a fresh Python 3.12.13 environment outside the repository, with isolated
imports and no NetworkX installed. Basic analysis passed, and the unrelated
function reachability defect reproduced there, ruling out an editable-import
artifact for that failure.

The sdist also built and installed in a separate fresh Python 3.12 environment;
isolated analysis smoke passed. Artifact SHA-256 values:

```text
wheel  67d293c1587c6dfd2fce0c4696f8bebdd448ed78a1aed4ef49f70baa14941c55
sdist  2981bb451cfb053fdec423b8af4af7078d8b153543e35365edf15519685b2a6f
```

## Repair order and acceptance

1. Correct interprocedural identity, call-site traversal, return provenance,
   and uncertainty propagation. Require all seven summary/reachability review
   assertions to pass and add argument-position, recursion, indirect-call,
   nested-call, shadowing, and recovery cases.
2. Define memory-dependence coverage and preserve incomplete-analysis evidence
   through slices, summaries, and Python results. Verify pointer and field
   stores, address escape, and unknown external effects with concrete examples.
3. Unify source reading, identity/duplicate policy, binding representation, and
   query validation; promote the relevant review tests into ordinary CI.
4. Restore standalone API documentation and meaningful typing. Add strict
   rustdoc and per-corpus presence checks.
5. Repair the runner matrix and Rust package contents; install/test built
   artifacts on every supported platform and Python baseline before publishing.

This review establishes actionable defects and bounded areas that passed. It
does not certify complete C analysis, Joern parity, or cross-platform release
readiness. No Joern/DecBench process, upstream interaction, release, or remote
mutation was performed during the review.
