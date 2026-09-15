"""Compare Cindergraph and Joern on the local DecBench-adjacent C corpus.

This invokes pyjoern locally. It does not run the DecBench pipeline, alter its
checkout, publish results, or interact with the upstream project.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import platform
import signal
import statistics
import subprocess
import sys
import time
from collections import Counter
from pathlib import Path
from typing import Any

import networkx as nx
from decbench.metrics.vj_ged import vj_ged
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_CORPORA = (
    ROOT / "crates/cindergraph/tests/decompiler_fixtures/src",
    ROOT / "tests/decbench_corpus/src",
)


def _sha256(parts: list[bytes]) -> str:
    digest = hashlib.sha256()
    for part in parts:
        digest.update(len(part).to_bytes(8, "big"))
        digest.update(part)
    return digest.hexdigest()


def _source_digest() -> str:
    paths = sorted(
        path
        for base in (ROOT / "crates", ROOT / "python/cindergraph")
        for path in base.rglob("*")
        if path.is_file() and path.suffix in {".rs", ".py", ".toml"}
    )
    return _sha256(
        [
            str(path.relative_to(ROOT)).encode() + b"\0" + path.read_bytes()
            for path in paths
        ]
    )


def _files(corpora: list[Path]) -> list[Path]:
    return sorted({path.resolve() for corpus in corpora for path in corpus.glob("*.c")})


def _role(node: object) -> tuple[bool, bool]:
    return (
        bool(getattr(node, "is_entrypoint", False)),
        bool(getattr(node, "is_exitpoint", False)),
    )


def _role_graph(graph: nx.DiGraph[Any]) -> nx.DiGraph[int]:
    result: nx.DiGraph[int] = nx.DiGraph()
    identities = {node: index for index, node in enumerate(graph.nodes)}
    for node, index in identities.items():
        result.add_node(index, role=_role(node))
    result.add_edges_from(
        (identities[src], identities[dst]) for src, dst in graph.edges
    )
    return result


class _IsomorphismTimeout(Exception):
    pass


def _isomorphic(left: nx.DiGraph[Any], right: nx.DiGraph[Any]) -> bool | None:
    def expired(_signum: int, _frame: object) -> None:
        raise _IsomorphismTimeout

    previous = signal.signal(signal.SIGALRM, expired)
    signal.setitimer(signal.ITIMER_REAL, 2.0)
    try:
        return nx.is_isomorphic(
            _role_graph(left),
            _role_graph(right),
            node_match=lambda a, b: a["role"] == b["role"],
        )
    except _IsomorphismTimeout:
        return None
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def _describe(graph: nx.DiGraph[Any]) -> dict[str, Any]:
    roles = Counter(_role(node) for node in graph.nodes)
    return {
        "nodes": graph.number_of_nodes(),
        "edges": graph.number_of_edges(),
        "entry_nodes": roles[(True, False)] + roles[(True, True)],
        "exit_nodes": roles[(False, True)] + roles[(True, True)],
    }


def _joern(path: Path) -> dict[str, nx.DiGraph[Any]]:
    parsed = parse_source(path, no_ddg=True, no_ast=True)
    if parsed is None:
        return {}
    return {
        function.name: function.cfg
        for function in parsed.values()
        if getattr(function, "cfg", None) is not None
    }


def _cindergraph(path: Path) -> dict[str, nx.DiGraph[Any]]:
    return cfgs_from_decompiled(path.read_text(errors="replace"))


def _version(distribution: str) -> str | None:
    try:
        return importlib.metadata.version(distribution)
    except importlib.metadata.PackageNotFoundError:
        return None


def _percentile(values: list[float], fraction: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    return ordered[round((len(ordered) - 1) * fraction)]


def _timing_summary(values: list[float]) -> dict[str, float | int | None]:
    return {
        "count": len(values),
        "total_seconds": sum(values),
        "median_seconds": statistics.median(values) if values else None,
        "p95_seconds": _percentile(values, 0.95),
        "min_seconds": min(values) if values else None,
        "max_seconds": max(values) if values else None,
    }


def _git(args: list[str], cwd: Path) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


def compare(paths: list[Path]) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    cinder_times: list[float] = []
    joern_times: list[float] = []
    for index, path in enumerate(paths, 1):
        row: dict[str, Any] = {"path": str(path.relative_to(ROOT))}
        print(f"[{index}/{len(paths)}] {row['path']}", file=sys.stderr, flush=True)
        providers: dict[str, dict[str, nx.DiGraph[Any]]] = {}
        for name, provider, timings in (
            ("cindergraph", _cindergraph, cinder_times),
            ("joern", _joern, joern_times),
        ):
            started = time.perf_counter()
            try:
                providers[name] = provider(path)
                row[f"{name}_error"] = None
            except Exception as error:  # noqa: BLE001 - benchmark records provider failure
                providers[name] = {}
                row[f"{name}_error"] = f"{type(error).__name__}: {error}"
            elapsed = time.perf_counter() - started
            timings.append(elapsed)
            row[f"{name}_seconds"] = elapsed

        cinder = providers["cindergraph"]
        joern = providers["joern"]
        cinder_names = set(cinder)
        joern_names = set(joern)
        shared = sorted(cinder_names & joern_names)
        row.update(
            cindergraph_functions=sorted(cinder_names),
            joern_functions=sorted(joern_names),
            only_cindergraph=sorted(cinder_names - joern_names),
            only_joern=sorted(joern_names - cinder_names),
            shared_functions=shared,
        )
        functions = []
        for name in shared:
            left = cinder[name]
            right = joern[name]
            functions.append(
                {
                    "name": name,
                    "cindergraph": _describe(left),
                    "joern": _describe(right),
                    "vj_ged": float(vj_ged(left, right)),
                    "role_preserving_isomorphic": _isomorphic(left, right),
                }
            )
        row["functions"] = functions
        rows.append(row)

    shared_rows = [function for row in rows for function in row["functions"]]
    cinder_names = {
        (row["path"], name) for row in rows for name in row["cindergraph_functions"]
    }
    joern_names = {
        (row["path"], name) for row in rows for name in row["joern_functions"]
    }
    perfect = sum(function["vj_ged"] == 0 for function in shared_rows)
    isomorphic = sum(
        function["role_preserving_isomorphic"] is True for function in shared_rows
    )
    nonisomorphic = sum(
        function["role_preserving_isomorphic"] is False for function in shared_rows
    )
    isomorphism_timeouts = sum(
        function["role_preserving_isomorphic"] is None for function in shared_rows
    )
    population_parts = [
        str(path.relative_to(ROOT)).encode() + b"\0" + path.read_bytes()
        for path in paths
    ]
    decbench_spec = importlib.util.find_spec("decbench")
    if decbench_spec is None or decbench_spec.origin is None:
        raise RuntimeError("cannot locate the imported DecBench checkout")
    decbench_root = Path(decbench_spec.origin).resolve().parents[1]
    status = _git(["status", "--porcelain=v1"], ROOT)
    return {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "local source-C CFG extraction used by DecBench GED; no upstream interaction",
        "provenance": {
            "cindergraph_git_head": _git(["rev-parse", "HEAD"], ROOT),
            "cindergraph_worktree_dirty": bool(status),
            "cindergraph_status_sha256": _sha256([status.encode()]),
            "cindergraph_source_sha256": _source_digest(),
            "decbench_git_head": _git(["rev-parse", "HEAD"], decbench_root),
            "python": sys.version,
            "platform": platform.platform(),
            "pyjoern": _version("pyjoern"),
            "decbench": _version("decbench"),
            "cfgutils": _version("cfgutils"),
            "networkx": _version("networkx"),
        },
        "population": {
            "files": len(paths),
            "sha256": _sha256(population_parts),
            "roots": [str(path.relative_to(ROOT)) for path in DEFAULT_CORPORA],
        },
        "summary": {
            "cindergraph_function_instances": len(cinder_names),
            "joern_function_instances": len(joern_names),
            "shared_function_instances": len(cinder_names & joern_names),
            "only_cindergraph_instances": len(cinder_names - joern_names),
            "only_joern_instances": len(joern_names - cinder_names),
            "shared_vj_ged_zero": perfect,
            "shared_vj_ged_nonzero": len(shared_rows) - perfect,
            "shared_role_preserving_isomorphic": isomorphic,
            "shared_not_role_preserving_isomorphic": nonisomorphic,
            "shared_isomorphism_timeouts": isomorphism_timeouts,
            "cindergraph_failures": sum(
                row["cindergraph_error"] is not None for row in rows
            ),
            "joern_failures": sum(row["joern_error"] is not None for row in rows),
            "cindergraph_timing": _timing_summary(cinder_times),
            "joern_timing": _timing_summary(joern_times),
        },
        "files": rows,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", action="append", type=Path)
    parser.add_argument(
        "--path",
        action="append",
        type=Path,
        help="compare an explicit C file; repeat for a reduced regression set",
    )
    parser.add_argument("--limit", type=int)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    corpora = (
        [path.resolve() for path in args.corpus]
        if args.corpus
        else list(DEFAULT_CORPORA)
    )
    paths = (
        sorted({path.resolve() for path in args.path}) if args.path else _files(corpora)
    )
    if args.limit is not None:
        paths = paths[: args.limit]
    result = compare(paths)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
