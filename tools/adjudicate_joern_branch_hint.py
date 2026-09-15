"""Compare source-equivalent hinted and unhinted CFGs."""

from __future__ import annotations

import argparse
import json
import time
from pathlib import Path
from typing import Any

import networkx as nx
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "crates/cindergraph/tests/decompiler_fixtures/src/119_branch_hints.c"


def _normalized(graph: nx.DiGraph[Any]) -> nx.DiGraph[int]:
    result: nx.DiGraph[int] = nx.DiGraph()
    ids = {node: index for index, node in enumerate(graph.nodes)}
    result.add_nodes_from(
        (
            ids[node],
            {
                "role": (
                    bool(getattr(node, "is_entrypoint", False)),
                    bool(getattr(node, "is_exitpoint", False)),
                )
            },
        )
        for node in graph.nodes
    )
    result.add_edges_from((ids[a], ids[b]) for a, b in graph.edges)
    return result


def _same(left: nx.DiGraph[Any], right: nx.DiGraph[Any]) -> bool:
    return nx.is_isomorphic(
        _normalized(left),
        _normalized(right),
        node_match=lambda a, b: a["role"] == b["role"],
    )


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    text = FIXTURE.read_text()
    cinder = cfgs_from_decompiled(text)
    parsed = parse_source(FIXTURE, no_ddg=True, no_ast=True) or {}
    joern = {function.name: function.cfg for function in parsed.values()}
    hinted = "hinted_validation"
    control = "unhinted_validation"
    result = {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "source-equivalent __builtin_expect adjudication; no upstream interaction",
        "semantic_basis": (
            "__builtin_expect returns its first argument and changes branch prediction, "
            "not C control-flow alternatives"
        ),
        "cindergraph": {
            "hinted_nodes": cinder[hinted].number_of_nodes(),
            "hinted_edges": cinder[hinted].number_of_edges(),
            "control_nodes": cinder[control].number_of_nodes(),
            "control_edges": cinder[control].number_of_edges(),
            "role_isomorphic": _same(cinder[hinted], cinder[control]),
        },
        "joern": {
            "hinted_nodes": joern[hinted].number_of_nodes(),
            "hinted_edges": joern[hinted].number_of_edges(),
            "control_nodes": joern[control].number_of_nodes(),
            "control_edges": joern[control].number_of_edges(),
            "role_isomorphic": _same(joern[hinted], joern[control]),
        },
        "providers_on_unhinted_control_role_isomorphic": _same(
            cinder[control], joern[control]
        ),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
