# C source-front-end robustness comparison

Status: executable contract, frozen initial population, and Cindergraph,
Tree-sitter C, and Clang adapters. A dirty-tree local comparison has validated
the machinery; no publishable cross-tool robustness record has run yet.

## Question and boundary

The comparison asks how much trustworthy structure each front end preserves
when C is incomplete, decompiler-shaped, pathological, or deliberately
hostile. It does not ask which product has the largest API. Joern is a CPG
platform, Clang is a compiler front end, CDT is IDE tooling, Tree-sitter is an
incremental concrete-syntax parser, and Cindergraph is a compact tolerant
semantic-analysis library. Results must therefore be reported per comparable
product rather than collapsed into one rank.

The existing [Joern comparison](joern-decbench-2026-09-15.md) remains a frozen,
narrow CFG record. Its VJ-GED scores are not a semantic oracle and its
per-translation-unit JVM startup measurements are not parser-kernel timings.

## Invariants

1. Every adapter receives the same immutable input bytes and declared build
   context. A transformed input is a separate specimen with its own identity.
2. Corpus membership is manifest-driven. Tool failure cannot remove a specimen
   or function from the denominator.
3. Timeout, signal, exception, invalid output and missing output are distinct
   results, never empty successful graphs.
4. Recovery yield and semantic confidence are separate fields. More nodes are
   not automatically a better recovery.
5. Negative semantic claims are counted only when the tool exposes the
   coverage needed to justify them.
6. Cold startup, warmed service time, analysis time and serialization time are
   recorded separately where the tool permits it.
7. Every run records tool versions, adapter revision, source revision, manifest
   hash, command, limits and host identity.
8. No upstream issue, comment, pull request or benchmark submission is part of
   this programme. DecBench material remains local evidence under its AI rules.

## Specimen identity and manifest

One JSON Lines manifest is the authoritative population. Each row contains:

```json
{
  "schema": 1,
  "id": "dialect/ghidra/captured-start",
  "family": "decompiler",
  "source_path": "tests/fixtures/decompiler_dialects/ghidra.c",
  "source_sha256": "...",
  "slice": {"start_byte": 0, "end_byte": 431},
  "origin": {"kind": "captured", "tool": "ghidra", "version": "..."},
  "dialect": "decompiler",
  "build_context": null,
  "expected_functions": ["_start"],
  "oracle": {"kind": "manual-fixture", "ref": "..."},
  "mutation": null
}
```

`source_sha256` identifies the complete stored source; the byte slice identifies
the specimen within a multi-case fixture. Generated cases additionally record
the generator name, version, seed and parameters. A build-context record names
the language standard, target triple, compiler arguments, working directory,
macro definitions and include snapshot. Paths alone are not sufficient
provenance for headers that can change.

The manifest generator must reject duplicate IDs, missing files, hash drift,
overlapping case metadata and generated cases without a seed. It emits the
ordered manifest hash used by every result file.

`tools/robustness_contract.py manifest MANIFEST --source-root ROOT` implements
this boundary using only the Python standard library. The identity is SHA-256
over the ordered rows serialized as canonical compact JSON plus one newline per
row; insignificant JSON whitespace and key order therefore cannot change it,
while specimen order does. Every declared source must be a regular non-symlink
file below `ROOT`, and generated or mutated rows require generator version and
seed metadata.

## Common result envelope

Adapters write one result for every `(run, tool, specimen)` tuple:

```json
{
  "schema": 1,
  "run_id": "...",
  "manifest_sha256": "...",
  "tool": {"name": "cindergraph", "version": "...", "adapter": "..."},
  "execution": {
    "host_id": "sha256-of-host-name",
    "platform": "...",
    "python": "...",
    "command": ["..."],
    "timeout_ns": 10000000000,
    "memory_limit_bytes": 536870912,
    "source_revision": "...",
    "source_dirty": false
  },
  "specimen_id": "dialect/ghidra/captured-start",
  "status": "success",
  "failure": null,
  "timing_ns": {"startup": 0, "analysis": 1234, "serialization": 567},
  "peak_rss_bytes": 123456,
  "diagnostics": {"errors": 1, "warnings": 0, "recovery_nodes": 2},
  "yield": {"functions": ["_start"], "covered_source_bytes": 407},
  "claims": {"syntax_complete": false, "cfg_complete": false},
  "artifacts": {
    "analyzed_source": "...",
    "native_diagnostics": "...",
    "normalized_ast": "...",
    "normalized_cfg": "..."
  }
}
```

