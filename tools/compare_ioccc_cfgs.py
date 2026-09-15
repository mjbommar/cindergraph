"""Compare Cindergraph and Joern on an adjudicated IOCCC source sample.

This is an offline research runner. It reads an existing sparse checkout of the
official ``ioccc-src/winner`` repository and writes one JSON artifact; it does
not download tools, modify either upstream, or interact with DecBench.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import json
import platform
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from decbench.metrics.vj_ged import vj_ged
from pyjoern import parse_source

from cindergraph.source_cfg import analyze_decompiled


ROOT = Path(__file__).resolve().parents[1]

# Expected definitions were adjudicated from compiler-preprocessed source with
# ``clang -std=gnu89 -fsyntax-only -Xclang -ast-dump=json``. This is a fixed
# evaluation population, not names returned by either provider under test.
CASES: dict[str, tuple[str, frozenset[str]]] = {
    "1984/mullender": ("1984/mullender/mullender.c", frozenset()),
    "1985/applin": ("1985/applin/applin.c", frozenset({"main"})),
    "1986/wall": ("1986/wall/wall.c", frozenset({"cc", "ccc", "main"})),
    "1987/korn": ("1987/korn/korn.c", frozenset({"main"})),
    "1988/westley": ("1988/westley/westley.c", frozenset({"F_OO", "main"})),
    "1990/theorem": (
        "1990/theorem/theorem.c",
        frozenset({"K", "L", "P", "S", "e", "main", "w"}),
    ),
    "1992/adrian": (
        "1992/adrian/adrian.c",
        frozenset({"A", "E", "N", "O", "main"}),
    ),
    "1995/vanschnitz": ("1995/vanschnitz/vanschnitz.c", frozenset({"main"})),
    "2001/anonymous": (
        "2001/anonymous/anonymous.c",
        frozenset(
            {
                "E",
                "K",
                "L",
                "Li",
                "Run",
                "Runi",
                "Sca",
                "Scan",
                "V",
                "main",
                "pain",
                "ru",
                "run",
            }
        ),
    ),
    "2005/anon": ("2005/anon/anon.c", frozenset({"main"})),
    "2011/akari": ("2011/akari/akari.c", frozenset({"main"})),
    "2018/algmyr": (
        "2018/algmyr/prog.c",
        frozenset({"C", "F", "T", "d", "f", "main"}),
    ),
    "2020/carlini": ("2020/carlini/prog.c", frozenset({"main"})),
    "2025/diels-grabsch": ("2025/diels-grabsch/prog.c", frozenset({"main"})),
    "2025/kurdyukov": ("2025/kurdyukov/prog.c", frozenset({"main"})),
}


def _sha256(parts: list[bytes]) -> str:
    digest = hashlib.sha256()
    for part in parts:
        digest.update(len(part).to_bytes(8, "big"))
        digest.update(part)
    return digest.hexdigest()


def _graph_stats(graph: Any) -> dict[str, int | bool]:
    nodes = list(graph.nodes)
    entries = sum(bool(getattr(node, "is_entrypoint", False)) for node in nodes)
    exits = sum(bool(getattr(node, "is_exitpoint", False)) for node in nodes)
    return {
        "nodes": len(nodes),
        "edges": len(graph.edges),
        "entry_nodes": entries,
        "exit_nodes": exits,
        "has_single_entry": entries == 1,
    }


def _accuracy(found: set[str], expected: frozenset[str]) -> dict[str, Any]:
    true_positive = len(found & expected)
    false_positive = len(found - expected)
    false_negative = len(expected - found)
    return {
        "true_positive": true_positive,
        "false_positive": false_positive,
        "false_negative": false_negative,
        "precision": true_positive / len(found) if found else float(not expected),
        "recall": true_positive / len(expected) if expected else float(not found),
    }


def _joern(text: str) -> tuple[dict[str, Any], str | None, float]:
    started = time.perf_counter()
    error = None
    graphs: dict[str, Any] = {}
    try:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".c") as source:
            source.write(text)
            source.flush()
            parsed = parse_source(Path(source.name), no_ddg=True, no_ast=True)
        if parsed is not None:
            graphs = {
                function.name: function.cfg
                for function in parsed.values()
                if getattr(function, "cfg", None) is not None
            }
    except Exception as exception:  # noqa: BLE001 - failures are measurements
        error = f"{type(exception).__name__}: {exception}"
    return graphs, error, time.perf_counter() - started


def _provider_record(
    graphs: dict[str, Any], expected: frozenset[str], seconds: float
) -> dict[str, Any]:
    found = set(graphs)
    return {
        "functions": sorted(found),
        "accuracy": _accuracy(found, expected),
        "seconds": seconds,
        "cfgs": {name: _graph_stats(graph) for name, graph in sorted(graphs.items())},
    }


def compare(winner_root: Path) -> dict[str, Any]:
    rows = []
    source_parts: list[bytes] = []
    for index, (case_id, (relative, expected)) in enumerate(CASES.items(), 1):
        print(f"[{index}/{len(CASES)}] {case_id}", flush=True)
        path = winner_root / relative
        text = path.read_text()
        source_parts.append(case_id.encode() + b"\0" + text.encode())

        started = time.perf_counter()
        cinder = analyze_decompiled(text)
        cinder_seconds = time.perf_counter() - started
        raw_joern, raw_error, raw_seconds = _joern(text)
        prepared_joern, prepared_error, prepared_seconds = _joern(
            cinder.preprocessing.text
        )

        cinder_record = _provider_record(cinder.graphs, expected, cinder_seconds)
        cinder_record["diagnostics"] = {
            "errors": sum(item["severity"] == "error" for item in cinder.diagnostics),
            "warnings": sum(
                item["severity"] == "warning" for item in cinder.diagnostics
            ),
            "messages": [item["message"] for item in cinder.diagnostics],
        }
        cinder_record["preprocessing"] = {
            "status": cinder.preprocessing.status,
            "compiler": cinder.preprocessing.compiler,
            "command": list(cinder.preprocessing.command),
            "stderr": cinder.preprocessing.stderr,
            "includes_removed": cinder.preprocessing.includes_removed,
        }
        cinder_record["provenance"] = {
            name: {
                "origin": provenance.origin,
                "recovery_qualified": provenance.recovery_qualified,
                "diagnostic_count": provenance.diagnostic_count,
                "start": provenance.start,
                "end": provenance.end,
            }
            for name, provenance in sorted(cinder.provenance.items())
        }

        raw_record = _provider_record(raw_joern, expected, raw_seconds)
        raw_record["error"] = raw_error
        raw_record["diagnostics"] = "unavailable through pyjoern"
        prepared_record = _provider_record(prepared_joern, expected, prepared_seconds)
        prepared_record["error"] = prepared_error
        prepared_record["diagnostics"] = "unavailable through pyjoern"

        shared = sorted(expected & set(cinder.graphs) & set(prepared_joern))
        rows.append(
            {
                "id": case_id,
                "source": relative,
                "expected_functions": sorted(expected),
                "cindergraph": cinder_record,
                "joern_raw": raw_record,
                "joern_prepared": prepared_record,
                "prepared_shared_vj_ged": {
                    name: float(vj_ged(cinder.graphs[name], prepared_joern[name]))
                    for name in shared
                },
            }
        )

    def totals(provider: str) -> dict[str, Any]:
        records = [row[provider] for row in rows]
        tp = sum(record["accuracy"]["true_positive"] for record in records)
        fp = sum(record["accuracy"]["false_positive"] for record in records)
        fn = sum(record["accuracy"]["false_negative"] for record in records)
        times = [record["seconds"] for record in records]
        return {
            "true_positive": tp,
            "false_positive": fp,
            "false_negative": fn,
            "micro_precision": tp / (tp + fp) if tp + fp else 1.0,
            "micro_recall": tp / (tp + fn) if tp + fn else 1.0,
            "crashes": sum(bool(record.get("error")) for record in records),
            "total_seconds": sum(times),
            "median_seconds": statistics.median(times),
        }

    distances = [
        distance for row in rows for distance in row["prepared_shared_vj_ged"].values()
    ]
    return {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "15 adjudicated IOCCC C sources; raw and shared-preprocessing Joern lanes",
        "provenance": {
            "ioccc_git_head": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=winner_root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip(),
            "cindergraph_git_head": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip(),
            "cindergraph_worktree_dirty": bool(
                subprocess.run(
                    ["git", "status", "--porcelain=v1"],
                    cwd=ROOT,
                    check=True,
                    capture_output=True,
                    text=True,
                ).stdout
            ),
            "python": sys.version,
            "platform": platform.platform(),
            "pyjoern": importlib.metadata.version("pyjoern"),
            "decbench": importlib.metadata.version("decbench"),
        },
        "population": {
            "cases": len(rows),
            "expected_functions": sum(len(expected) for _, expected in CASES.values()),
            "sha256": _sha256(source_parts),
        },
        "summary": {
            "cindergraph": totals("cindergraph"),
            "joern_raw": totals("joern_raw"),
            "joern_prepared": totals("joern_prepared"),
            "shared_cfgs": len(distances),
            "shared_vj_ged_zero": sum(distance == 0 for distance in distances),
            "shared_vj_ged_nonzero": sum(distance != 0 for distance in distances),
            "shared_vj_ged_total": sum(distances),
        },
        "cases": rows,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--winner-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = compare(args.winner_root.resolve())
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
