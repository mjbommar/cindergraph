# Cindergraph QA records

Files in this directory are dated-in-practice engineering snapshots from the
2026-09-14 extraction and QA sequence. They record the source state, commands,
measurements and limitations known at each iteration. Later work can supersede
their results, so begin with the maintained [documentation index](../docs/README.md)
and [support matrix](../docs/support-and-evidence.md).

The subsequent [semantic foundation review and plan](../docs/architecture/semantic-foundation-plan.md)
synthesizes this sequence and later source changes into architectural causes,
replacement data structures, algorithms and migration gates. It also records
the interrupted patch's compile failure; historical passing runs below do not
validate that later source state.

## Reading the sequence

The original [design review](design-review-2026-09-14.md) is the red baseline.
[QA iteration 1](qa-progress-2026-09-14.md) records the first repair and
benchmark pass. Numbered files then form a chronological audit trail:

| Iterations | Theme | Useful current records |
| --- | --- | --- |
| 2–5 | reaching values, call/control identity, unresolved names and pointer dependence | [2](qa-iteration-2.md), [3](qa-iteration-3.md), [4](qa-iteration-4.md), [5](qa-iteration-5.md) |
| 6–15 | executable references, notices, uncertainty, seeded provenance, summary scaling and package workflows | [6](qa-iteration-6.md), [7](qa-iteration-7.md), [10](qa-iteration-10.md), [11](qa-iteration-11.md), [12](qa-iteration-12.md), [13](qa-iteration-13.md), [15](qa-iteration-15.md) |
| 16–24 | fail-closed corpora, graph serialization, globals, address flow, comma expressions and control provenance | [16](qa-iteration-16.md), [17](qa-iteration-17.md), [20](qa-iteration-20.md), [21](qa-iteration-21.md), [22](qa-iteration-22.md), [23](qa-iteration-23.md), [24](qa-iteration-24.md) |
| 25–31 | unevaluated `sizeof`, indexed regions, artifact refresh, identity guards and mutation testing | [25](qa-iteration-25.md), [26](qa-iteration-26.md), [27](qa-iteration-27.md), [28](qa-iteration-28.md), [29](qa-iteration-29.md), [30](qa-iteration-30.md), [31](qa-iteration-31.md) |
| 32–39 | generated signatures, path errors, comparison populations, pointer `sizeof`, rendered docs and call-graph identity | [32](qa-iteration-32.md), [33](qa-iteration-33.md), [34](qa-iteration-34.md), [35](qa-iteration-35.md), [36](qa-iteration-36.md), [37](qa-iteration-37.md), [38](qa-iteration-38.md), [39](qa-iteration-39.md) |
| 40–46 | ABI3 wheel refresh, lookup scaling, process determinism, query validation, line accounting and crate artifacts | [40](qa-iteration-40.md), [41](qa-iteration-41.md), [42](qa-iteration-42.md), [43](qa-iteration-43.md), [44](qa-iteration-44.md), [45](qa-iteration-45.md), [46](qa-iteration-46.md) |
| 47–94 | initializer sequencing, type recovery, public docs, publication resilience, summary scaling and clean-snapshot packaging | [47](qa-iteration-47.md), [48](qa-iteration-48.md), [49](qa-iteration-49.md), [50](qa-iteration-50.md), [51](qa-iteration-51.md), [52](qa-iteration-52.md), [53](qa-iteration-53.md), [54](qa-iteration-54.md), [55](qa-iteration-55.md), [56](qa-iteration-56.md), [57](qa-iteration-57.md), [58](qa-iteration-58.md), [59](qa-iteration-59.md), [60](qa-iteration-60.md), [61](qa-iteration-61.md), [62](qa-iteration-62.md), [63](qa-iteration-63.md), [64](qa-iteration-64.md), [65](qa-iteration-65.md), [66](qa-iteration-66.md), [67](qa-iteration-67.md), [68](qa-iteration-68.md), [69](qa-iteration-69.md), [70](qa-iteration-70.md), [71](qa-iteration-71.md), [72](qa-iteration-72.md), [73](qa-iteration-73.md), [74](qa-iteration-74.md), [75](qa-iteration-75.md), [76](qa-iteration-76.md), [77](qa-iteration-77.md), [78](qa-iteration-78.md), [79](qa-iteration-79.md), [80](qa-iteration-80.md), [81](qa-iteration-81.md), [82](qa-iteration-82.md), [83](qa-iteration-83.md), [84](qa-iteration-84.md), [85](qa-iteration-85.md), [86](qa-iteration-86.md), [87](qa-iteration-87.md), [88](qa-iteration-88.md), [89](qa-iteration-89.md), [90](qa-iteration-90.md), [91](qa-iteration-91.md), [92](qa-iteration-92.md), [93](qa-iteration-93.md), [94](qa-iteration-94.md) |

