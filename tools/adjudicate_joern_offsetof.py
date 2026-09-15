"""Adjudicate CFG differences caused by Joern losing `offsetof` branches."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import tempfile
import time
from pathlib import Path
from typing import Any

import networkx as nx
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
CASES = {
    "crates/cindergraph/tests/decompiler_fixtures/src/94_alignment.c": {
        "offset_of_payload": {
            ("Padded", "payload"): 16,
            ("Packed", "payload"): 4,
        }
    },
    "crates/cindergraph/tests/decompiler_fixtures/src/100_struct_layout.c": {
        "layout_offsets": {
            ("Wasteful", "value"): 4,
            ("Wasteful", "extra"): 12,
            ("Tight", "tag"): 8,
            ("Tight", "flag"): 9,
        }
    },
    "crates/cindergraph/tests/decompiler_fixtures/src/161_packed_struct_layout.c": {
        "pk161_member_offset": {
            ("pk161_natural", "kind"): 0,
            ("pk161_natural", "seq"): 4,
            ("pk161_natural", "port"): 8,
            ("pk161_natural", "ttl"): 10,
            ("pk161_wire", "kind"): 0,
            ("pk161_wire", "seq"): 1,
            ("pk161_wire", "port"): 5,
            ("pk161_wire", "ttl"): 7,
        }
    },
}


def _replace(text: str, values: dict[tuple[str, str], int]) -> str:
    for (type_name, member), value in values.items():
        pattern = (
            rf"offsetof\s*\(\s*struct\s+{re.escape(type_name)}\s*,\s*{member}\s*\)"
        )
        text, count = re.subn(pattern, str(value), text)
        if count != 1:
            raise RuntimeError(
                f"expected one offsetof({type_name}, {member}), got {count}"
            )
    return text


def _compiler_proves(text: str, values: dict[tuple[str, str], int]) -> None:
    assertions = "\n".join(
        f'_Static_assert(offsetof(struct {type_name}, {member}) == {value}, "offset");'
        for (type_name, member), value in values.items()
    )
    subprocess.run(
        ["cc", "-fsyntax-only", "-x", "c", "-"],
        input=text + "\n" + assertions + "\n",
        text=True,
        capture_output=True,
        check=True,
    )


def _joern(text: str) -> dict[str, nx.DiGraph[Any]]:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".c") as source:
        source.write(text)
        source.flush()
        parsed = parse_source(Path(source.name), no_ddg=True, no_ast=True) or {}
    return {function.name: function.cfg for function in parsed.values()}


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


def adjudicate() -> dict[str, Any]:
    rows = []
    population = hashlib.sha256()
    for relative, functions in CASES.items():
        raw = (ROOT / relative).read_text()
        population.update(relative.encode() + b"\0" + raw.encode())
        values = next(iter(functions.values()))
        _compiler_proves(raw, values)
        replaced = _replace(raw, values)
        cinder_raw = cfgs_from_decompiled(raw)
        cinder_replaced = cfgs_from_decompiled(replaced)
        joern_raw = _joern(raw)
        joern_replaced = _joern(replaced)
        for function, function_values in functions.items():
            left = cinder_raw[function]
            right = joern_raw[function]
            transformed_left = cinder_replaced[function]
            transformed_right = joern_replaced[function]
            rows.append(
                {
                    "path": relative,
                    "function": function,
                    "compiler_verified_offsets": {
                        f"struct {type_name}.{member}": value
                        for (type_name, member), value in function_values.items()
                    },
                    "raw": {
                        "cindergraph_nodes": left.number_of_nodes(),
                        "cindergraph_edges": left.number_of_edges(),
                        "joern_nodes": right.number_of_nodes(),
                        "joern_edges": right.number_of_edges(),
                    },
                    "cindergraph_invariant_under_literal_substitution": _same(
                        left, transformed_left
                    ),
                    "providers_equal_after_literal_substitution": _same(
                        transformed_left, transformed_right
                    ),
                }
            )
    return {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "compiler-verified offsetof CFG adjudication; no upstream interaction",
        "population_sha256": population.hexdigest(),
        "summary": {
            "functions": len(rows),
            "compiler_verified": len(rows),
            "cindergraph_invariant": sum(
                row["cindergraph_invariant_under_literal_substitution"] for row in rows
            ),
            "providers_equal_after_literal_substitution": sum(
                row["providers_equal_after_literal_substitution"] for row in rows
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
