# IOCCC source CFG comparison, 2026-09-15

This record compares Cindergraph and Joern on 15 winning entries from the
International Obfuscated C Code Contest (IOCCC), spanning 1984 through 2025.
It tests source-front-end tolerance and DecBench-adjacent function/CFG
extraction. It does not test CPG queries, data-flow analysis, compilation, or
program behaviour.

The expanded population contains 44 real function definitions. Expected
function names were adjudicated independently with GCC-preprocessed source and
Clang's GNU89 AST, rather than copied from either tool's result. One entry,
`1984/mullender`, intentionally contains no C function definition: it is data
that is made executable by the build procedure.

## Result

| Provider/input | TP | FP | FN | Precision | Recall | Crashes | Total time |
|---|---:|---:|---:|---:|---:|---:|---:|
| Cindergraph automatic preparation | 44 | 0 | 0 | 1.000 | 1.000 | 0 | 0.149 s |
| Joern raw source | 44 | 28 | 0 | 0.611 | 1.000 | 0 | 86.744 s |
| Joern identical prepared source | 44 | 0 | 0 | 1.000 | 1.000 | 0 | 101.961 s |

The raw-source Joern false positives are macro identifiers interpreted as
functions. They disappear when Joern receives the same preprocessed text as
Cindergraph. The fair function-recovery conclusion is therefore a tie, not a
Cindergraph accuracy win. The timing is wall-clock provider time on this host;
it includes Joern process and workspace overhead and is not a parser-kernel
microbenchmark.

For the 44 shared functions on identical prepared input, 38 CFGs have zero
VJ-GED distance and six do not. The total distance is 296. The largest
residual is `2001/anonymous::pain` at 252; that translation unit also requires
substantial parser recovery. A recovered graph is useful output, but its
presence is not proof that every edge is semantically correct.

| Winner | Expected functions | Cinder errors | Cinder warnings | Joern raw FP | Exact CFGs | Non-exact CFGs | VJ-GED sum |
|---|---:|---:|---:|---:|---:|---:|---:|
| 1984/mullender | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1985/applin | 1 | 0 | 1 | 0 | 1 | 0 | 0 |
| 1986/wall | 3 | 0 | 1 | 7 | 3 | 0 | 0 |
| 1987/korn | 1 | 0 | 1 | 0 | 1 | 0 | 0 |
| 1988/westley | 2 | 0 | 2 | 1 | 2 | 0 | 0 |
| 1990/theorem | 7 | 0 | 6 | 1 | 5 | 2 | 18 |
| 1992/adrian | 5 | 0 | 2 | 6 | 5 | 0 | 0 |
| 1995/vanschnitz | 1 | 0 | 1 | 0 | 1 | 0 | 0 |
| 2001/anonymous | 13 | 16 | 17 | 1 | 10 | 3 | 268 |
| 2005/anon | 1 | 0 | 0 | 7 | 1 | 0 | 0 |
| 2011/akari | 1 | 0 | 0 | 0 | 1 | 0 | 0 |
| 2018/algmyr | 6 | 0 | 0 | 3 | 5 | 1 | 10 |
| 2020/carlini | 1 | 0 | 0 | 2 | 1 | 0 | 0 |
| 2025/diels-grabsch | 1 | 0 | 1 | 0 | 1 | 0 | 0 |
| 2025/kurdyukov | 1 | 0 | 0 | 0 | 1 | 0 | 0 |

The six non-zero distances are:

| Function | VJ-GED |
|---|---:|
| `1990/theorem::e` | 8 |
| `1990/theorem::main` | 10 |
| `2001/anonymous::Runi` | 3 |
| `2001/anonymous::main` | 13 |
| `2001/anonymous::pain` | 252 |
| `2018/algmyr::main` | 10 |

The Kurdyukov difference was reduced to the evaluation-order boundary between
a comma prefix and a nested conditional expression. Restoring that boundary
in the parity projection closes the full winner at 60 nodes, 87 edges and
VJ-GED zero.

Algmyr's earlier large raw-source difference was mainly a preprocessing
comparison artifact. Its remaining distance of 10 reduces independently to
two empty-body loop forms:

```c
while (C());
while (fclose(f[--w]), w);
```

Joern 4.0.150.4 omits the loop back-edge in both minimized cases. C semantics
require the condition to be evaluated again when it is true, and Clang 22's
`debug.DumpCFG` independently emits that cyclic edge for both forms.
Cindergraph retains the cycles. The remaining Algmyr distance is therefore an
adjudicated Joern-side CFG defect; changing Cindergraph to score zero would
delete real control flow.

## Reproduction

The official IOCCC winner repository was pinned at commit
`4e10cbf3187c5d7cd882d10e107de1d56375c70c`. With that checkout available:

```bash
PYTHONPATH="$PWD/python" /path/to/python-with-pyjoern \
  tools/compare_ioccc_cfgs.py \
  --winner-root /path/to/ioccc-winner \
  --output docs/benchmarks/data/ioccc-cfg-comparison-2026-09-15.json
```

The [machine-readable result](data/ioccc-cfg-comparison-2026-09-15.json)
contains the selected paths, expected function sets, per-function graph sizes,
diagnostics, preprocessing status, provenance, timings, tool versions, and
repository hashes. Joern diagnostics are marked unavailable because the
`pyjoern` interface used by this comparison does not expose them.

No IOCCC or DecBench issue, comment, submission, or pull request was created.
