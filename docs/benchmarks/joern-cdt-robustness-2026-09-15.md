# Joern and Eclipse CDT robustness comparison — 2026-09-15

This is a local, dirty-tree observation over robustness manifest schema 1,
SHA-256 `49bc22e83931262b21a96a0c427e2e741f9173957969735aec85656fa2e1307a`
(525 specimens). It is not a publishable benchmark record and involved no
DecBench upstream interaction.

## Installed tools

- Joern 4.0.628, official Linux x86-64 distribution in
  `/opt/joern-v4.0.628`; Joern declares `eclipse-cdt-core`
  `9.2.100.202507101054+1` for its C frontend.
- Eclipse CDT 12.6.0, official 2026-09 C/C++ Eclipse package in
  `/opt/eclipse-cdt-12.6.0`.

Each specimen ran in a fresh process and temporary workspace. Function yield
means a named function definition emitted by the tool. It does not establish
CFG, binding, type, or data-flow correctness.

| Population | Cindergraph | Tree-sitter | Clang | CDT 12.6 | Joern 4.0.628 |
|---|---:|---:|---:|---:|---:|
| Clean function oracle | 930/930 | 930/930 | 930/930 | 930/930 | 930/930 |
| Decompiler name oracle | 25/25 | 20/25 | 13/25 | 11/25 | 15/25 |
| Controlled unaffected neighbours | 63/63 | 59/63 | 52/63 | 60/63 | 60/63 |
| Random damage: zero-function specimens | 28/256 | 57/256 | 51/256 | 48/256 | 49/256* |

`*` Joern had 255 successful random-damage rows and one fail-closed adapter
serialization failure (`mutation/seed-010`): Joern emitted a function name
containing a raw control character that the minimal JSON adapter did not
escape. This is not classified as a Joern parse failure.

### What the random-damage row does and does not mean

The 256 deterministic cases select one of 211 real fixtures, retain at most
the first 8 KiB, and apply four random replacements. Each replacement deletes
zero to eleven characters and inserts one of: NUL, U+FFFE, lambda, CRLF, a
quote, `/*`, `}`, `(`, or `#if X`. Every third seed also truncates the source at
a random prefix. This population's oracle is only totality and determinism;
it has no independently labelled surviving-function oracle. A zero-function
result can therefore be correct when truncation removes all definitions, and a
non-zero result can still be a false or semantically damaged recovery.

| Random-damage stratum | Cindergraph | Tree-sitter | Clang | CDT 12.6 | Joern 4.0.628 |
|---|---:|---:|---:|---:|---:|
| No prefix truncation: zero yield | **6/170** | 11/170 | 20/170 | 18/170 | 19/170* |
| Random prefix truncation: zero yield | **22/86** | 46/86 | 31/86 | 30/86 | 30/86 |

These counts support a narrow statement about totality and retained function
yield. They must not be described as semantic accuracy. The controlled-damage
population is the stronger recovery comparison because it names unaffected
left and right guards before introducing damage into a separate target.

Both CDT and Joern lose the right-hand guard in all three unterminated block
comment cases. Cindergraph's 63/63 result comes from its explicitly
recovery-qualified, conservative top-level resynchronisation. Joern's four
additional decompiler-name recoveries over standalone CDT demonstrate that
CPG construction and the embedded fork are not equivalent to current raw CDT.

Local result artifacts:

- `target/tmp/robustness-cdt-12.6.0/`
- `target/tmp/robustness-joern-v4.0.628/`

The conclusion is narrow: Cindergraph leads this fixed function-recovery
population. It does not imply broad Pareto dominance over CDT's compiler-grade
semantic model or Joern's mature CPG query and interprocedural analysis stack.
