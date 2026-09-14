# Cindergraph

Cindergraph is a tolerant, deterministic C source-analysis library implemented
in Rust, with native Rust and Python APIs. It parses ordinary and
decompiler-shaped C, measures functions, builds control and dependence graphs,
and reports partial results with diagnostics instead of losing an entire file.

It originated in [Glaurung](https://github.com/mjbommar/glaurung). Cindergraph
is not a code property graph, a Joern distribution, or a promise that arbitrary
Joern queries will run unchanged.

The project is pre-alpha while the history-preserving extraction and Glaurung
dependency inversion are completed.

## Python

```python
import cindergraph

report = cindergraph.analyze("int answer(void) { return 42; }")
print(report.functions[0].name)
```

## Rust

```rust
let report = cindergraph::metrics::analyze("int answer(void) { return 42; }")
    .into_parts()
    .0;
assert_eq!(report.functions[0].name, "answer");
```

Licensed under Apache-2.0.
