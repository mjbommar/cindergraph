"""Measure native graph construction/traversal against optional NetworkX."""

from __future__ import annotations

import argparse
import gc
import json
import statistics
import time
from collections.abc import Callable
from typing import Any

import cindergraph as cg


def median_ms(operation: Callable[[], Any], repetitions: int) -> tuple[float, Any]:
    """Return median wall time and the final result."""
    samples = []
    result = None
    for _ in range(repetitions):
        gc.collect()
        started = time.perf_counter()
        result = operation()
        samples.append((time.perf_counter() - started) * 1_000)
    return statistics.median(samples), result


def main() -> int:
    """Build a deterministic large CFG and print comparable timings."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--statements", type=int, default=20_000)
    parser.add_argument("--repeat", type=int, default=7)
    args = parser.parse_args()
    if args.statements < 1 or args.repeat < 1:
        parser.error("--statements and --repeat must be positive")

    code = (
        "int f(int x){"
        + "".join(f"x=x+{index % 7};" for index in range(args.statements))
        + "return x;}"
    )
    session = cg.AnalysisSession(code)
    native_ms, graphs = median_ms(session.native_graphs, args.repeat)
    graph = graphs[0]
    native_walk_ms, reached = median_ms(lambda: graph.descendants(0), args.repeat)
    measurements: dict[str, int | float | str] = {
        "profile": "release extension; warm session; median wall time",
        "statements": args.statements,
        "nodes": graph.node_count,
        "edges": graph.edge_count,
        "native_view_ms": round(native_ms, 3),
        "native_descendants_ms": round(native_walk_ms, 3),
        "native_reached": len(reached),
    }

    try:
        import networkx as nx
    except ImportError:
        measurements["networkx"] = "not installed"
    else:
        nx_build_ms, nx_graph = median_ms(graph.to_networkx, args.repeat)
        nx_walk_ms, nx_reached = median_ms(
            lambda: nx.descendants(nx_graph, 0), args.repeat
        )
        json_ms, rendered = median_ms(
            lambda: session.export_graphs(repr="cfg", format="json"), args.repeat
        )
        json_load_ms, _ = median_ms(
            lambda: nx.node_link_graph(json.loads(rendered[0][1])), args.repeat
        )
        measurements.update(
            networkx_from_native_ms=round(nx_build_ms, 3),
            networkx_descendants_ms=round(nx_walk_ms, 3),
            networkx_reached=len(nx_reached),
            json_export_ms=round(json_ms, 3),
            networkx_from_json_ms=round(json_load_ms, 3),
        )
    print(json.dumps(measurements, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
