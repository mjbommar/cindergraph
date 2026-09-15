# Dependency policy and current budget

Date: 2026-09-15. Status: implemented policy and local measurement.

Cindergraph keeps analysis in the Rust crate and does not require an external
server, database, compiler framework, graph engine, or solver. Dependencies
must own a clear capability that is smaller and safer to reuse than to maintain
locally. Development oracles are not runtime dependencies.

## Current normal dependency graph

On the current workspace lockfile, `cargo tree -p cindergraph --edges normal`
contains one direct third-party crate, `regex`, and its four transitive crates:
`aho-corasick`, `memchr`, `regex-automata`, and `regex-syntax`.

The Python extension adds PyO3 and its build-time macro stack. The installed
Python distribution has no mandatory Python package dependency; `networkx` is
available only through the `graphs` extra.

The 2026-09-15 lockfile measurement contains 18 unique third-party crates in
the normal/build tree of the wheel package (PyO3's procedural-macro stack
included), no duplicate versions in the normal tree, and five beneath the core
crate. The latest existing manylinux wheel is about 1.29 MB, but artifact size
must be remeasured after rebuilding the eventual release candidate.

`serde_json` is a development-only dependency used to independently parse and
check the built-in JSON writer in tests. Graph serialization itself is a small,
total Rust writer and therefore does not pull Serde into the core crate or
native wheel.

## Boundaries

- Axeyum, SMT solvers, Clang/LLVM, Joern, and NetworkX may be comparison or
  development tools but are not core runtime dependencies.
- Tree-sitter comparison code lives in its own nested, locked Rust workspace
  under `tools/tree-sitter-adapter`; its parser, C grammar, and JSON dependencies
  do not enter the root workspace lockfile, core crate, extension, or wheel.
- A dependency added to the Rust core needs an identified semantic or safety
  owner and a measured reason not to implement the narrow requirement locally.
- Python runtime dependencies remain optional unless the native API cannot
  provide the advertised base functionality by itself.
- Dependency counts and artifact sizes are measurements of a named lockfile and
  candidate, not timeless project claims.

`regex` remains because dialect normalization currently promises byte-sensitive
compatibility for three established rewrites. Removing it requires differential
tests against that contract; it should not be replaced by ad hoc matching only
to improve a dependency count.
