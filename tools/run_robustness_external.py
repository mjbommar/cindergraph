#!/usr/bin/env python3
"""Run the fixed robustness manifest through installed CDT or Joern."""

from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
import time
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
JOERN_START = "CINDERGRAPH_JOERN_RESULT_START"
JOERN_END = "CINDERGRAPH_JOERN_RESULT_END"


def _module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _git_state() -> tuple[str, bool]:
    revision = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, capture_output=True
    )
    status = subprocess.run(
        ["git", "status", "--porcelain"], cwd=ROOT, text=True, capture_output=True
    )
    return revision.stdout.strip() or "unavailable", bool(status.stdout)


def _failure(
    base: dict[str, Any], status: str, kind: str, message: str, elapsed: int
) -> dict[str, Any]:
    return {
        **base,
        "status": status,
        "failure": {"kind": kind, "message": message},
        "timing_ns": {"startup": None, "analysis": elapsed, "serialization": None},
        "peak_rss_bytes": None,
        "diagnostics": {"errors": None, "warnings": None, "recovery_nodes": None},
        "yield": {"functions": [], "covered_source_bytes": None},
        "claims": {"syntax_complete": None, "cfg_complete": None},
        "artifacts": {"normalized_ast": None, "normalized_cfg": None},
    }


def _result_payload(tool: str, stdout: str) -> dict[str, Any]:
    if tool == "cdt":
        return json.loads(stdout.strip().splitlines()[-1])
    start = stdout.rfind(JOERN_START)
    end = stdout.rfind(JOERN_END)
    if start < 0 or end < start:
        raise ValueError("Joern result delimiters are absent")
    body = stdout[start + len(JOERN_START) : end].strip()
    return json.loads(body)


