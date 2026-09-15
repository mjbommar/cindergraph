# Relationship to Glaurung

Cindergraph began as the reusable C source-analysis subsystem inside
[Glaurung](https://github.com/mjbommar/glaurung). It is now a standalone Rust
crate and Maturin/PyO3 Python distribution. This page distinguishes extraction,
subsequent development and the still-pending consumer migration.

## What the extraction did

The initial standalone commit copied the selected Glaurung implementation with
filtered Git history. It did not reimplement the dataflow engine from a prose
description or substitute a second parser. The authoritative source revision,
included directories and history tool are recorded in
[EXTRACTION.md](../../EXTRACTION.md).

The initial boundary included:

- the tolerant token, syntax and general CFG layers;
- C parsing, metrics, dataflow and graph export;
- the narrow comparison graph previously named for Joern;
- the relevant PyO3 bindings, Python facade and focused fixtures.

Package layout, imports and public names necessarily changed. The comparison
module was renamed `parity` to avoid implying a general Joern implementation.
Those adaptations are part of packaging the same subsystem, not evidence of a
new analysis engine.

## What was deliberately left in Glaurung

The standalone package does not contain Glaurung's:

- C-to-Glaurung-LLIR lowering;
- solver-backed feasibility and equivalence checking;
- project knowledge-base persistence;
- binary-analysis and decompiler pipelines;
- DecBench orchestration.

These depend on Glaurung-specific models or applications and are not required
for a reusable source-analysis core. Cindergraph's `parity` module only emits
the graph properties consumed by stored offline comparison fixtures. It is not
a code property graph, a CPGQL engine or a Joern distribution.

## Why the implementations now differ

The extraction exposed analysis contracts to independent Rust and Python
consumers and added adversarial tests around them. Those tests found defects in
the copied implementation, including call-site-insensitive interprocedural
flow, incomplete pointer-store dependence, ambiguous function identity and
unreported recovery uncertainty. The historical red baseline is preserved in
the [design review](https://github.com/mjbommar/cindergraph/blob/main/review/design-review-2026-09-14.md);
later QA records show the repairs and their bounded evidence.

Consequently, a difference between current Cindergraph and Glaurung can have
three causes:

1. a standalone package adaptation;
2. a post-extraction correctness or performance repair;
3. functionality intentionally retained only in Glaurung.

It should not be described generically as a porting omission without checking
the extraction revision and the relevant QA record.

## Current ownership and migration status

Cindergraph is presently a source fork of this subsystem, not yet the single
shared implementation. Glaurung still carries its embedded source-analysis
modules, and neither repository automatically receives the other's fixes.
Publishing Cindergraph and migrating Glaurung are therefore separate changes:

1. publish a reviewed Cindergraph crate and Python distribution;
2. replace Glaurung's reusable copied modules with an explicit Cindergraph
   dependency;
3. keep Glaurung-specific lowering, solver and persistence adapters in
   Glaurung;
4. compare Glaurung's public source-analysis output before and after migration
   on the same fixtures;
5. remove the embedded copy only after Glaurung's complete local gates pass.

The first Cindergraph release does not depend on completing step 2. Requiring a
consumer to use an unpublished registry package would create a circular release
gate. During migration, Glaurung can first pin the reviewed source revision by
path or Git identity, then change to the released version after independent
registry verification.

## Acceptance criteria for the Glaurung migration

The migration is complete only when all of the following are true:

- Glaurung's manifests resolve the reusable source-analysis core to
  Cindergraph rather than compiling a second copy;
- public Python behavior is either preserved or each intentional change is
  documented with a regression test;
- Glaurung-specific functionality remains outside the reusable crate;
- both repositories record the exact dependency/release identity used by their
  tests;
- the embedded duplicate is removed and cannot silently drift back into use.

Until then, report fixes as Cindergraph fixes unless they have also been
applied and tested in Glaurung.
