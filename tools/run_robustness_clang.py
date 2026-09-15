#!/usr/bin/env python3
"""Run Clang's raw C frontend over one validated robustness manifest."""

from __future__ import annotations

import argparse
from collections import Counter
from collections.abc import Iterator
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DIAGNOSTIC = re.compile(r"(?m)^.*?:\d+:\d+: (?P<severity>fatal error|error|warning):")


def _module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _adapter_identity() -> str:
    return hashlib.sha256(Path(__file__).read_bytes()).hexdigest()


def _source_state() -> tuple[str, bool]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    status = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=ROOT,
        text=True,
        capture_output=True,
        check=False,
    )
    if revision.returncode != 0 or status.returncode != 0:
        return "unavailable", True
    return revision.stdout.strip(), bool(status.stdout)


def _execution_identity(
    clang: Path, manifest: Path, source_root: Path, timeout_seconds: float
) -> dict[str, Any]:
    revision, dirty = _source_state()
    host = platform.node() or "unknown-host"
    return {
        "host_id": hashlib.sha256(host.encode()).hexdigest(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "command": [
            "run_robustness_clang.py",
            str(manifest),
            "--clang",
            str(clang),
            "--source-root",
            str(source_root),
            "--timeout-seconds",
            f"{timeout_seconds:g}",
        ],
        "timeout_ns": int(timeout_seconds * 1_000_000_000),
        "memory_limit_bytes": None,
        "source_revision": revision,
        "source_dirty": dirty,
    }


def _failure(
    base: dict[str, Any], status: str, kind: str, message: str, elapsed_ns: int
) -> dict[str, Any]:
    return {
        **base,
        "status": status,
        "failure": {"kind": kind, "message": message},
        "timing_ns": {"startup": None, "analysis": elapsed_ns, "serialization": None},
        "peak_rss_bytes": None,
        "diagnostics": {"errors": None, "warnings": None, "recovery_nodes": None},
        "yield": {"functions": [], "covered_source_bytes": None},
        "claims": {"syntax_complete": None, "cfg_complete": None},
        "artifacts": {"normalized_ast": None, "normalized_cfg": None},
    }


def _locations(value: Any) -> Iterator[dict[str, Any]]:
    if isinstance(value, dict):
        yield value
        for key in ("spellingLoc", "expansionLoc"):
            if key in value:
                yield from _locations(value[key])


def _offset(value: Any) -> int | None:
    for location in _locations(value):
        offset = location.get("offset")
        if isinstance(offset, int):
            return offset
    return None


def _end_offset(value: Any) -> int | None:
    for location in _locations(value):
        offset = location.get("offset")
        token_length = location.get("tokLen", 0)
        if isinstance(offset, int) and isinstance(token_length, int):
            return offset + token_length
    return None


def _span(node: dict[str, Any]) -> tuple[int | None, int | None]:
    source_range = node.get("range", {})
    if not isinstance(source_range, dict):
        return None, None
    return _offset(source_range.get("begin")), _end_offset(source_range.get("end"))


def _walk(node: dict[str, Any]) -> Iterator[dict[str, Any]]:
    yield node
    children = node.get("inner", [])
    if isinstance(children, list):
        for child in children:
            if isinstance(child, dict):
                yield from _walk(child)


def _functions(ast: dict[str, Any]) -> list[dict[str, Any]]:
    return [
        node
        for node in _walk(ast)
        if node.get("kind") == "FunctionDecl"
        and any(
            isinstance(child, dict) and child.get("kind") == "CompoundStmt"
            for child in node.get("inner", [])
        )
    ]


def _project_function(function: dict[str, Any]) -> dict[str, Any]:
    nodes: list[dict[str, Any]] = []
    edges: list[dict[str, int]] = []

    def visit(node: dict[str, Any], parent: int | None = None) -> None:
        node_id = len(nodes)
        start, end = _span(node)
        projected: dict[str, Any] = {
            "id": node_id,
            "kind": str(node.get("kind", "unknown")),
            "start": start,
            "end": end,
        }
        if isinstance(node.get("name"), str):
            projected["name"] = node["name"]
        nodes.append(projected)
        if parent is not None:
            edges.append({"source": parent, "target": node_id})
        children = node.get("inner", [])
        if isinstance(children, list):
            for child in children:
                if isinstance(child, dict):
                    visit(child, node_id)

    visit(function)
    start, end = _span(function)
    return {
        "name": function.get("name", ""),
        "start": start,
        "end": end,
        "nodes": nodes,
        "edges": edges,
    }


def _covered_bytes(graphs: list[dict[str, Any]]) -> int:
    spans = sorted(
        (graph["start"], graph["end"])
        for graph in graphs
        if isinstance(graph["start"], int)
        and isinstance(graph["end"], int)
        and graph["start"] < graph["end"]
    )
    covered = 0
    previous_end = 0
    for start, end in spans:
        covered += max(0, end - max(start, previous_end))
        previous_end = max(previous_end, end)
    return covered


def _artifact_name(specimen_id: str, suffix: str) -> str:
    identity = hashlib.sha256(specimen_id.encode()).hexdigest()[:24]
    return f"{identity}.{suffix}"


def analyze(
    row: dict[str, Any],
    base: dict[str, Any],
    clang: Path,
    source_root: Path,
    artifact_root: Path,
    timeout_seconds: float,
) -> dict[str, Any]:
    """Analyze one raw specimen with a disposable Clang process."""
    if row["build_context"] is not None:
        return _failure(
            base,
            "unsupported",
            "BuildContext",
            "configured compilation is not implemented by this adapter",
            0,
        )
    data = (source_root / row["source_path"]).read_bytes()
    span = row["slice"]
    source = data[span["start_byte"] : span["end_byte"]]
    command = [
        str(clang),
        "-x",
        "c",
        "-std=gnu11",
        "-fsyntax-only",
        "-fno-color-diagnostics",
        "-Xclang",
        "-ast-dump=json",
        "-",
    ]
    started = time.perf_counter_ns()
    try:
        completed = subprocess.run(
            command,
            input=source,
            capture_output=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return _failure(
            base,
            "timeout",
            "TimeoutExpired",
            f"clang exceeded {timeout_seconds:g} seconds",
            time.perf_counter_ns() - started,
        )
    analysis_ns = time.perf_counter_ns() - started
    if completed.returncode < 0:
        signal_number = -completed.returncode
        return _failure(
            base,
            "signal",
            f"Signal{signal_number}",
            f"clang terminated by signal {signal_number}",
            analysis_ns,
        )
    serialization_started = time.perf_counter_ns()
    try:
        ast = json.loads(completed.stdout)
        if not isinstance(ast, dict):
            raise ValueError("Clang AST root is not an object")
        graphs = [_project_function(function) for function in _functions(ast)]
        diagnostic_text = completed.stderr.decode("utf-8", errors="replace")
        source_name = _artifact_name(row["id"], "input.c")
        diagnostics_name = _artifact_name(row["id"], "diagnostics.txt")
        raw_ast_name = _artifact_name(row["id"], "clang-ast.json")
        normalized_name = _artifact_name(row["id"], "normalized-ast.json")
        artifact_root.mkdir(parents=True, exist_ok=True)
        (artifact_root / source_name).write_bytes(source)
        (artifact_root / diagnostics_name).write_text(diagnostic_text, encoding="utf-8")
        (artifact_root / raw_ast_name).write_bytes(completed.stdout)
        (artifact_root / normalized_name).write_text(
            json.dumps(graphs, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
    except (json.JSONDecodeError, OSError, TypeError, ValueError) as error:
        return _failure(
            base, "invalid_output", type(error).__name__, str(error), analysis_ns
        )
    serialization_ns = time.perf_counter_ns() - serialization_started
    severities = Counter(
        match.group("severity") for match in DIAGNOSTIC.finditer(diagnostic_text)
    )
    names = [graph["name"] for graph in graphs]
    return {
        **base,
        "status": "success",
        "failure": None,
        "native_exit_code": completed.returncode,
        "timing_ns": {
            "startup": None,
            "analysis": analysis_ns,
            "serialization": serialization_ns,
        },
        "peak_rss_bytes": None,
        "diagnostics": {
            "errors": severities["error"] + severities["fatal error"],
            "warnings": severities["warning"],
            "recovery_nodes": None,
        },
        "yield": {"functions": names, "covered_source_bytes": _covered_bytes(graphs)},
        "claims": {"syntax_complete": completed.returncode == 0, "cfg_complete": None},
        "artifacts": {
            "analyzed_source": source_name,
            "native_diagnostics": diagnostics_name,
            "native_ast": raw_ast_name,
            "normalized_ast": normalized_name,
            "normalized_cfg": None,
        },
    }


def _clang_identity(clang: Path) -> tuple[Path, str]:
    resolved = clang.resolve(strict=True)
    completed = subprocess.run(
        [str(resolved), "--version"], text=True, capture_output=True, check=False
    )
    if completed.returncode != 0 or not completed.stdout.strip():
        raise ValueError(f"cannot identify Clang executable: {clang}")
    return resolved, completed.stdout.splitlines()[0]


def run(
    manifest: Path,
    output: Path,
    artifact_root: Path,
    source_root: Path,
    run_id: str,
    clang: Path,
    *,
    timeout_seconds: float = 30.0,
) -> list[dict[str, Any]]:
    """Run every manifest row, journal it, and validate the fixed denominator."""
    contract = _module(ROOT / "tools/robustness_contract.py", "robustness_contract")
    manifest_rows = contract.load_jsonl(manifest)
    contract.validate_manifest(manifest_rows, source_root)
    if timeout_seconds <= 0:
        raise ValueError("timeout_seconds must be positive")
    if output.exists():
        raise ValueError(f"refusing to overwrite result file: {output}")
    partial = output.with_suffix(f"{output.suffix}.partial")
    if partial.exists():
        raise ValueError(f"refusing to overwrite partial result file: {partial}")
    if artifact_root.exists() and any(artifact_root.iterdir()):
        raise ValueError(f"artifact directory is not empty: {artifact_root}")
    clang, version = _clang_identity(clang)
    tool = {"name": "clang", "version": version, "adapter": _adapter_identity()}
    execution = _execution_identity(clang, manifest, source_root, timeout_seconds)
    manifest_hash = contract.manifest_sha256(manifest_rows)
    results = []
    output.parent.mkdir(parents=True, exist_ok=True)
    with partial.open("x", encoding="utf-8") as journal:
        for index, row in enumerate(manifest_rows, 1):
            base = {
                "schema": 1,
                "run_id": run_id,
                "manifest_sha256": manifest_hash,
                "tool": tool,
                "execution": execution,
                "specimen_id": row["id"],
            }
            result = analyze(
                row, base, clang, source_root, artifact_root, timeout_seconds
            )
            results.append(result)
            journal.write(json.dumps(result, sort_keys=True) + "\n")
            journal.flush()
            os.fsync(journal.fileno())
            if index % 25 == 0 or index == len(manifest_rows):
                print(f"[{index}/{len(manifest_rows)}]", flush=True)
    contract.validate_results(results, manifest_rows)
    partial.replace(output)
    return results


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--clang", type=Path, default=Path("/usr/bin/clang"))
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    args = parser.parse_args()
    results = run(
        args.manifest,
        args.output,
        args.artifacts,
        args.source_root,
        args.run_id,
        args.clang,
        timeout_seconds=args.timeout_seconds,
    )
    statuses = Counter(row["status"] for row in results)
    print(json.dumps({"specimens": len(results), "statuses": statuses}, sort_keys=True))


if __name__ == "__main__":
    main()
