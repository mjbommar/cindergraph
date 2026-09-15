#!/usr/bin/env python3
"""Run the pinned standalone Tree-sitter C robustness adapter."""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def _module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _adapter_identity() -> str:
    files = [
        Path(__file__),
        ROOT / "tools/tree-sitter-adapter/Cargo.toml",
        ROOT / "tools/tree-sitter-adapter/Cargo.lock",
        ROOT / "tools/tree-sitter-adapter/src/main.rs",
    ]
    digest = hashlib.sha256()
    for path in files:
        digest.update(path.relative_to(ROOT).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


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
    binary: Path, manifest: Path, source_root: Path, timeout_seconds: float
) -> dict[str, Any]:
    revision, dirty = _source_state()
    host = platform.node() or "unknown-host"
    return {
        "host_id": hashlib.sha256(host.encode()).hexdigest(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "command": [
            "run_robustness_tree_sitter.py",
            str(manifest),
            "--binary",
            str(binary),
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


def _artifact_name(specimen_id: str, suffix: str) -> str:
    identity = hashlib.sha256(specimen_id.encode()).hexdigest()[:24]
    return f"{identity}.{suffix}"


def _covered_bytes(functions: list[dict[str, Any]]) -> int:
    spans = sorted(
        (function["start"], function["end"])
        for function in functions
        if isinstance(function.get("start"), int)
        and isinstance(function.get("end"), int)
        and function["start"] < function["end"]
    )
    covered = 0
    previous_end = 0
    for start, end in spans:
        covered += max(0, end - max(start, previous_end))
        previous_end = max(previous_end, end)
    return covered


def _validate_worker(document: Any) -> dict[str, Any]:
    if not isinstance(document, dict):
        raise ValueError("worker output is not an object")
    required = {
        "schema",
        "tree_sitter_version",
        "grammar_version",
        "language_abi",
        "parse_ns",
        "has_error",
        "error_nodes",
        "missing_nodes",
        "root_end_byte",
        "functions",
        "nodes",
        "edges",
    }
    if missing := required.difference(document):
        raise ValueError(f"worker output is missing fields: {sorted(missing)}")
    if document["schema"] != 1 or not isinstance(document["has_error"], bool):
        raise ValueError("worker schema or has_error value is invalid")
    for name in (
        "language_abi",
        "parse_ns",
        "error_nodes",
        "missing_nodes",
        "root_end_byte",
    ):
        if (
            not isinstance(document[name], int)
            or isinstance(document[name], bool)
            or document[name] < 0
        ):
            raise ValueError(f"worker {name} must be a non-negative integer")
    if not isinstance(document["functions"], list):
        raise ValueError("worker functions must be a list")
    if not isinstance(document["nodes"], list) or not isinstance(
        document["edges"], list
    ):
        raise ValueError("worker graph tables must be lists")
    node_ids = [node["id"] for node in document["nodes"]]
    if len(node_ids) != len(set(node_ids)):
        raise ValueError("worker graph has duplicate node IDs")
    node_set = set(node_ids)
    if any(
        edge["source"] not in node_set or edge["target"] not in node_set
        for edge in document["edges"]
    ):
        raise ValueError("worker graph has an open edge")
    for function in document["functions"]:
        if not isinstance(function.get("name"), str):
            raise ValueError("worker function name must be a string")
    return document


def analyze(
    row: dict[str, Any],
    base: dict[str, Any],
    binary: Path,
    source_root: Path,
    artifact_root: Path,
    timeout_seconds: float,
) -> dict[str, Any]:
    """Analyze one exact specimen in an isolated native parser process."""
    data = (source_root / row["source_path"]).read_bytes()
    span = row["slice"]
    source = data[span["start_byte"] : span["end_byte"]]
    started = time.perf_counter_ns()
    try:
        completed = subprocess.run(
            [str(binary)],
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
            f"Tree-sitter worker exceeded {timeout_seconds:g} seconds",
            time.perf_counter_ns() - started,
        )
    analysis_ns = time.perf_counter_ns() - started
    if completed.returncode < 0:
        signal_number = -completed.returncode
        return _failure(
            base,
            "signal",
            f"Signal{signal_number}",
            f"Tree-sitter worker terminated by signal {signal_number}",
            analysis_ns,
        )
    if completed.returncode != 0:
        message = completed.stderr.decode("utf-8", errors="replace").strip()
        return _failure(
            base, "adapter_error", "WorkerExit", message or "worker failed", analysis_ns
        )
    serialization_started = time.perf_counter_ns()
    try:
        document = _validate_worker(json.loads(completed.stdout))
        source_name = _artifact_name(row["id"], "input.c")
        native_name = _artifact_name(row["id"], "tree-sitter.json")
        normalized_name = _artifact_name(row["id"], "normalized-ast.json")
        artifact_root.mkdir(parents=True, exist_ok=True)
        (artifact_root / source_name).write_bytes(source)
        (artifact_root / native_name).write_bytes(completed.stdout)
        normalized = {
            "functions": document["functions"],
            "nodes": document["nodes"],
            "edges": document["edges"],
        }
        (artifact_root / normalized_name).write_text(
            json.dumps(normalized, sort_keys=True, separators=(",", ":")) + "\n",
            encoding="utf-8",
        )
    except (json.JSONDecodeError, KeyError, OSError, TypeError, ValueError) as error:
        return _failure(
            base, "invalid_output", type(error).__name__, str(error), analysis_ns
        )
    serialization_ns = time.perf_counter_ns() - serialization_started
    functions = document["functions"]
    return {
        **base,
        "status": "success",
        "failure": None,
        "native_parse_ns": document["parse_ns"],
        "timing_ns": {
            "startup": None,
            "analysis": analysis_ns,
            "serialization": serialization_ns,
        },
        "peak_rss_bytes": None,
        "diagnostics": {
            "errors": document["error_nodes"],
            "warnings": None,
            "recovery_nodes": document["error_nodes"] + document["missing_nodes"],
        },
        "yield": {
            "functions": [function["name"] for function in functions],
            "covered_source_bytes": _covered_bytes(functions),
        },
        "claims": {"syntax_complete": not document["has_error"], "cfg_complete": None},
        "artifacts": {
            "analyzed_source": source_name,
            "native_diagnostics": native_name,
            "native_tree": native_name,
            "normalized_ast": normalized_name,
            "normalized_cfg": None,
        },
    }


def _binary_identity(binary: Path) -> tuple[Path, str]:
    resolved = binary.resolve(strict=True)
    completed = subprocess.run(
        [str(resolved), "--version"], text=True, capture_output=True, check=False
    )
    if completed.returncode != 0 or not completed.stdout.strip():
        raise ValueError(f"cannot identify Tree-sitter adapter binary: {binary}")
    return resolved, completed.stdout.strip()


def run(
    manifest: Path,
    output: Path,
    artifact_root: Path,
    source_root: Path,
    run_id: str,
    binary: Path,
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
    binary, version = _binary_identity(binary)
    tool = {"name": "tree-sitter-c", "version": version, "adapter": _adapter_identity()}
    execution = _execution_identity(binary, manifest, source_root, timeout_seconds)
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
                row, base, binary, source_root, artifact_root, timeout_seconds
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
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    args = parser.parse_args()
    results = run(
        args.manifest,
        args.output,
        args.artifacts,
        args.source_root,
        args.run_id,
        args.binary,
        timeout_seconds=args.timeout_seconds,
    )
    statuses = Counter(row["status"] for row in results)
    print(json.dumps({"specimens": len(results), "statuses": statuses}, sort_keys=True))


if __name__ == "__main__":
    main()