def _analyze(
    row: dict[str, Any],
    base: dict[str, Any],
    tool: str,
    executable: Path,
    source_root: Path,
    artifacts: Path,
    timeout: float,
) -> dict[str, Any]:
    if row["build_context"] is not None:
        return _failure(
            base,
            "unsupported",
            "BuildContext",
            "configured compilation is not implemented",
            0,
        )
    raw = (source_root / row["source_path"]).read_bytes()
    span = row["slice"]
    source = raw[span["start_byte"] : span["end_byte"]]
    identity = hashlib.sha256(row["id"].encode()).hexdigest()[:24]
    with tempfile.TemporaryDirectory(
        prefix="external-", dir=ROOT / "target/tmp"
    ) as directory:
        work = Path(directory)
        input_path = work / "input.c"
        input_path.write_bytes(source)
        if tool == "cdt":
            command = [str(executable), str(input_path)]
        else:
            command = [
                str(executable),
                "--script",
                str((ROOT / "tools/joern-adapter/functions.sc").resolve()),
                "--param",
                f"target={input_path}",
            ]
        started = time.perf_counter_ns()
        try:
            completed = subprocess.run(
                command,
                cwd=work,
                text=True,
                capture_output=True,
                timeout=timeout,
                check=False,
            )
        except subprocess.TimeoutExpired:
            return _failure(
                base,
                "timeout",
                "TimeoutExpired",
                f"{tool} exceeded {timeout:g} seconds",
                time.perf_counter_ns() - started,
            )
    elapsed = time.perf_counter_ns() - started
    if completed.returncode < 0:
        return _failure(
            base,
            "signal",
            f"Signal{-completed.returncode}",
            f"{tool} terminated by signal {-completed.returncode}",
            elapsed,
        )
    artifacts.mkdir(parents=True, exist_ok=True)
    source_name = f"{identity}.input.c"
    stdout_name = f"{identity}.stdout.txt"
    stderr_name = f"{identity}.stderr.txt"
    (artifacts / source_name).write_bytes(source)
    (artifacts / stdout_name).write_text(completed.stdout, encoding="utf-8")
    (artifacts / stderr_name).write_text(completed.stderr, encoding="utf-8")
    try:
        payload = _result_payload(tool, completed.stdout)
        functions = payload["functions"]
        if not isinstance(functions, list) or not all(
            isinstance(name, str) for name in functions
        ):
            raise ValueError("functions is not a string list")
    except (json.JSONDecodeError, KeyError, TypeError, ValueError) as error:
        return _failure(
            base, "invalid_output", type(error).__name__, str(error), elapsed
        )
    problems = payload.get("problems")
    return {
        **base,
        "status": "success",
        "failure": None,
        "native_exit_code": completed.returncode,
        "timing_ns": {"startup": None, "analysis": elapsed, "serialization": None},
        "peak_rss_bytes": None,
        "diagnostics": {"errors": problems, "warnings": None, "recovery_nodes": None},
        "yield": {"functions": functions, "covered_source_bytes": None},
        "claims": {
            "syntax_complete": problems == 0 if isinstance(problems, int) else None,
            "cfg_complete": None,
        },
        "artifacts": {
            "analyzed_source": source_name,
            "native_stdout": stdout_name,
            "native_stderr": stderr_name,
            "normalized_ast": None,
            "normalized_cfg": None,
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("--tool", choices=("cdt", "joern"), required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--artifacts", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--timeout-seconds", type=float, default=30.0)
    parser.add_argument("--jobs", type=int, default=1)
    args = parser.parse_args()
    contract = _module(
        ROOT / "tools/robustness_contract.py", "robustness_contract_external"
    )
    rows = contract.load_jsonl(args.manifest)
    contract.validate_manifest(rows, args.source_root)
    if (
        args.output.exists()
        or args.output.with_suffix(args.output.suffix + ".partial").exists()
    ):
        raise ValueError("refusing to overwrite existing results")
    executable = args.executable.resolve(strict=True)
    revision, dirty = _git_state()
    adapter_path = ROOT / (
        "tools/cdt-adapter/CindergraphCdtAdapter.java"
        if args.tool == "cdt"
        else "tools/joern-adapter/functions.sc"
    )
    adapter = hashlib.sha256(
        Path(__file__).read_bytes() + adapter_path.read_bytes()
    ).hexdigest()
    tool_identity = {"name": args.tool, "version": args.version, "adapter": adapter}
    execution = {
        "host_id": hashlib.sha256((platform.node() or "unknown").encode()).hexdigest(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "command": [
            "run_robustness_external.py",
            "--tool",
            args.tool,
            "--jobs",
            str(args.jobs),
            str(args.manifest),
        ],
        "timeout_ns": int(args.timeout_seconds * 1_000_000_000),
        "memory_limit_bytes": None,
        "source_revision": revision,
        "source_dirty": dirty,
    }
    manifest_hash = contract.manifest_sha256(rows)
    partial = args.output.with_suffix(args.output.suffix + ".partial")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    results = []
    if args.jobs < 1:
        raise ValueError("jobs must be positive")

    def analyze_row(row: dict[str, Any]) -> dict[str, Any]:
        base = {
            "schema": 1,
            "run_id": args.run_id,
            "manifest_sha256": manifest_hash,
            "tool": tool_identity,
            "execution": execution,
            "specimen_id": row["id"],
        }
        return _analyze(
            row,
            base,
            args.tool,
            executable,
            args.source_root,
            args.artifacts,
            args.timeout_seconds,
        )

    with (
        partial.open("x", encoding="utf-8") as journal,
        ThreadPoolExecutor(max_workers=args.jobs) as pool,
    ):
        for index, result in enumerate(pool.map(analyze_row, rows), 1):
            results.append(result)
            journal.write(json.dumps(result, sort_keys=True) + "\n")
            journal.flush()
            os.fsync(journal.fileno())
            if index % 25 == 0 or index == len(rows):
                print(f"[{index}/{len(rows)}]", flush=True)
    contract.validate_results(results, rows)
    partial.replace(args.output)
    print(
        json.dumps(
            {
                "specimens": len(results),
                "statuses": Counter(row["status"] for row in results),
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
