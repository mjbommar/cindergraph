# Extraction provenance

Cindergraph was extracted with history from
[`mjbommar/glaurung`](https://github.com/mjbommar/glaurung).

- Source checkout: `/home/mjbommar/projects/personal/glaurung`
- Source revision before filtering: `0892552157be6bd9267007231419ff6606a2dd38`
- Extraction date: 2026-09-14
- History tool: `git-filter-repo 5df1b13a5cfd`
- Initial included implementation: `src/syntax`, `src/csource`, the two source
  PyO3 binding modules, Python source facades, and their focused tests/fixtures
- Deliberately excluded from the standalone core: C-to-Glaurung-LLIR lowering,
  solver-backed feasibility/equivalence, Glaurung KB persistence, and DecBench
  orchestration

The filtered history rewrites commit IDs. Consult `.git/filter-repo/commit-map`
in the extraction worktree to relate original and filtered commits. Releases do
not include that internal mapping; this file pins the authoritative origin.

The parity graph code reproduces only the graph properties consumed by
DecBench's graph-edit-distance calculation. Cindergraph is not Joern, does not
ship Joern, and does not implement a general code property graph or CPGQL.
