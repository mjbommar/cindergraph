# Cindergraph versus Joern for DecBench-adjacent CFG work

Date: 2026-09-15

> Follow-up: the
> [difference roadmap](joern-difference-roadmap-2026-09-15.md) records the
> disposition of all 72 original differences. Two semantic parity fixes close
> 34 graphs, and provider preprocessing closes two conditional-compilation
> graphs, for a projected 894/930 exact matches (96.13%). Checked per-class
> evidence adjudicates the other 36: macro expansion, invalid Joern entry
> roles, layout builtins, branch hints, complex operators, one computed-goto
> orphan and one C-permitted conditional-store evaluation order. The resulting
> exact-or-individually-adjudicated coverage is 930/930. A fresh full run
> measured the projected result exactly: 894 zero and 36 nonzero graphs, with
> zero provider failures. The baseline numbers and artifact below remain
> immutable; the roadmap links every follow-up proof, the post-remediation raw
> record, and a fail-closed audit covering all 36 current differences.

## Verdict

For the specific job DecBench uses Joern for—recovering per-function control-flow
graphs from decompiled C-like text—Cindergraph is already a credible local
replacement, but it is not graph-equivalent to Joern on the full test
population.

- Cindergraph recovered all 930 source function definitions recovered by Joern.
- 858 of 930 shared functions (92.26%) had zero DecBench VJ-GED; 72 (7.74%)
  differed.
- On the smaller, hand-maintained DecBench corpus, 29 of 30 functions (96.67%)
  had zero VJ-GED. `strops.c::str_cmp` differed with VJ-GED 5.
- The provider calls took 0.571 seconds for Cindergraph and 1,067.263 seconds
  for Joern over 210 translation units: 1,870.5 times longer for Joern in this
  end-to-end setup.
- On raw decompiler-dialect fixtures, Cindergraph recovered 25 of 25 expected
  functions. Joern, after DecBench's general sanitisation and preprocessing,
  recovered 16 of 25.
- The optional source-control-dependence differential test passed all three
  fixtures after its stale fixture path was repaired.

These are parity and operational measurements, not proof that either graph is
semantically correct. Joern is the historical comparison implementation, not
ground truth.

## Scope and population

The clean-C comparison used all 210 checked-in translation units in:

| Population | Files | Cindergraph functions | Joern raw names | Shared | VJ-GED 0 | Nonzero |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `crates/cindergraph/tests/decompiler_fixtures/src` | 196 | 900 | 1,112 | 900 | 829 | 71 |
| `tests/decbench_corpus/src` | 14 | 30 | 30 | 30 | 29 | 1 |
| **Total** | **210** | **930** | **1,142** | **930** | **858** | **72** |

The population manifest hash is
`789a5d066e89b90034b184f66d61995d30bc48e104b50d5de28725009a06c8cc`.
Every translation unit was parsed independently by each provider. Imports were
warmed before timing.

Joern returned 212 additional dictionary names. Inspection against the source
classified 208 as preprocessor macro identifiers (nine function-like macro
patterns), two as GNU operator tokens (`__real__` and `__imag__`), and two as
alias/ifunc target symbols (`wk158_absent_scale` and `ifunc_double`). None was a
source function definition. They are therefore reported but excluded from the
930-function paired score denominator. Counting the raw provider dictionaries
without this filtering would incorrectly present macro tokens as functions.

## CFG agreement

The primary measure is DecBench 1.1's own `vj_ged` implementation. It compares
node-degree and entry/exit-role assignments; zero does not establish labelled
or statement-level graph equality. As a second check, the harness tested exact
directed graph isomorphism while preserving entry and exit roles.

| Result over 930 shared functions | Count | Percentage |
| --- | ---: | ---: |
| VJ-GED zero | 858 | 92.26% |
| VJ-GED nonzero | 72 | 7.74% |
| Role-preserving isomorphic | 857 | 92.15% |
| Not role-preserving isomorphic | 72 | 7.74% |
| Isomorphism check timed out | 1 | 0.11% |

