"""Deterministic workload timings; run before/after with the same interpreter.

    uv run python tools/bench_analysis.py

Includes parsing and Python conversion. No claims about peak native memory.
"""

import argparse
import json
import platform
import statistics
import time

import cindergraph as cg


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--shape",
        choices=(
            "functions",
            "statements",
            "globals",
            "parameters",
            "parameters-call",
            "parameter-typedefs",
            "local-typedefs",
            "vla",
            "parameter-vla",
            "parenthesized-parameter-vla",
            "generic-vla",
            "assignment-vla",
            "pointer-copies",
            "pointer-reverse-copies",
            "pointer-results",
            "pointer-accesses",
            "discarded-pointer-assignments",
            "branch-overwrites",
            "branch-accumulation",
            "sizeof",
            "sizeof-pointer",
        ),
        default="functions",
    )
    shape = parser.parse_args().shape
    rows = []
    for count in (8, 32, 128) if shape == "functions" else (32, 128, 512):
        code = "".join(
            f"int f{i}(int x){{int y=x+1;"
            + (f"f{i + 1}(y);" if i + 1 < count else "external(y);")
            + "return y;}"
            for i in range(count)
        )
        if shape == "statements":
            code = "int f(int x){int y=x;" + "y=y+1;" * count + "return y;}"
        elif shape == "sizeof":
            code = "int f(int x){" + "sizeof(x+1);" * count + "return x;}"
        elif shape == "sizeof-pointer":
            code = "int f(int **p){" + "sizeof(*(*(p)));" * count + "return 0;}"
        elif shape == "globals":
            code = (
                "int f(void){"
                + "".join(f"g{i}={i};" for i in range(count))
                + "return g0;}"
            )
        elif shape in ("parameters", "parameters-call"):
            code = (
                "int f(" + ",".join(f"int p{i}" for i in range(count)) + "){return p0;}"
            )
            if shape == "parameters-call":
                code = "int sink(int x){return x;}" + code.replace(
                    "return p0;", "return sink(p0);"
                )
        elif shape == "parameter-typedefs":
            code = "".join(
                f"typedef unsigned long t{i};" for i in range(count)
            ) + "".join(f"int f{i}(t{i}, int x){{return x;}}" for i in range(count))
        elif shape == "local-typedefs":
            code = (
                "int f(void){"
                + "".join(f"typedef unsigned long t{i};" for i in range(count))
                + f"return sizeof(t{count - 1});}}"
            )
        elif shape == "vla":
            code = (
                "int f(int n){"
                + "".join(f"int values{i}[n];" for i in range(count))
                + f"return sizeof(values{count - 1});}}"
            )
        elif shape == "parameter-vla":
            code = (
                "int f(int n,"
                + ",".join(f"int values{i}[n]" for i in range(count))
                + "){return n;}"
            )
        elif shape == "parenthesized-parameter-vla":
            code = (
                "int f(int n,"
                + ",".join(f"int (values{i}[n])" for i in range(count))
                + "){return n;}"
            )
        elif shape == "generic-vla":
            code = (
                "int f(int n){"
                + "".join(
                    f"int values{i}[_Generic((n), int: 4, default: 8)];"
                    for i in range(count)
                )
                + "return 0;}"
            )
        elif shape == "assignment-vla":
            code = (
                "int f(int n){"
                + "".join(f"int values{i}[n = {i + 1}];" for i in range(count))
                + "return n;}"
            )
        elif shape == "pointer-copies":
            code = (
                "int f(void){int x=0;int *p0=&x;"
                + "".join(f"int *p{i}=p{i - 1};" for i in range(1, count))
                + f"*p{count - 1}=1;return x;}}"
            )
        elif shape == "pointer-reverse-copies":
            code = (
                "int f(void){int x=0;"
                + "".join(f"int *p{i};" for i in range(count))
                + "".join(f"p{i}=p{i + 1};" for i in range(count - 1))
                + f"p{count - 1}=&x;*p0=1;return x;}}"
            )
        elif shape == "pointer-results":
            code = (
                "int f(int x){int a=0,b=0;"
                + "".join(f"int *p{i}=(x+{i})?&a:&b;" for i in range(count))
                + f"*p{count - 1}=x;return a;}}"
            )
        elif shape == "pointer-accesses":
            code = (
                "int f(int x){int y=0;int *p=&y;" + "*p=x;y+=*p;" * count + "return y;}"
            )
        elif shape == "discarded-pointer-assignments":
            code = (
                "int f(int x){int y=0;int *p;" + "(p=&y,0);" * count + "*p=x;return y;}"
            )
        elif shape == "branch-overwrites":
            code = (
                "int f(int x){int y=0;"
                + "".join(f"if(x)y={i};else y={i + 1};" for i in range(count))
                + "return y;}"
            )
        elif shape == "branch-accumulation":
            code = (
                "int f(int x){int y=0;"
                + "".join(f"if(x)y={i};" for i in range(count))
                + "return y;}"
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
                    shape=shape,
                    size=count,
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
