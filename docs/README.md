# Cindergraph documentation

This directory contains maintained guidance for the current source tree. The
project is pre-alpha: an API described here may still change, and a configured
CI job is not evidence that a release artifact has passed on that platform.

## Start here

- [Project roadmap](ROADMAP.md) prioritizes semantic architecture, precise
  control and memory analysis, public APIs, publication, Glaurung migration and
  measured post-release expansion. It is the portfolio-level execution plan.
- [Semantic foundation plan](architecture/semantic-foundation-plan.md) reviews
  the extraction and today's semantic repairs, identifies root causes, and
  proposes staged architecture changes with correctness gates. It is a plan,
  not a claim that those changes are implemented.
- [Precise indirect dispatch](architecture/precise-indirect-dispatch.md) designs
  per-dispatch computed-`goto` target resolution, sound fallback semantics,
  explicit uncertainty, public metadata, and staged acceptance gates.
- [Dependency policy](architecture/dependency-policy.md) records the minimal
  runtime boundary, current resolved graph, and why development solvers stay
  outside published artifacts.
- [Native graph API](architecture/native-graph-api.md) records the NetworkX
  boundary, required graph contract, optional expansion, and measured baseline.
- [Python source analysis](reference/source-python.md) documents parsing,
  diagnostics, graphs, dataflow, summaries, slicing, export and compatibility.
- [DecBench-adjacent CFG workflows](use-cases/decbench-adjacent.md) documents
  parity serialization, GED-ready NetworkX graphs, topology-only `pyjoern`
  migration, and the explicit boundary of that compatibility surface.
- [Rust analysis sessions](reference/rust-api.md) documents the owning
  `AnalysisUnit`, cached flows and summaries, typed function views, and identity
  boundaries.
- [Source metrics](reference/source-metrics.md) defines every reported metric
  and the comparison population.
- [Install and build](install.md) gives checkout, wheel, sdist and Rust path
  dependency workflows without assuming a registry release.
- [Release checklist](releasing.md) covers registry setup, clean-candidate
  validation, protected publication and post-upload verification.
- [Support and evidence](support-and-evidence.md) distinguishes intended
  support, configured automation, locally exercised artifacts and open release
  work.
- [Benchmark records](benchmarks/README.md) contain dated, reproducible
  comparisons, including the narrow Joern comparison for DecBench-adjacent CFG
  extraction.
- [Relationship to Glaurung](architecture/glaurung.md) records what was
  extracted, what remains Glaurung-specific, which project currently owns
  fixes, and the planned dependency migration.
- [QA records](https://github.com/mjbommar/cindergraph/blob/main/review/README.md)
  are chronological snapshots with commands,
  inputs and limitations. They explain why behavior changed; they are not a
  replacement for the maintained references above.

## Documentation contract

Examples in the two reference pages are executable tests. Relative Markdown
links in `README.md`, `docs/` and the review index are checked locally. This
does not prove every prose claim: capability statements must still identify
the API behavior, test population or artifact that supports them.

Documentation uses these evidence terms consistently:

| Term | Meaning |
| --- | --- |
| implemented | Present in the inspected source tree |
| locally tested | A named command passed on the stated local snapshot |
| configured | Automation exists, but its successful execution is not claimed |
| published | The named registry artifact was uploaded and independently fetched |
| supported | The project intentionally accepts bug reports for that environment |

Unless an artifact record says otherwise, commands run from the checkout and
may exercise an editable native extension. See the support matrix before
inferring wheel, source-distribution or cross-platform coverage.