The timeout was `deep152_conditional_tower` (34 nodes, 49 edges on both sides,
VJ-GED zero). The auxiliary exact-isomorphism check has a two-second timeout to
avoid exponential matching on symmetric graphs; VJ-GED itself was not timed
out.

The nonzero VJ-GED distribution was: 5 (30 functions), 8 (8), 10 (6), 18 (4),
6 (3), 64 (2), 17 (2), 16 (2), and one each at 205, 130, 75, 72, 43, 28, 26,
24, 22, 19, 14, 13, 9, 3, and 2. The largest differences were:

| Function | VJ-GED | Cindergraph nodes/edges | Joern nodes/edges |
| --- | ---: | ---: | ---: |
| `wide154_dense_effects` | 205 | 632 / 1,048 | 591 / 966 |
| `big151_branch_ladder` | 130 | 985 / 1,513 | 959 / 1,461 |
| `lcs_recover` | 75 | see raw record | see raw record |
| `lcs_length` | 72 | see raw record | see raw record |
| `matrix_chain_cost` | 64 | see raw record | see raw record |
| `edit_distance` | 64 | see raw record | see raw record |
| `huffman_code_lengths` | 43 | see raw record | see raw record |
| `apply_opcode` | 28 | 6 / 0 | 12 / 11 |
| `pk161_member_offset` | 26 | 9 / 8 | 1 / 0 |

The complete per-file and per-function record, including all 72 differences,
is [`data/joern-decbench-2026-09-15.json`](data/joern-decbench-2026-09-15.json).
The recurring score-5 pattern is often one additional Cindergraph node and two
additional edges, suggesting a systematic loop/control-node convention rather
than random parser failure. The larger cases require semantic, source-level
adjudication before either graph can be called correct.

## Performance

| Provider | Total | Median/file | p95/file | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Cindergraph | 0.571 s | 1.731 ms | 5.217 ms | 54.879 ms |
| Joern | 1,067.263 s | 5.044 s | 5.236 s | 8.195 s |

This corresponds to approximately 368.05 files/second for Cindergraph and
0.197 files/second for Joern. The total-time ratio is 1,870.5x; the median-file
ratio is 2,913.6x.

This is the latency experienced through each Python provider API, which is the
relevant cost for a DecBench run. It is not a parser-kernel microbenchmark:
Joern starts external JVM work for each translation unit, while Cindergraph is
in-process. The figures come from one sequential pass on an otherwise
uncontrolled host, so they should not be treated as stable hardware benchmarks.

## Decompiler-dialect recovery

The dialect comparison contains 26 declared cases: 22 captured backend outputs
and four explicit reconstructions. One captured r2dec case is deliberately a
crash message rather than C and is expected to yield no function. Cindergraph
received each case raw. Joern received `sanitize_decompiled_c` followed by
`preprocess_decompiled_c`, matching DecBench's general cleanup path.

| Positive cases | Cindergraph raw | Joern after general cleanup |
| --- | ---: | ---: |
| Captured | 21 / 21 | 14 / 21 |
| Reconstructed | 4 / 4 | 2 / 4 |
| Total | 25 / 25 | 16 / 25 |

Joern did not recover the expected function in seven captured cases: dewolf
`history_def_last` and `bi_reverse`; Ghidra `_start`, a switch case label, and
`Base::op`; IDA `dis_func1`; and r2dec `bi_reverse`. It also missed two raw IDA
reconstructions. Those two are not evidence of an actual IDA pipeline loss:
the IDA adapter applies backend-specific replacements before storage, beyond
the general cleanup used here.

Of the 16 functions both recovered, 15 had zero VJ-GED. Captured Binary Ninja
`bi_reverse` had VJ-GED 10; its input includes the non-C operator text
`var_14 u>>= 1`, so recovery by both parsers does not imply identical CFG
interpretation. Both correctly recovered no function from the deliberate r2dec
crash-message case.

The full case record is
[`data/joern-dialects-2026-09-15.json`](data/joern-dialects-2026-09-15.json).
Its 26-case population hash is
`8c9c7190985942b37dc7758495160fa7a2caded5e9b2036c6a3db0ea5549fc86`.