`status` is one of `success`, `timeout`, `memory_limit`, `signal`, `exception`,
`invalid_output`, `unsupported` or `adapter_error`. An adapter may say that a
metric is unavailable; it may not synthesize a favourable value. Tool-native
diagnostics and raw output are retained beside normalized projections.

One result JSONL file contains exactly one `(run_id, tool)` identity.
`tools/robustness_contract.py results MANIFEST RESULTS --source-root ROOT`
requires exactly one row for every manifest specimen, including failures, and
rejects duplicates, foreign specimens, mixed tool identities, manifest drift,
unknown statuses, mixed execution identities, and success/failure payload
contradictions. Required fields must be present even when a tool reports an
unavailable metric as `null`. Host names are hashed; dirty source state remains
explicit and disqualifies a run from publication.
Adapters may add typed artifact fields. The Cindergraph adapter retains the
exact post-policy source and full native diagnostics in addition to normalized
graphs, so decompiler preparation and aggregate diagnostic counts remain
auditable.

The Cindergraph adapter executes every specimen in a fresh worker process. Its
supervisor enforces a per-specimen wall-clock timeout, optionally applies a
POSIX address-space limit before importing the extension, and converts timeout,
signal, memory exhaustion, worker failure, exception, and invalid output into
distinct result rows. One crash therefore cannot shrink the denominator or
poison later session state. Import startup, analysis, serialization, and worker
peak RSS are recorded separately where the host exposes them.
Completed rows are synchronously journalled to a `.partial` JSONL file and
atomically promoted only after the complete result set validates. A supervisor
or host failure therefore leaves inspectable evidence without presenting a
truncated file as a completed run.

```bash
mkdir -p target/tmp
run_dir=$(mktemp -d "$PWD/target/tmp/robustness.XXXXXX")
uv run python tools/run_robustness_cindergraph.py \
  docs/benchmarks/manifests/robustness-v1.jsonl \
  --source-root "$PWD" \
  --output "$run_dir/results.jsonl" \
  --artifacts "$run_dir/artifacts" \
  --run-id local-cindergraph \
  --timeout-seconds 10 \
  --memory-mib 512
uv run python tools/robustness_contract.py results \
  docs/benchmarks/manifests/robustness-v1.jsonl \
  "$run_dir/results.jsonl" --source-root "$PWD"
uv run python tools/summarize_robustness_results.py \
  docs/benchmarks/manifests/robustness-v1.jsonl \
  "$run_dir/results.jsonl" --output "$run_dir/summary.json"
```

Local result directories belong below `target/tmp`; they are not publishable
records because the working tree may be unattributable. A dated record must
name a clean source revision and capture its host and command metadata.
The dependency-free summarizer validates every input result set again and
reports fixed-denominator status, completeness, yield, oracle coverage, source
coverage, timing, and RSS per family and overall. A family with no independent
function oracle is explicitly `coverage_available: false`; zero oracle entries
can never become a perfect recovery score.

`tools/run_robustness_clang.py` invokes a pinned Clang executable directly and
adds no package dependency. It sends the exact specimen bytes on standard
input, retains raw diagnostics and Clang JSON AST, and projects source-derived
function ASTs into deterministic closed graphs. A nonzero compiler exit with a
valid partial AST is a successful recovery result with
`syntax_complete: false`, not a crashed analysis. Signals, timeouts, and invalid
AST output remain failures. CFG completeness, process-startup separation,
peak RSS, and configured build contexts are currently unavailable and are
reported as such rather than inferred.

