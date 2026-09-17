# Defect conformance corpus

What a semantic consumer must be able to see in the export. Item 15 of
[`docs/improvement-list-2026-09-16.md`](../../../docs/improvement-list-2026-09-16.md).

`01_*.c` through `08_*.c` are verbatim copies of the eight annotated
defect/fix samples from the Axeyum repository,
`python/examples/cindergraph_defects/samples/*.c` (copied 2026-09-17; that
directory is the source of record and is not edited from here). Each file has
a textbook C defect and its fixed twin side by side, `// expect:` lines naming
which function has a finding and which is clean, and `// axeyum:` capacity
annotations the solver front end reads. Axeyum's consumer lifts every
loop-free function to QF_BV from this export, replays every witness under a
sanitizer, and reported 12 replayed witnesses, 2 dead branches and 8 clean
functions on them --- so a regression in what the export publishes is a
regression in that result, which the metrics tests cannot see.

`09_loop_bounds.c` was written here, not copied: none of the eight Axeyum
samples contains a loop (the consumer refuses loops), and the loop-header
assertions would be vacuous without one.

The test is `python/tests/test_defect_conformance.py`. For every function in
every sample it asserts, from the `ast`, `cfg` and `ops` exports of one
`AnalysisSession`:

- every `binary_expr`, `assign_expr` and `unary_expr` carries `op`;
- every `name_ref` to a parameter or a local carries a `type` that is not
  `unknown` (and no pointer or array over `unknown`);
- every `if_stmt` condition is a CFG `cond` node with `expr_internal` false;
- every `for`/`while`/`do` has a `loop_header` carrying `bound_kind`, and
  the loop sample's headers classify as the comments beside them say;
- every `param_decl` carries `type`, `pointer_depth` and `name`;
- every `ops` operation carries a `type`, and no `ops` node is `unknown`
  for a reason other than the documented ones.

The samples include `<stddef.h>` and `<stdint.h>`, so `size_t` and `uint32_t`
are typedefs the file never declares and would export as `unknown`. The test
prepends `typedef unsigned long size_t;` and `typedef unsigned int uint32_t;`
(LP64) to the text and analyses it with the **`ordinary`** dialect. It does
not use the `preprocessed` dialect: that dialect is for gcc `.i` output and
keeps only lines under a `# <line> "<file>"` marker naming a project file, so
text without markers --- these samples, typedefs included --- is dropped
entirely (zero functions; measured 2026-09-17). Every span the test reads is
therefore offset by the prepended prefix, and the test reads spans from the
prefixed text it analysed, never from the file.
