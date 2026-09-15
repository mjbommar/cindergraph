"""Prove duplicated Joern entry roles on weak/ifunc CFGs are artifacts."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import time
from pathlib import Path
from typing import Any

import networkx as nx
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "crates/cindergraph/tests/decompiler_fixtures/src/158_weak_symbols.c": [
        "weak_absent_probe",
        "weak_dispatch",
        "weak_fold",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/159_ifunc_resolver.c": [
        "ifn159_resolve"
    ],
}


def _entries(graph: nx.DiGraph[Any]) -> list[Any]:
    return [node for node in graph if bool(getattr(node, "is_entrypoint", False))]


def adjudicate() -> dict[str, Any]:
    rows = []
    for relative, names in TARGETS.items():
        path = ROOT / relative
        text = path.read_text()
        cinder = cfgs_from_decompiled(text)
        parsed = parse_source(path, no_ddg=True, no_ast=True) or {}
        joern = {function.name: function.cfg for function in parsed.values()}
        for name in names:
            left, right = cinder[name], joern[name]
            left_entries, right_entries = _entries(left), _entries(right)
            rows.append(
                {
                    "path": relative,
                    "function": name,
                    "topology_isomorphic_ignoring_roles": nx.is_isomorphic(left, right),
                    "cindergraph_entry_count": len(left_entries),
                    "cindergraph_entry_indegrees": [
                        left.in_degree(node) for node in left_entries
                    ],
                    "joern_entry_count": len(right_entries),
                    "joern_entry_indegrees": [
                        right.in_degree(node) for node in right_entries
                    ],
                }
            )
    population = b"".join(
        relative.encode() + b"\0" + (ROOT / relative).read_bytes()
        for relative in TARGETS
    )
    status = subprocess.run(
        ["git", "status", "--porcelain=v1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    return {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "local CFG entry-role adjudication; no upstream interaction",
        "provenance": {
            "cindergraph_git_head": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip(),
            "cindergraph_worktree_dirty": bool(status.strip()),
        },
        "population": {
            "translation_units": len(TARGETS),
            "functions": len(rows),
            "sha256": hashlib.sha256(population).hexdigest(),
        },
        "summary": {
            "functions": len(rows),
            "topology_isomorphic_ignoring_roles": sum(
                row["topology_isomorphic_ignoring_roles"] for row in rows
            ),
            "cindergraph_unique_zero_indegree_entry": sum(
                row["cindergraph_entry_count"] == 1
                and row["cindergraph_entry_indegrees"] == [0]
                for row in rows
            ),
            "joern_unique_zero_indegree_entry": sum(
                row["joern_entry_count"] == 1 and row["joern_entry_indegrees"] == [0]
                for row in rows
            ),
        },
        "functions": rows,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = adjudicate()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