The Tree-sitter adapter is deliberately a separate, nested Rust workspace at
`tools/tree-sitter-adapter`. Its lockfile pins `tree-sitter` 0.27.0 and
`tree-sitter-c` 0.24.2, following the current official
[parser API](https://docs.rs/tree-sitter/0.27.0/tree_sitter/struct.Parser.html)
and [C grammar language constant](https://docs.rs/tree-sitter-c/0.24.2/tree_sitter_c/constant.LANGUAGE.html).
It is not a member of Cindergraph's workspace and cannot alter the crate or
wheel dependency graph. The native worker records the grammar ABI, traverses
without adapter recursion, retains `ERROR` and missing nodes, and emits a
closed named-syntax graph. Its Python supervisor applies the same fixed-result,
timeout, signal, journal, provenance, and artifact rules as the other adapters.

```bash
cargo build --release --locked \
  --manifest-path tools/tree-sitter-adapter/Cargo.toml \
  --target-dir target/tmp/tree-sitter-adapter-target
uv run python tools/run_robustness_tree_sitter.py \
  docs/benchmarks/manifests/robustness-v1.jsonl \
  --source-root "$PWD" \
  --binary "$PWD/target/tmp/tree-sitter-adapter-target/release/cindergraph-robustness-tree-sitter" \
  --output "$run_dir/tree-sitter-results.jsonl" \
  --artifacts "$run_dir/tree-sitter-artifacts" \
  --run-id local-tree-sitter --timeout-seconds 10
```

## Population lanes

### A. Configured, valid C

Select complete translation units from real projects with a captured
`compile_commands.json` context and vendored or content-addressed headers.
Include GNU and MSVC extensions separately from portable C. Clang compilation
and executable differential checks supply independent semantic evidence;
agreement between analysis implementations is not the oracle.

Measure declaration and function recovery, exact source ranges, resolved
bindings and types, CFG successor semantics, direct calls, local def-use,
diagnostics, time and peak RSS.

### B. Broken editor-like and damaged C

Derive mutations from the valid and decompiler populations at explicit
severity levels:

- every-prefix truncation at selected token boundaries;
- deletion, insertion and substitution of tokens;
- missing delimiters, identifiers, types and statement terminators;
- damaged directives and unavailable includes;
- garbage between otherwise valid neighbouring functions;
- incomplete declaration, expression, statement and function-body edits.

Run isolated mutations before compound random mutations. For paired cases,
measure retained functions and declarations, source-span overlap, error
locality, graph validity, deterministic replay and whether completeness fails
closed. A finite mutation suite establishes a measured survival curve, not
totality over all byte strings.

### C. Raw decompiler dialects

Store complete captured functions and files from named versions of Ghidra,
IDA, Binary Ninja, angr, dewolf, r2dec and RetDec where licensing permits.
Preserve raw text. Backend-specific or general sanitization creates additional
manifest rows and must never overwrite the raw specimen.

Measure expected-function recovery, neighbouring-function retention, CFG
successors, labels and indirect dispatch, calls, object reads/writes, recovery
qualification and reproducibility. Manually adjudicate reduced semantic
disagreements; VJ-GED remains only a shape measure.

### D. Pathological scaling

Generate independent logarithmic series for nesting depth, expression width,
parameter count, declarator depth, initializer width, label/goto density,
switch size, CFG fan-in/fan-out and ambiguous token runs. Each generator emits
small oracle-checkable members as well as stress members.

Report success/failure, time and RSS against input tokens and produced nodes.
Retain the first failing member and the largest passing member. Explicit tool
budgets are part of the result rather than silently raised until a case passes.

### E. Intentional obfuscation

Use reproducibly generated and provenance-bearing examples of control-flow
flattening, opaque predicates, bogus branches, dead-code inflation, indirect
dispatch, aliased dispatcher updates and nested state machines. Keep the
unobfuscated source, transform configuration, compiled binaries and behavioral
vectors as the oracle bundle.

Score survival and semantic facts, not source resemblance. In particular,
check sound target supersets, reachable exits, caller-visible effects and
whether unsupported alias or indirect-control facts visibly invalidate
negative answers.

## Normalized comparisons

No single representation is common to all five tools. Use the smallest honest
projection for each comparison:

| Product pair | Comparable projection |
| --- | --- |
| All tools | process outcome, diagnostics, declarations/functions and spans |
| Cindergraph / Tree-sitter | syntax recovery locality, error nodes and retained structure |
| Cindergraph / Clang / CDT | declarations, bindings/types where available, statements and CFG facts |
| Cindergraph / Joern | functions, CFG, calls and explicitly aligned dependence relations |

Node labels, implicit nodes, entry/exit conventions and preprocessing must be
normalized by documented rules. Every aggregate reports both the fixed
manifest population and the matched subpopulation; matched results cannot
replace failures in the fixed denominator.

## Metrics

The primary scorecard is a vector, not a scalar ranking:

- **survival:** crash, timeout, memory exhaustion and invalid-output rates;
- **yield:** expected functions/declarations and source bytes retained;
- **locality:** unaffected neighbouring facts retained after a localized edit;
- **validity:** unique IDs, closed edge endpoints and representation invariants;
- **semantic agreement:** independently adjudicated bindings, successors,
  calls and def-use facts;
- **honesty:** false-complete results and unqualified negative claims;
- **determinism:** byte-identical normalized replay under varied process hash
  seeds;
- **scale:** cold/warm time and peak RSS curves.

Publish distributions and per-family results. A mean over unrelated families,
or a matched-only percentage without the fixed denominator, is prohibited.

## Execution phases

1. **Complete:** implement and test the manifest/result validator with no
   runtime dependency.
2. **Complete:** materialize the current 210-file, 26-case, 256-seed, and
   33-case controlled-recovery populations without changing their existing
   tests. The 525-row
   `docs/benchmarks/manifests/robustness-v1.jsonl` manifest has canonical hash
   `49bc22e83931262b21a96a0c427e2e741f9173957969735aec85656fa2e1307a`.
3. **Complete:** add Cindergraph and Tree-sitter adapters and run the initial
   recovery lanes. `tools/run_robustness_cindergraph.py` writes a complete,
   contract-validated result row and normalized AST/CFG artifacts for every
   specimen. Its CFG-completeness claim requires recovered graphs plus
   `recovery_free` and `control_targets_complete`; unsupported recovery-node
   measurements remain `null` rather than being estimated. The separately
   pinned Tree-sitter adapter records error/missing recovery nodes and a closed
   syntax graph without entering the product dependency graph.
4. **In progress:** add Clang with both raw and captured build-context modes.
   The dependency-free raw-input adapter is complete; compile database context,
   CFG extraction, and independently normalized semantic facts remain.
5. Add a persistent Joern adapter and keep startup outside warmed analysis
   timing; preserve the existing frozen comparison unchanged.
6. Add a pinned standalone CDT adapter with identical scanner context.
7. **In progress:** add scaling generators, resource isolation and
   deterministic replay. Cindergraph specimen workers now enforce wall time,
   support address-space limits, survive signals, and report peak RSS. Shared
   host metadata, replay orchestration, external-adapter isolation, and scaling
   series remain.
8. Add captured decompiler and reproducible obfuscation populations.
9. Reduce and adjudicate disagreements; convert Cindergraph defects into
   semantic regressions at their owning representation.
10. Publish a dated record only from an attributable Cindergraph revision and
    pinned external-tool identities.

The first useful result is not a five-way league table. It is the Cindergraph,
Tree-sitter and Clang recovery curve plus a ledger of minimized cases where
Cindergraph crashes, loses an unaffected neighbour, emits an invalid graph or
claims completeness without support.

## Local development observation

An isolated dirty-tree run on 2026-09-15 exercised all three adapters over all
525 manifest specimens with a ten-second per-specimen timeout. Each emitted 525
contract-valid results. All recovered the 930 clean oracle functions.
Cindergraph recovered all 25 named decompiler cases; raw Clang recovered 13.
Tree-sitter C recovered 20. The Cindergraph/Clang twelve-case difference is
confined to Binary Ninja, dewolf, Ghidra, and IDA dialect constructs handled by
Cindergraph's declared decompiler policy. Tree-sitter's five exact-name misses
are three Ghidra declarator/name recoveries and two IDA annotation forms. On
the unannotated damaged lane, Cindergraph returned no function for 28
specimens, Clang for 51, and Tree-sitter for 57. These are
machinery-validation observations from a dirty source tree, not a versioned
benchmark, semantic-correctness result, or publishable performance comparison.

The controlled lane constructs 33 damaged targets across eleven operators and
three severity levels. Its left and right guard functions are emitted outside
the damaged byte region, giving 63 generator-owned locality obligations rather
than using any compared parser as the oracle. Before remediation, Cindergraph
retained 60/63, Tree-sitter 59/63, and Clang 52/63. Stratification showed that
all three Cindergraph misses were the right neighbour after an unterminated
block comment; every target-local, delimiter, token, directive, and truncation
stratum was complete. The C lexer now preserves the required unterminated-
comment diagnostic but conservatively restarts at a later line shaped like a
top-level function definition. It rejects control statements and calls as
restart points, and recovery continues to invalidate completeness. The same
fixed population now measures Cindergraph at 63/63. This is a concrete
Cindergraph regression result; the external-tool figures remain dirty-tree
development observations.