The parser also accepts legacy implicit-int definitions and K&R parameter
declaration lists. This capability was added after the fixed 26-case dialect
run and therefore does not change the table above. Regression coverage includes
both `main(B) { ... }` and semicolon-terminated old-style parameter declarations;
macro invocations followed by an ordinary definition are explicitly rejected
as false K&R headers.

## Control dependence

`python/tests/test_source_dependence_joern.py` is an optional differential test
covering three fixtures: conditional polarity, loop shapes, and early loop
exit. Its path still pointed to the pre-extraction Glaurung fixture location;
the comparison was silently skipping. After changing it to the Cindergraph
fixture location, all three tests passed.

The assertion is deliberately one-sided: every Joern controller-to-controlled
source-line pair must occur in Cindergraph after line normalisation. It neither
requires Cindergraph to have no additional pairs nor compares full Joern DDG or
AST behaviour. It is useful evidence for this narrow CDG surface only.

## Reproduction and provenance

The clean comparison was produced with:

```bash
export DEC="$HOME/.cache/glaurung/decbench-full/decbench"
PYTHONPATH="$PWD/python:$DEC" "$DEC/.venv/bin/python" \
  tools/compare_joern_decbench.py \
  --output docs/benchmarks/data/joern-decbench-2026-09-15.json
```

The dialect comparison and optional CDG check were produced with:

```bash
PYTHONPATH="$PWD/python:$DEC" "$DEC/.venv/bin/python" \
  tools/compare_joern_dialects.py \
  --output docs/benchmarks/data/joern-dialects-2026-09-15.json
uv run --no-sync pytest -q python/tests/test_source_dependence_joern.py -m decbench
```

| Component | Version or identity |
| --- | --- |
| Cindergraph base commit | `edf2777abc99e1c3113b4f1d51fe021f31432538` |
| Cindergraph compared-source hash | `31b6c44973b470ff690106cc1246ac8c27902fa4b6364747f61ba3e3c1d61306` |
| Cindergraph status hash | `c74d721dd21946ef194c772a5c76e7a27ff43c0f89166008c99d4ec98e2fdf55` |
| DecBench commit | `f76dae075d4d82004fb21132b3f15e43b680e179` |
| DecBench | 1.1 |
| pyjoern / Joern bundle | 4.0.150.4 |
| cfgutils | 1.16.0 |
| NetworkX | 3.6.1 |
| Python | 3.12.13 |
| Java | OpenJDK 25.0.4 |
| Host | Linux 7.0.0-27, Intel Core i9-12900K, 123 GiB RAM |

The Cindergraph worktree was dirty because this evaluation was performed amid
the extraction and documentation work. The clean-C artifact therefore records
both the Git base and hashes of the exact compared source and status. Its
SHA-256 is
`ea8c4236a7b37d1fef3110aa69b9fbcbd7e4c650293f9e377e86f578f0c6c4ce`.
The dialect artifact's SHA-256 is
`f63ab2737d67bcfe340f06333065c59c2646c7030821c25ea3d49d4a7548d3ff`;
it records status hash
`9bd9ccda8990bf039fb52caea4c68cf7535235506410ee4684cd867bca373f86`
and the same compared-source hash as the clean-C artifact.

No external DecBench benchmark submission, issue, comment, pull request, or
other upstream interaction was performed. This is local comparative evidence.

## What this does not establish

- It does not compare source reconstruction, type recovery, byte similarity,
  or any DecBench metric other than CFG VJ-GED.
- It does not establish semantic correctness where Cindergraph and Joern agree;
  two implementations can share the same mistake.
- It does not establish which provider is correct for the 72 disagreements.
- It does not make Cindergraph a replacement for Joern's broader code-property
  graph, AST, DDG, query, language, or security-analysis functionality.
- It does not provide multi-run confidence intervals or controlled cold/warm
  JVM timing.

The next correctness step is not more broad CI. It is to classify the 72
nonzero cases by construct, reduce representative examples, and adjudicate both
graphs against source semantics. The recurring small-difference cluster should
come first, followed by the disconnected `apply_opcode` macro case and the
large dynamic-programming and branch-ladder outliers.
