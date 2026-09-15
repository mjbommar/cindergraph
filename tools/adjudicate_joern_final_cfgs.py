"""Adjudicate the final computed-goto and conditional-store CFG differences.

This is a local evidence generator. It invokes Joern through pyjoern, writes no
upstream state, and records enough labelled topology to make both conclusions
independently reviewable.
"""

from __future__ import annotations

import argparse
import hashlib
import html
import json
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

import networkx as nx
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
COMPUTED = ROOT / "crates/cindergraph/tests/decompiler_fixtures/src/103_computed_goto.c"
DECIMAL = (
    ROOT / "crates/cindergraph/tests/decompiler_fixtures/src/45_string_algorithms.c"
)
STORE = "*value = (int32_t)(negative ? -accumulator : accumulator);"
SEQUENCED_STORE = (
    "int32_t selected = (int32_t)(negative ? -accumulator : accumulator);\n"
    "    *value = selected;"
)


def _role(node: object) -> tuple[bool, bool]:
    return (
        bool(getattr(node, "is_entrypoint", False)),
        bool(getattr(node, "is_exitpoint", False)),
    )


def _normalized(graph: nx.DiGraph[Any]) -> nx.DiGraph[int]:
    ids = {node: index for index, node in enumerate(graph.nodes)}
    result: nx.DiGraph[int] = nx.DiGraph()
    result.add_nodes_from((ids[node], {"role": _role(node)}) for node in graph.nodes)
    result.add_edges_from((ids[src], ids[dst]) for src, dst in graph.edges)
    return result


def _same(left: nx.DiGraph[Any], right: nx.DiGraph[Any]) -> bool:
    return nx.is_isomorphic(
        _normalized(left),
        _normalized(right),
        node_match=lambda a, b: a["role"] == b["role"],
    )


def _shape(graph: nx.DiGraph[Any]) -> dict[str, Any]:
    ids = {node: index for index, node in enumerate(graph.nodes)}
    return {
        "nodes": graph.number_of_nodes(),
        "edges": graph.number_of_edges(),
        "node_degrees": [
            {
                "id": ids[node],
                "in": graph.in_degree(node),
                "out": graph.out_degree(node),
                "entry": _role(node)[0],
                "exit": _role(node)[1],
            }
            for node in graph.nodes
        ],
        "edge_list": [[ids[src], ids[dst]] for src, dst in graph.edges],
    }


def _statements(node: object) -> list[str]:
    return [
        html.unescape(repr(statement)) for statement in getattr(node, "statements", [])
    ]


def _joern(path: Path, function: str) -> nx.DiGraph[Any]:
    parsed = parse_source(path, no_ddg=True, no_ast=True) or {}
    graphs = {
        item.name: item.cfg
        for item in parsed.values()
        if getattr(item, "cfg", None) is not None
    }
    return graphs[function]


def _computed_goto() -> dict[str, Any]:
    text = COMPUTED.read_text()
    cinder = cfgs_from_decompiled(text)["threaded_interpreter"]
    joern = _joern(COMPUTED, "threaded_interpreter")
    orphans = [
        node
        for node in joern.nodes
        if joern.in_degree(node) == 0 and not _role(node)[0]
    ]
    reduced = joern.copy()
    reduced.remove_nodes_from(orphans)
    cinder_dispatches = [node for node in cinder if cinder.out_degree(node) == 4]
    joern_dispatches = [node for node in joern if joern.out_degree(node) == 4]
    result = {
        "path": str(COMPUTED.relative_to(ROOT)),
        "function": "threaded_interpreter",
        "semantic_basis": (
            "GNU computed goto may reach exactly the four labels whose addresses "
            "initialize targets; it has no ordinary fall-through successor"
        ),
        "cindergraph": _shape(cinder),
        "joern": _shape(joern),
        "cindergraph_four_way_dispatch_nodes": len(cinder_dispatches),
        "joern_four_way_dispatch_nodes": len(joern_dispatches),
        "joern_non_entry_zero_indegree_nodes": [
            {
                "statements": _statements(node),
                "out_degree": joern.out_degree(node),
            }
            for node in orphans
        ],
        "role_isomorphic_after_removing_only_joern_orphans": _same(cinder, reduced),
    }
    assert len(orphans) == 1
    assert any("targets[opcode]" in item for item in _statements(orphans[0]))
    assert len(cinder_dispatches) == len(joern_dispatches) == 1
    assert result["role_isomorphic_after_removing_only_joern_orphans"]
    return result


def _conditional_store() -> dict[str, Any]:
    text = DECIMAL.read_text()
    assert text.count(STORE) == 1
    sequenced = text.replace(STORE, SEQUENCED_STORE)
    cinder_raw = cfgs_from_decompiled(text)["parse_decimal"]
    joern_raw = _joern(DECIMAL, "parse_decimal")
    with tempfile.NamedTemporaryFile(
        "w", suffix=".c", dir=Path.home() / ".cache/glaurung/tmp", delete=False
    ) as handle:
        handle.write(sequenced)
        transformed_path = Path(handle.name)
    try:
        joern_sequenced = _joern(transformed_path, "parse_decimal")
    finally:
        transformed_path.unlink()
    cinder_sequenced = cfgs_from_decompiled(sequenced)["parse_decimal"]
    result = {
        "path": str(DECIMAL.relative_to(ROOT)),
        "function": "parse_decimal",
        "semantic_basis": (
            "the address computation for *value is side-effect-free and non-volatile; "
            "C leaves assignment-operand evaluation order unspecified, so sequencing "
            "the conditional value before that store preserves this function's control flow"
        ),
        "transformation": {"from": STORE, "to": SEQUENCED_STORE},
        "raw": {"cindergraph": _shape(cinder_raw), "joern": _shape(joern_raw)},
        "sequenced": {
            "cindergraph": _shape(cinder_sequenced),
            "joern": _shape(joern_sequenced),
        },
        "cindergraph_raw_vs_sequenced_role_isomorphic": _same(
            cinder_raw, cinder_sequenced
        ),
        "providers_on_sequenced_control_role_isomorphic": _same(
            cinder_sequenced, joern_sequenced
        ),
    }
    assert result["cindergraph_raw_vs_sequenced_role_isomorphic"]
    assert result["providers_on_sequenced_control_role_isomorphic"]
    assert cinder_raw.number_of_nodes() == joern_raw.number_of_nodes() - 1
    assert cinder_raw.number_of_edges() == joern_raw.number_of_edges() - 1
    return result


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    status = subprocess.run(
        ["git", "status", "--porcelain=v1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    population = b"".join(path.read_bytes() for path in (COMPUTED, DECIMAL))
    result = {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "local final CFG-difference adjudication; no upstream interaction",
        "provenance": {
            "cindergraph_git_head": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip(),
            "cindergraph_worktree_dirty": bool(status.strip()),
            "population_sha256": hashlib.sha256(population).hexdigest(),
        },
        "computed_goto": _computed_goto(),
        "conditional_store": _conditional_store(),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
