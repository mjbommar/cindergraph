#!/usr/bin/env python3
"""Run Cindergraph over one validated robustness manifest."""

from __future__ import annotations

import argparse
import hashlib
import importlib
import importlib.metadata
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def _peak_rss_bytes() -> int | None:
    """Return this worker's peak resident set where the platform exposes it."""
    try:
        import resource

        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    except (ImportError, OSError):
        return None
    # Linux and the BSDs report KiB; macOS reports bytes.
    return int(peak if sys.platform == "darwin" else peak * 1024)


def _set_memory_limit(memory_bytes: int | None) -> None:
    """Apply a worker address-space limit before loading Cindergraph."""
    if memory_bytes is None:
        return
    try:
        import resource
    except ImportError as error:
        raise RuntimeError("memory limits are unavailable on this platform") from error
    resource.setrlimit(resource.RLIMIT_AS, (memory_bytes, memory_bytes))


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
    """Identify the checked-out revision and whether its worktree is dirty."""
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
    manifest: Path,
    source_root: Path,
    timeout_seconds: float,
    memory_bytes: int | None,
) -> dict[str, Any]:
    revision, dirty = _source_state()
    host = platform.node() or "unknown-host"
    return {
        "host_id": hashlib.sha256(host.encode()).hexdigest(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "command": [
            "run_robustness_cindergraph.py",
            str(manifest),
            "--source-root",
            str(source_root),
            "--timeout-seconds",
            f"{timeout_seconds:g}",
            "--memory-bytes",
            "none" if memory_bytes is None else str(memory_bytes),
        ],
        "timeout_ns": int(timeout_seconds * 1_000_000_000),
        "memory_limit_bytes": memory_bytes,
        "source_revision": revision,
        "source_dirty": dirty,
    }


def _covered_bytes(graphs: list[dict[str, Any]]) -> int:
    spans = sorted(
        (graph["start"], graph["end"])
        for graph in graphs
        if isinstance(graph.get("start"), int)
        and isinstance(graph.get("end"), int)
        and graph["start"] < graph["end"]
    )
    covered = 0
    end = 0
    for start, next_end in spans:
        if start >= end:
            covered += next_end - start
        elif next_end > end:
            covered += next_end - end
        end = max(end, next_end)
    return covered


def _validate_graphs(serialized: list[tuple[str, str]], representation: str) -> None:
    for name, document in serialized:
        try:
            graph = json.loads(document)
        except json.JSONDecodeError as error:
            raise ValueError(f"{representation} graph {name!r} is not JSON") from error
        nodes = [node["id"] for node in graph["nodes"]]
        if len(nodes) != len(set(nodes)):
            raise ValueError(f"{representation} graph {name!r} has duplicate node IDs")
        node_set = set(nodes)
        if any(
            edge["source"] not in node_set or edge["target"] not in node_set
            for edge in graph["edges"]
        ):
            raise ValueError(f"{representation} graph {name!r} has an open edge")


def _artifact_name(specimen_id: str, representation: str) -> str:
    identity = hashlib.sha256(specimen_id.encode()).hexdigest()[:24]
    return f"{identity}.{representation}.json"


def _failure_row(
    base: dict[str, Any],
    kind: str,
    error: BaseException,
    analysis_ns: int | None,
    *,
    startup_ns: int | None = None,
    peak_rss_bytes: int | None = None,
) -> dict[str, Any]:
    return {
        **base,
        "status": kind,
        "failure": {"kind": type(error).__name__, "message": str(error)},
        "timing_ns": {
            "startup": startup_ns,
            "analysis": analysis_ns,
            "serialization": 0,
        },
        "peak_rss_bytes": peak_rss_bytes,
        "diagnostics": {"errors": None, "warnings": None, "recovery_nodes": None},
        "yield": {"functions": [], "covered_source_bytes": None},
        "claims": {"syntax_complete": None, "cfg_complete": None},
        "artifacts": {
            "analyzed_source": None,
            "native_diagnostics": None,
            "normalized_ast": None,
            "normalized_cfg": None,
        },
    }


def analyze_specimen(
    row: dict[str, Any],
    source_root: Path,
    artifact_root: Path,
    base: dict[str, Any],
    cg: Any,
    startup_ns: int,
) -> dict[str, Any]:
    """Analyze one immutable byte slice and return one contract result."""
    started = time.perf_counter_ns()
    try:
        data = (source_root / row["source_path"]).read_bytes()
        span = row["slice"]
        source = data[span["start_byte"] : span["end_byte"]].decode(
            "utf-8", errors="replace"
        )
        session = cg.AnalysisSession(source, dialect=row["dialect"])
        graphs = session.control_flow_graphs()
        flows = session.data_flow()
        diagnostics = session.diagnostics
    except Exception as error:  # noqa: BLE001 - provider failures are measurements
        return _failure_row(
            base,
            "memory_limit" if isinstance(error, MemoryError) else "exception",
            error,
            time.perf_counter_ns() - started,
            startup_ns=startup_ns,
            peak_rss_bytes=_peak_rss_bytes(),
        )
    analysis_ns = time.perf_counter_ns() - started

    serialization_started = time.perf_counter_ns()
    try:
        ast = session.export_graphs(repr="ast", format="json")
        cfg = session.export_graphs(repr="cfg", format="json")
        _validate_graphs(ast, "ast")
        _validate_graphs(cfg, "cfg")
        ast_name = _artifact_name(row["id"], "ast")
        cfg_name = _artifact_name(row["id"], "cfg")
        source_name = _artifact_name(row["id"], "analyzed.c")
        diagnostics_name = _artifact_name(row["id"], "diagnostics")
        artifact_root.mkdir(parents=True, exist_ok=True)
        (artifact_root / source_name).write_text(session.source, encoding="utf-8")
        (artifact_root / diagnostics_name).write_text(
            json.dumps(
                [
                    {
                        "severity": diagnostic.severity,
                        "message": diagnostic.message,
                        "start": diagnostic.start,
                        "end": diagnostic.end,
                        "text": diagnostic.text,
                    }
                    for diagnostic in diagnostics
                ],
                separators=(",", ":"),
            )
            + "\n",
            encoding="utf-8",
        )
        (artifact_root / ast_name).write_text(
            json.dumps(ast, separators=(",", ":")) + "\n", encoding="utf-8"
        )
        (artifact_root / cfg_name).write_text(
            json.dumps(cfg, separators=(",", ":")) + "\n", encoding="utf-8"
        )
    except Exception as error:  # noqa: BLE001 - invalid provider output is measured
        return _failure_row(
            base,
            "memory_limit" if isinstance(error, MemoryError) else "invalid_output",
            error,
            analysis_ns,
            startup_ns=startup_ns,
            peak_rss_bytes=_peak_rss_bytes(),
        )
    serialization_ns = time.perf_counter_ns() - serialization_started

    errors = sum(diagnostic.severity == "error" for diagnostic in diagnostics)
    warnings = sum(diagnostic.severity == "warning" for diagnostic in diagnostics)
    names = [graph["name"] for graph in graphs]
    syntax_complete = not diagnostics
    cfg_complete = (
        bool(graphs)
        and len(flows) == len(graphs)
        and all(
            flow["recovery_free"] and flow["control_targets_complete"] for flow in flows
        )
    )
    return {
        **base,
        "status": "success",
        "failure": None,
        "timing_ns": {
            "startup": startup_ns,
            "analysis": analysis_ns,
            "serialization": serialization_ns,
        },
        "peak_rss_bytes": _peak_rss_bytes(),
        "diagnostics": {
            "errors": errors,
            "warnings": warnings,
            "recovery_nodes": None,
        },
        "yield": {"functions": names, "covered_source_bytes": _covered_bytes(graphs)},
        "claims": {
            "syntax_complete": syntax_complete,
            "cfg_complete": cfg_complete,
        },
        "artifacts": {
            "analyzed_source": source_name,
            "native_diagnostics": diagnostics_name,
            "normalized_ast": ast_name,
            "normalized_cfg": cfg_name,
        },
    }


def _supervisor_failure(
    base: dict[str, Any], status: str, kind: str, message: str, elapsed_ns: int
) -> dict[str, Any]:
    """Represent a worker-level failure without inventing unavailable facts."""
    return {
        **base,
        "status": status,
        "failure": {"kind": kind, "message": message},
        "timing_ns": {
            "startup": elapsed_ns,
            "analysis": None,
            "serialization": None,
        },
        "peak_rss_bytes": None,
        "diagnostics": {"errors": None, "warnings": None, "recovery_nodes": None},
        "yield": {"functions": [], "covered_source_bytes": None},
        "claims": {"syntax_complete": None, "cfg_complete": None},
        "artifacts": {
            "analyzed_source": None,
            "native_diagnostics": None,
            "normalized_ast": None,
            "normalized_cfg": None,
        },
    }


def _run_isolated(
    row: dict[str, Any],
    base: dict[str, Any],
    source_root: Path,
    artifact_root: Path,
    timeout_seconds: float,
    memory_bytes: int | None,
) -> dict[str, Any]:
    """Run one specimen in a disposable process and classify its termination."""
    command = [
        sys.executable,
        str(Path(__file__).resolve()),
        "--worker",
        "--source-root",
        str(source_root),
        "--artifacts",
        str(artifact_root),
    ]
    if memory_bytes is not None:
        command.extend(("--memory-bytes", str(memory_bytes)))
    started = time.perf_counter_ns()
    try:
        completed = subprocess.run(
            command,
            input=json.dumps({"row": row, "base": base}),
            text=True,
            capture_output=True,
            timeout=timeout_seconds,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return _supervisor_failure(
            base,
            "timeout",
            "TimeoutExpired",
            f"worker exceeded {timeout_seconds:g} seconds",
            time.perf_counter_ns() - started,
        )
    elapsed_ns = time.perf_counter_ns() - started
    if completed.returncode < 0:
        signal_number = -completed.returncode
        return _supervisor_failure(
            base,
            "signal",
            f"Signal{signal_number}",
            f"worker terminated by signal {signal_number}",
            elapsed_ns,
        )
    if completed.returncode != 0:
        message = completed.stderr.strip() or "worker exited without diagnostics"
        return _supervisor_failure(
            base, "adapter_error", "WorkerExit", message, elapsed_ns
        )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        return _supervisor_failure(
            base, "adapter_error", type(error).__name__, str(error), elapsed_ns
        )
    if not isinstance(result, dict):
        return _supervisor_failure(
            base,
            "adapter_error",
            "WorkerProtocol",
            "worker result is not a JSON object",
            elapsed_ns,
        )
    return result


def run(
    manifest: Path,
    output: Path,
    artifact_root: Path,
    source_root: Path,
    run_id: str,
    *,
    timeout_seconds: float = 30.0,
    memory_bytes: int | None = None,
) -> list[dict[str, Any]]:
    """Run every row exactly once and validate the completed result set."""
    contract = _module(ROOT / "tools/robustness_contract.py", "robustness_contract")
    manifest_rows = contract.load_jsonl(manifest)
    contract.validate_manifest(manifest_rows, source_root)
    if output.exists():
        raise ValueError(f"refusing to overwrite result file: {output}")
    partial = output.with_suffix(f"{output.suffix}.partial")
    if partial.exists():
        raise ValueError(f"refusing to overwrite partial result file: {partial}")
    if artifact_root.exists() and any(artifact_root.iterdir()):
        raise ValueError(f"artifact directory is not empty: {artifact_root}")
    if timeout_seconds <= 0:
        raise ValueError("timeout_seconds must be positive")
    if memory_bytes is not None and memory_bytes <= 0:
        raise ValueError("memory_bytes must be positive")
    tool = {
        "name": "cindergraph",
        "version": importlib.metadata.version("cindergraph"),
        "adapter": _adapter_identity(),
    }
    execution = _execution_identity(
        manifest, source_root, timeout_seconds, memory_bytes
    )
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
            result = _run_isolated(
                row, base, source_root, artifact_root, timeout_seconds, memory_bytes
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


def _worker_main(arguments: list[str]) -> None:
    """Run the private one-specimen worker protocol."""
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--memory-bytes", type=int)
    args = parser.parse_args(arguments)
    payload = json.loads(sys.stdin.read())
    _set_memory_limit(args.memory_bytes)
    started = time.perf_counter_ns()
    try:
        cg = importlib.import_module("cindergraph")
    except MemoryError as error:
        result = _failure_row(
            payload["base"],
            "memory_limit",
            error,
            None,
            startup_ns=time.perf_counter_ns() - started,
            peak_rss_bytes=_peak_rss_bytes(),
        )
        sys.stdout.write(json.dumps(result, sort_keys=True))
        return
    startup_ns = time.perf_counter_ns() - started
    result = analyze_specimen(
        payload["row"],
        args.source_root,
        args.artifacts,
        payload["base"],
        cg,
        startup_ns,
    )
    sys.stdout.write(json.dumps(result, sort_keys=True))


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "--worker":
        _worker_main(sys.argv[2:])
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--run-id", required=True)
    parser.add_argument(
        "--timeout-seconds",
        type=float,
        default=30.0,
        help="wall-clock limit for each disposable worker (default: 30)",
    )
    parser.add_argument(
        "--memory-mib",
        type=int,
        help="optional per-worker address-space limit in MiB",
    )
    args = parser.parse_args()
    results = run(
        args.manifest,
        args.output,
        args.artifacts,
        args.source_root,
        args.run_id,
        timeout_seconds=args.timeout_seconds,
        memory_bytes=None if args.memory_mib is None else args.memory_mib * 1024 * 1024,
    )
    statuses: dict[str, int] = {}
    for row in results:
        statuses[row["status"]] = statuses.get(row["status"], 0) + 1
    print(json.dumps({"specimens": len(results), "statuses": statuses}, sort_keys=True))


if __name__ == "__main__":
    main()
