#!/usr/bin/env python3
"""Validate fail-closed manifests and adapter results for robustness studies.

The comparison harness deliberately uses JSON Lines and the Python standard
library only.  An adapter failure is a result row; it must never remove a
specimen from the population or masquerade as an empty successful analysis.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
from typing import Any


SCHEMA = 1
SHA256 = re.compile(r"[0-9a-f]{64}")
RESULT_STATUSES = frozenset(
    {
        "success",
        "timeout",
        "memory_limit",
        "signal",
        "exception",
        "invalid_output",
        "unsupported",
        "adapter_error",
    }
)
MANIFEST_FIELDS = frozenset(
    {
        "schema",
        "id",
        "family",
        "source_path",
        "source_sha256",
        "slice",
        "origin",
        "dialect",
        "build_context",
        "expected_functions",
        "oracle",
        "mutation",
    }
)
RESULT_FIELDS = frozenset(
    {
        "schema",
        "run_id",
        "manifest_sha256",
        "tool",
        "execution",
        "specimen_id",
        "status",
        "failure",
        "timing_ns",
        "peak_rss_bytes",
        "diagnostics",
        "yield",
        "claims",
        "artifacts",
    }
)


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    """Read a non-empty JSONL file and retain its declared row order."""
    rows: list[dict[str, Any]] = []
    for line_number, line in enumerate(
        path.read_text(encoding="utf-8").splitlines(), 1
    ):
        if not line.strip():
            raise ValueError(f"{path}:{line_number}: blank JSONL row")
        try:
            row = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(
                f"{path}:{line_number}: invalid JSON: {error.msg}"
            ) from error
        if not isinstance(row, dict):
            raise ValueError(f"{path}:{line_number}: row must be an object")
        rows.append(row)
    if not rows:
        raise ValueError(f"{path}: JSONL file is empty")
    return rows


def _canonical_jsonl(rows: list[dict[str, Any]]) -> bytes:
    return b"".join(
        (json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n").encode()
        for row in rows
    )


def manifest_sha256(rows: list[dict[str, Any]]) -> str:
    """Hash ordered canonical rows, independent of insignificant whitespace."""
    return hashlib.sha256(_canonical_jsonl(rows)).hexdigest()


def _object(value: Any, where: str) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise ValueError(f"{where} must be an object")
    return value


def _require_keys(value: dict[str, Any], keys: frozenset[str], where: str) -> None:
    missing = sorted(keys.difference(value))
    if missing:
        raise ValueError(f"{where} is missing required fields: {missing}")


def _text(value: Any, where: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{where} must be a non-empty string")
    return value


def _integer(value: Any, where: str, *, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise ValueError(f"{where} must be an integer >= {minimum}")
    return value


def _string_list(
    value: Any,
    where: str,
    *,
    require_unique: bool = True,
    require_nonempty: bool = True,
) -> list[str]:
    if not isinstance(value, list):
        raise ValueError(f"{where} must be a list")
    if require_nonempty:
        result = [_text(item, f"{where}[{index}]") for index, item in enumerate(value)]
    else:
        if not all(isinstance(item, str) for item in value):
            raise ValueError(f"{where} must contain only strings")
        result = value
    if require_unique and len(result) != len(set(result)):
        raise ValueError(f"{where} contains duplicates")
    return result


def _source_file(source_root: Path, declared: Any, where: str) -> Path:
    relative = Path(_text(declared, where))
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError(f"{where} must stay below the source root")
    root = source_root.resolve()
    path = root / relative
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"{where} is not a regular non-symlink file: {relative}")
    try:
        path.resolve().relative_to(root)
    except ValueError as error:
        raise ValueError(f"{where} escapes the source root: {relative}") from error
    return path


def _validate_generation(row: dict[str, Any], where: str) -> None:
    origin = _object(row.get("origin"), f"{where}.origin")
    kind = _text(origin.get("kind"), f"{where}.origin.kind")
    mutation = row.get("mutation")
    if mutation is not None:
        mutation = _object(mutation, f"{where}.mutation")
    if kind != "generated":
        _text(origin.get("tool"), f"{where}.origin.tool")
        _text(origin.get("version"), f"{where}.origin.version")
    generated = kind == "generated" or mutation is not None
    if not generated:
        return
    metadata = mutation if mutation is not None else origin
    _text(metadata.get("generator"), f"{where}.generation.generator")
    _text(metadata.get("version"), f"{where}.generation.version")
    _integer(metadata.get("seed"), f"{where}.generation.seed")


def _validate_build_context(value: Any, where: str) -> None:
    if value is None:
        return
    context = _object(value, where)
    _require_keys(
        context,
        frozenset(
            {
                "language_standard",
                "target_triple",
                "compiler_arguments",
                "working_directory",
                "macro_definitions",
                "include_snapshot",
            }
        ),
        where,
    )
    for key in ("language_standard", "target_triple", "working_directory"):
        _text(context.get(key), f"{where}.{key}")
    arguments = context.get("compiler_arguments")
    if not isinstance(arguments, list) or not all(
        isinstance(argument, str) for argument in arguments
    ):
        raise ValueError(f"{where}.compiler_arguments must be a list of strings")
    definitions = _object(
        context.get("macro_definitions"), f"{where}.macro_definitions"
    )
    if not all(
        isinstance(key, str) and isinstance(value, str)
        for key, value in definitions.items()
    ):
        raise ValueError(f"{where}.macro_definitions must map strings to strings")
    _text(context.get("include_snapshot"), f"{where}.include_snapshot")


def validate_manifest(
    rows: list[dict[str, Any]], source_root: Path
) -> dict[str, dict[str, Any]]:
    """Validate identities, source hashes, slices, provenance, and overlaps."""
    by_id: dict[str, dict[str, Any]] = {}
    slices: dict[str, list[tuple[int, int, str]]] = {}
    for index, row in enumerate(rows):
        where = f"manifest[{index}]"
        _require_keys(row, MANIFEST_FIELDS, where)
        if row.get("schema") != SCHEMA:
            raise ValueError(f"{where}.schema must equal {SCHEMA}")
        specimen_id = _text(row.get("id"), f"{where}.id")
        if specimen_id in by_id:
            raise ValueError(f"duplicate specimen id: {specimen_id}")
        _text(row.get("family"), f"{where}.family")
        source_path = _text(row.get("source_path"), f"{where}.source_path")
        path = _source_file(source_root, source_path, f"{where}.source_path")
        expected_hash = _text(row.get("source_sha256"), f"{where}.source_sha256")
        if SHA256.fullmatch(expected_hash) is None:
            raise ValueError(f"{where}.source_sha256 must be lowercase SHA-256")
        actual_hash = hashlib.sha256(path.read_bytes()).hexdigest()
        if actual_hash != expected_hash:
            raise ValueError(
                f"{where}.source_sha256 drift: expected {expected_hash}, got {actual_hash}"
            )
        span = _object(row.get("slice"), f"{where}.slice")
        _require_keys(span, frozenset({"start_byte", "end_byte"}), f"{where}.slice")
        start = _integer(span.get("start_byte"), f"{where}.slice.start_byte")
        end = _integer(span.get("end_byte"), f"{where}.slice.end_byte")
        if start >= end or end > path.stat().st_size:
            raise ValueError(
                f"{where}.slice must be non-empty and inside the source file"
            )
        _text(row.get("dialect"), f"{where}.dialect")
        _string_list(row.get("expected_functions"), f"{where}.expected_functions")
        oracle = _object(row.get("oracle"), f"{where}.oracle")
        _require_keys(oracle, frozenset({"kind", "ref"}), f"{where}.oracle")
        _text(oracle.get("kind"), f"{where}.oracle.kind")
        _text(oracle.get("ref"), f"{where}.oracle.ref")
        _validate_generation(row, where)
        _validate_build_context(row.get("build_context"), f"{where}.build_context")
        for other_start, other_end, other_id in slices.setdefault(source_path, []):
            if max(start, other_start) < min(end, other_end):
                raise ValueError(
                    f"overlapping slices in {source_path}: {other_id} and {specimen_id}"
                )
        slices[source_path].append((start, end, specimen_id))
        by_id[specimen_id] = row
    return by_id


def _optional_count(value: Any, where: str) -> None:
    if value is not None:
        _integer(value, where)


def _validate_result_row(
    row: dict[str, Any], index: int, manifest_hash: str, specimen_ids: set[str]
) -> tuple[str, tuple[str, str, str]]:
    where = f"results[{index}]"
    _require_keys(row, RESULT_FIELDS, where)
    if row.get("schema") != SCHEMA:
        raise ValueError(f"{where}.schema must equal {SCHEMA}")
    run_id = _text(row.get("run_id"), f"{where}.run_id")
    if row.get("manifest_sha256") != manifest_hash:
        raise ValueError(f"{where}.manifest_sha256 does not identify this manifest")
    tool = _object(row.get("tool"), f"{where}.tool")
    _require_keys(tool, frozenset({"name", "version", "adapter"}), f"{where}.tool")
    tool_id = (
        _text(tool.get("name"), f"{where}.tool.name"),
        _text(tool.get("version"), f"{where}.tool.version"),
        _text(tool.get("adapter"), f"{where}.tool.adapter"),
    )
    execution = _object(row.get("execution"), f"{where}.execution")
    _require_keys(
        execution,
        frozenset(
            {
                "host_id",
                "platform",
                "python",
                "command",
                "timeout_ns",
                "memory_limit_bytes",
                "source_revision",
                "source_dirty",
            }
        ),
        f"{where}.execution",
    )
    for key in ("host_id", "platform", "python", "source_revision"):
        _text(execution.get(key), f"{where}.execution.{key}")
    command = execution.get("command")
    if (
        not isinstance(command, list)
        or not command
        or not all(isinstance(argument, str) for argument in command)
    ):
        raise ValueError(f"{where}.execution.command must be a non-empty string list")
    _integer(execution.get("timeout_ns"), f"{where}.execution.timeout_ns", minimum=1)
    _optional_count(
        execution.get("memory_limit_bytes"),
        f"{where}.execution.memory_limit_bytes",
    )
    if not isinstance(execution.get("source_dirty"), bool):
        raise ValueError(f"{where}.execution.source_dirty must be boolean")
    specimen_id = _text(row.get("specimen_id"), f"{where}.specimen_id")
    if specimen_id not in specimen_ids:
        raise ValueError(f"{where}.specimen_id is not in the manifest: {specimen_id}")
    status = row.get("status")
    if status not in RESULT_STATUSES:
        raise ValueError(f"{where}.status is not a recognized outcome: {status!r}")
    failure = row.get("failure")
    if status == "success":
        if failure is not None:
            raise ValueError(f"{where}.failure must be null for success")
    else:
        failure = _object(failure, f"{where}.failure")
        _require_keys(failure, frozenset({"kind", "message"}), f"{where}.failure")
        _text(failure.get("kind"), f"{where}.failure.kind")
        _text(failure.get("message"), f"{where}.failure.message")
    timing = _object(row.get("timing_ns"), f"{where}.timing_ns")
    _require_keys(
        timing,
        frozenset({"startup", "analysis", "serialization"}),
        f"{where}.timing_ns",
    )
    for key in ("startup", "analysis", "serialization"):
        _optional_count(timing.get(key), f"{where}.timing_ns.{key}")
    _optional_count(row.get("peak_rss_bytes"), f"{where}.peak_rss_bytes")
    diagnostics = _object(row.get("diagnostics"), f"{where}.diagnostics")
    _require_keys(
        diagnostics,
        frozenset({"errors", "warnings", "recovery_nodes"}),
        f"{where}.diagnostics",
    )
    for key in ("errors", "warnings", "recovery_nodes"):
        _optional_count(diagnostics.get(key), f"{where}.diagnostics.{key}")
    yielded = _object(row.get("yield"), f"{where}.yield")
    _require_keys(
        yielded, frozenset({"functions", "covered_source_bytes"}), f"{where}.yield"
    )
    # Damaged sources may recover duplicate function spellings.  They are
    # measured output, not identities, so retaining duplicates is essential.
    _string_list(
        yielded.get("functions"),
        f"{where}.yield.functions",
        require_unique=False,
        require_nonempty=False,
    )
    _optional_count(
        yielded.get("covered_source_bytes"), f"{where}.yield.covered_source_bytes"
    )
    claims = _object(row.get("claims"), f"{where}.claims")
    _require_keys(
        claims, frozenset({"syntax_complete", "cfg_complete"}), f"{where}.claims"
    )
    for key in ("syntax_complete", "cfg_complete"):
        if claims.get(key) is not None and not isinstance(claims.get(key), bool):
            raise ValueError(f"{where}.claims.{key} must be boolean or null")
    artifacts = _object(row.get("artifacts"), f"{where}.artifacts")
    _require_keys(
        artifacts, frozenset({"normalized_ast", "normalized_cfg"}), f"{where}.artifacts"
    )
    for key in ("normalized_ast", "normalized_cfg"):
        if artifacts.get(key) is not None and not isinstance(artifacts.get(key), str):
            raise ValueError(f"{where}.artifacts.{key} must be a string or null")
    return run_id, tool_id


def validate_results(
    rows: list[dict[str, Any]], manifest_rows: list[dict[str, Any]]
) -> tuple[str, tuple[str, str, str]]:
    """Require exactly one result for every specimen from one run and tool."""
    manifest_hash = manifest_sha256(manifest_rows)
    specimen_ids = {row["id"] for row in manifest_rows}
    identities = {
        _validate_result_row(row, index, manifest_hash, specimen_ids)
        for index, row in enumerate(rows)
    }
    if len(identities) != 1:
        raise ValueError("one result file must contain exactly one run/tool identity")
    executions = {
        json.dumps(row["execution"], sort_keys=True, separators=(",", ":"))
        for row in rows
    }
    if len(executions) != 1:
        raise ValueError("one result file must contain exactly one execution identity")
    result_ids = [row["specimen_id"] for row in rows]
    seen: set[str] = set()
    duplicates: set[str] = set()
    for specimen_id in result_ids:
        if specimen_id in seen:
            duplicates.add(specimen_id)
        seen.add(specimen_id)
    if duplicates:
        raise ValueError(f"duplicate result specimens: {sorted(duplicates)}")
    missing = sorted(specimen_ids.difference(result_ids))
    if missing:
        raise ValueError(f"missing result specimens: {missing}")
    return next(iter(identities))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)
    manifest_parser = subparsers.add_parser("manifest")
    manifest_parser.add_argument("manifest", type=Path)
    manifest_parser.add_argument("--source-root", type=Path, default=Path.cwd())
    results_parser = subparsers.add_parser("results")
    results_parser.add_argument("manifest", type=Path)
    results_parser.add_argument("results", type=Path)
    results_parser.add_argument("--source-root", type=Path, default=Path.cwd())
    args = parser.parse_args()

    manifest_rows = load_jsonl(args.manifest)
    specimens = validate_manifest(manifest_rows, args.source_root)
    summary: dict[str, Any] = {
        "schema": SCHEMA,
        "manifest_sha256": manifest_sha256(manifest_rows),
        "specimens": len(specimens),
    }
    if args.command == "results":
        result_rows = load_jsonl(args.results)
        run_id, tool = validate_results(result_rows, manifest_rows)
        summary.update({"results": len(result_rows), "run_id": run_id, "tool": tool})
    print(json.dumps(summary, sort_keys=True))


if __name__ == "__main__":
    main()