Iterations omitted from the “useful current records” column still remain part
of the sequence; the table is a route map, not a claim that the linked subset
alone validates the present tree.

## Evidence rules

- Read the baseline stated in each record. Most later notes use commit
  `edf2777` plus explicitly pending changes, not a clean release commit.
- Treat a passed focused test as evidence only for its named behavior and input
  population. Mutation and corpus tests are finite robustness evidence, not a
  proof of parser totality or general C soundness.
- Keep editable-extension, wheel, sdist and Rust-crate results separate. An old
  artifact hash does not validate later source changes.
- Keep local checks, commits, pushes, remote CI and registry publication
  separate. No record in this sequence establishes publication.
- Optional Joern cases were normally deselected and no DecBench interaction was
  performed. Stored fixtures and parity projections do not establish Joern
  compatibility.

The latest numbered QA record is [QA iteration 94](qa-iteration-94.md). Declarator
type recovery was repaired in [iteration 56](qa-iteration-56.md), complex
parameters were hardened in [iteration 57](qa-iteration-57.md), release
automation was tightened in [iteration 58](qa-iteration-58.md), and iteration
59 shared provenance indexes. Iteration 60 removes the remaining repeated
definition scans while preserving sparse-signature performance; iteration 61
makes tag/version/changelog identity fail closed; iteration 62 repairs
aggregate/typedef parameter identity with a translation-unit index; iteration
63 adds lexical block-scope typedef identity without reintroducing quadratic
file-scope scans; iteration 64 recovers direct VLA-bound dependencies and makes
unsupported typedef-level VLA provenance fail closed; iteration 65 extends
that recovery to outer parameter bounds with source-order and nested-signature
guards; iteration 66 covers parenthesized outer declarators and arrays of
function pointers without weakening nested-signature isolation; iteration 67
separates unevaluated array-bound operands from evaluated VLA type bounds;
iteration 68 makes unresolved `_Generic` association selection fail closed;
iteration 69 represents direct bound writes and removes quadratic node-transfer
work; iteration 70 corrects the pointer-order probe and documents analysis-order
event tables; iteration 71 removes build-host paths from wheel SBOMs and proves
relocatable source-to-wheel reproduction; iteration 72 adds generated mixed
pointer/direct-write ordering coverage; iteration 73 expands pointer-copy and
reassignment oracles and indexes source-span candidates for better scaling;
iteration 74 restores known-local stores through parenthesized dereferences
while retaining fail-closed handling for unknown pointers; iteration 75 makes
pointer arithmetic taint completeness without regressing pointer-copy scaling;
iteration 76 preserves newly visible local targets after second-order pointer
mutation while keeping that bounded model explicitly incomplete; iteration 77
separates direct pointer copies from dereference loads and re-solves the
monotone constraints after second-order target growth; iteration 78 excludes
address operands from direct-copy constraints, preserving pointer-depth
identity and restoring completeness for fully known double-pointer loads;
iteration 79 makes integer-derived pointer alternatives fail closed while
retaining bounded scaling on deeply nested known-pointer cast chains;
iteration 80 checks every value-producing conditional arm while preserving
known comma-expression and pointer-conditional precision; iteration 81 limits
pointer-arithmetic taint to result-producing regions and reuses binding indexes
to remove repeated whole-definition and whole-edge scans from the solver;
iteration 82 processes the fixed-point lattice in explicit 64-bit words and
borrows live sets during edge recovery; iteration 83 separates address-taking
escape events from value reads and definitions, eliminating quadratic false
dependence edges while retaining conservative dead/unused diagnostics;
iteration 84 gives the private value lattice a dense index so public escape
events no longer consume empty fixed-point words; iteration 85 replaces
repeated whole-use scans for indirect accesses with indexed span queries and
adds a durable pointer-access benchmark; iteration 86 worklists direct pointer
copies and makes memory completeness fail closed when a pointer is not
initialized on every path to an access.
