"""Deterministic workload timings; run before/after with the same interpreter.

    uv run python tools/bench_analysis.py

Includes parsing and Python conversion. No claims about peak native memory.
"""

import json
import platform
import statistics
import time

import cindergraph as cg


def main():
    rows = []
    for count in (8, 32, 128):
        code = "".join(
            f"int f{i}(int x){{int y=x+1;"
            + (f"f{i + 1}(y);" if i + 1 < count else "external(y);")
            + "return y;}"
            for i in range(count)
        )
        for name in ("analyze", "data_flow", "call_summaries"):
            operation = getattr(cg, name)
            operation(code)
            samples = []
            for _ in range(7):
                start = time.perf_counter_ns()
                for _ in range(5):
                    operation(code)
                samples.append((time.perf_counter_ns() - start) / 5 / 1e6)
            rows.append(
                dict(
                    operation=name,
                    functions=count,
                    source_bytes=len(code.encode()),
                    median_ms=statistics.median(samples),
                    min_ms=min(samples),
                )
            )
    print(
        json.dumps(
            dict(
                python=platform.python_version(),
                extension=cg._native.__file__,
                timings=rows,
            ),
            indent=2,
        )
    )


if __name__ == "__main__":
    main()
