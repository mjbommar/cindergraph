"""Adjudicate Joern CFG nodes that spell object-like C macros as calls.

This is a local evidence tool. It reads fixtures and invokes the installed C
preprocessor and pyjoern; it does not interact with DecBench upstream.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import platform
import re
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

import networkx as nx
from decbench.metrics.vj_ged import vj_ged
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "crates/cindergraph/tests/decompiler_fixtures/src/103_computed_goto.c": [
        "threaded_interpreter"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/145_control_flow_flattening.c": [
        "flattened_accumulate",
        "flattened_classify",
        "flattened_gcd",
        "flattened_search",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/150_obfuscation_composite.c": [
        "obfuscated_digest",
        "obfuscated_transform",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/164_nested_tlv_walker.c": [
        "tlv164_seek"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/174_float_compare_classify.c": [
        "classify_binary32"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/31_edit_distance.c": [
        "edit_distance"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/32_longest_common_subsequence.c": [
        "lcs_length",
        "lcs_recover",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/35_matrix_chain.c": [
        "matrix_chain_cost"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/45_string_algorithms.c": [
        "parse_decimal"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/47_huffman.c": [
        "huffman_code_lengths"
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/78_ring_buffer.c": [
        "ring_push",
        "ring_pop",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/79_segment_tree.c": [
        "segment_build"
    ],
}
DEFINE = re.compile(r"^\s*#\s*define\s+([A-Za-z_]\w*)", re.MULTILINE)
INCLUDE = re.compile(r"^\s*#\s*include\b.*$", re.MULTILINE)
CALL = re.compile(r"<Call: ([A-Za-z_]\w*)\(\)>")


def _git(args: list[str], cwd: Path) -> str:
    return subprocess.run(
        ["git", *args], cwd=cwd, check=True, capture_output=True, text=True
    ).stdout.strip()


def _source_digest() -> str:
    paths = sorted(
        path
        for base in (ROOT / "crates", ROOT / "python/cindergraph")
        for path in base.rglob("*")
        if path.is_file() and path.suffix in {".rs", ".py", ".toml"}
    )
    digest = hashlib.sha256()
    for path in paths:
        digest.update(str(path.relative_to(ROOT)).encode() + b"\0")
        digest.update(path.read_bytes())
    return digest.hexdigest()


def _object_macros(text: str) -> dict[str, str]:
    """Object-like definitions, preserving their source declaration."""
    found = {}
    for match in DEFINE.finditer(text):
        line_end = text.find("\n", match.end())
        if line_end < 0:
            line_end = len(text)
        suffix = text[match.end() : line_end]
        # Function-like macros require `(` immediately after the name. A space
        # before `(` makes an object-like replacement beginning with parens.
        if suffix.startswith("("):
            continue
        found[match.group(1)] = text[match.start() : line_end].strip()
    return found


def _preprocess(text: str) -> str:
    """Expand fixture macros without importing system-header declarations."""
    without_includes = INCLUDE.sub("", text)
    result = subprocess.run(
        ["cc", "-E", "-P", "-x", "c", "-"],
        input=without_includes,
        text=True,
        capture_output=True,
        check=True,
    )
    return result.stdout


def _joern(text: str) -> tuple[dict[str, nx.DiGraph[Any]], dict[str, list[str]]]:
    with tempfile.NamedTemporaryFile(mode="w", suffix=".c") as source:
        source.write(text)
        source.flush()
        parsed = parse_source(Path(source.name), no_ddg=True, no_ast=False) or {}
    graphs = {}
    calls = {}
    for function in parsed.values():
        graph = getattr(function, "cfg", None)
        if graph is None:
            continue
        graphs[function.name] = graph
        calls[function.name] = [
            match.group(1)
            for node in graph.nodes
            for statement in getattr(node, "statements", ())
            for match in CALL.finditer(repr(statement))
        ]
    return graphs, calls


def _role(node: object) -> tuple[bool, bool]:
    return (
        bool(getattr(node, "is_entrypoint", False)),
        bool(getattr(node, "is_exitpoint", False)),
    )


def _isomorphic(left: nx.DiGraph[Any], right: nx.DiGraph[Any]) -> bool:
    return nx.is_isomorphic(
        left,
        right,
        node_match=lambda a, b: a["role"] == b["role"],
    )


def _normalized(graph: nx.DiGraph[Any]) -> nx.DiGraph[int]:
    result: nx.DiGraph[int] = nx.DiGraph()
    ids = {node: index for index, node in enumerate(graph.nodes)}
    result.add_nodes_from((ids[node], {"role": _role(node)}) for node in graph.nodes)
    result.add_edges_from((ids[a], ids[b]) for a, b in graph.edges)
    return result


def _compare(
    left: nx.DiGraph[Any] | None, right: nx.DiGraph[Any] | None
) -> dict[str, Any]:
    if left is None or right is None:
        return {"both_recovered": False, "vj_ged": None, "role_isomorphic": None}
    return {
        "both_recovered": True,
        "vj_ged": float(vj_ged(left, right)),
        "role_isomorphic": _isomorphic(_normalized(left), _normalized(right)),
    }


def adjudicate() -> dict[str, Any]:
    rows = []
    for index, (relative, names) in enumerate(TARGETS.items(), 1):
        print(f"[{index}/{len(TARGETS)}] {relative}", flush=True)
        path = ROOT / relative
        raw = path.read_text()
        expanded = _preprocess(raw)
        macros = _object_macros(raw)
        raw_cinder = cfgs_from_decompiled(raw)
        expanded_cinder = cfgs_from_decompiled(expanded)
        raw_joern, raw_calls = _joern(raw)
        expanded_joern, expanded_calls = _joern(expanded)
        for name in names:
            phantom = sorted(call for call in raw_calls.get(name, []) if call in macros)
            rows.append(
                {
                    "path": relative,
                    "function": name,
                    "object_macro_calls_in_raw_joern": phantom,
                    "object_macro_declarations": {
                        macro: macros[macro] for macro in sorted(set(phantom))
                    },
                    "same_calls_after_preprocessing": sorted(
                        call
                        for call in expanded_calls.get(name, [])
                        if call in set(phantom)
                    ),
                    "cindergraph_raw_vs_preprocessed": _compare(
                        raw_cinder.get(name), expanded_cinder.get(name)
                    ),
                    "joern_raw_vs_preprocessed": _compare(
                        raw_joern.get(name), expanded_joern.get(name)
                    ),
                    "providers_on_preprocessed": _compare(
                        expanded_cinder.get(name), expanded_joern.get(name)
                    ),
                }
            )
    population = b"".join(
        relative.encode() + b"\0" + (ROOT / relative).read_bytes()
        for relative in TARGETS
    )
    decbench_spec = importlib.util.find_spec("decbench")
    if decbench_spec is None or decbench_spec.origin is None:
        raise RuntimeError("cannot locate the imported DecBench checkout")
    decbench_root = Path(decbench_spec.origin).resolve().parents[1]
    status = _git(["status", "--porcelain=v1"], ROOT)
    return {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "local object-like macro call adjudication; no upstream interaction",
        "provenance": {
            "cindergraph_git_head": _git(["rev-parse", "HEAD"], ROOT),
            "cindergraph_worktree_dirty": bool(status),
            "cindergraph_source_sha256": _source_digest(),
            "decbench_git_head": _git(["rev-parse", "HEAD"], decbench_root),
            "python": sys.version,
            "platform": platform.platform(),
            "cc": subprocess.run(
                ["cc", "--version"], check=True, capture_output=True, text=True
            ).stdout.splitlines()[0],
            "pyjoern": importlib.metadata.version("pyjoern"),
            "decbench": importlib.metadata.version("decbench"),
            "networkx": importlib.metadata.version("networkx"),
        },
        "population": {
            "translation_units": len(TARGETS),
            "functions": sum(map(len, TARGETS.values())),
            "sha256": hashlib.sha256(population).hexdigest(),
        },
        "summary": {
            "functions": len(rows),
            "with_object_macro_calls_in_raw_joern": sum(
                bool(row["object_macro_calls_in_raw_joern"]) for row in rows
            ),
            "calls_surviving_preprocessing": sum(
                len(row["same_calls_after_preprocessing"]) for row in rows
            ),
            "cindergraph_invariant_under_preprocessing": sum(
                row["cindergraph_raw_vs_preprocessed"]["role_isomorphic"] is True
                for row in rows
            ),
            "providers_equal_after_preprocessing": sum(
                row["providers_on_preprocessed"]["role_isomorphic"] is True
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
