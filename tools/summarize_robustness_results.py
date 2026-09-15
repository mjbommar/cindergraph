#!/usr/bin/env python3
"""Summarize validated robustness results without shrinking denominators."""

from __future__ import annotations

import argparse
from collections import Counter
import importlib.util
import json
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]


def _module(path: Path, name: str) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"cannot load {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _distribution(values: list[int]) -> dict[str, int | None]:
    if not values:
        return {"count": 0, "median": None, "p95": None, "maximum": None}
    ordered = sorted(values)
    return {
        "count": len(ordered),
        "median": ordered[(len(ordered) - 1) // 2],
        "p95": ordered[((95 * len(ordered) + 99) // 100) - 1],
        "maximum": ordered[-1],
    }


def _claim_counts(rows: list[dict[str, Any]], name: str) -> dict[str, int]:
    values = Counter(
        "unavailable"
        if row["claims"][name] is None
        else str(row["claims"][name]).lower()
        for row in rows
    )
    return {key: values.get(key, 0) for key in ("true", "false", "unavailable")}


def _recovery_cell(
    rows: list[dict[str, Any]], manifest: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    expected_total = 0
    expected_found = 0
    for row in rows:
        expected = manifest[row["specimen_id"]]["expected_functions"]
        recovered = Counter(row["yield"]["functions"])
        expected_total += len(expected)
        expected_found += sum(recovered[name] > 0 for name in expected)
    return {
        "specimens": len(rows),
        "statuses": dict(sorted(Counter(row["status"] for row in rows).items())),
        "oracle_functions": {
            "found": expected_found,
            "total": expected_total,
            "coverage_available": expected_total > 0,
        },
        "zero_function_results": sum(not row["yield"]["functions"] for row in rows),
        "syntax_complete": _claim_counts(rows, "syntax_complete"),
    }


def _mutation_strata(
    rows: list[dict[str, Any]], manifest: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    grouped: dict[str, dict[int, list[dict[str, Any]]]] = {}
    scopes: dict[str, str] = {}
    for row in rows:
        mutation = manifest[row["specimen_id"]].get("mutation")
        if not isinstance(mutation, dict):
            continue
        operator = mutation.get("operator")
        severity = mutation.get("severity")
        if not isinstance(operator, str) or not isinstance(severity, int):
            continue
        grouped.setdefault(operator, {}).setdefault(severity, []).append(row)
        if isinstance(mutation.get("damage_scope"), str):
            scopes[operator] = mutation["damage_scope"]
    return {
        operator: {
            "damage_scope": scopes.get(operator),
            "all": _recovery_cell(
                [row for severity_rows in severities.values() for row in severity_rows],
                manifest,
            ),
            "severities": {
                str(severity): _recovery_cell(severity_rows, manifest)
                for severity, severity_rows in sorted(severities.items())
            },
        }
        for operator, severities in sorted(grouped.items())
    }


def _family_summary(
    rows: list[dict[str, Any]], manifest: dict[str, dict[str, Any]]
) -> dict[str, Any]:
    statuses = Counter(row["status"] for row in rows)
    expected_total = 0
    expected_found = 0
    source_bytes = 0
    for row in rows:
        specimen = manifest[row["specimen_id"]]
        expected = specimen["expected_functions"]
        recovered = Counter(row["yield"]["functions"])
        expected_total += len(expected)
        expected_found += sum(recovered[name] > 0 for name in expected)
        source_bytes += specimen["slice"]["end_byte"] - specimen["slice"]["start_byte"]
    timing = {
        name: _distribution(
            [
                row["timing_ns"][name]
                for row in rows
                if row["timing_ns"][name] is not None
            ]
        )
        for name in ("startup", "analysis", "serialization")
    }
    timing["peak_rss_bytes"] = _distribution(
        [row["peak_rss_bytes"] for row in rows if row["peak_rss_bytes"] is not None]
    )
    covered = [
        row["yield"]["covered_source_bytes"]
        for row in rows
        if row["yield"]["covered_source_bytes"] is not None
    ]
    return {
        "specimens": len(rows),
        "statuses": dict(sorted(statuses.items())),
        "syntax_complete": _claim_counts(rows, "syntax_complete"),
        "cfg_complete": _claim_counts(rows, "cfg_complete"),
        "zero_function_results": sum(not row["yield"]["functions"] for row in rows),
        "yielded_functions": sum(len(row["yield"]["functions"]) for row in rows),
        "oracle_functions": {
            "found": expected_found,
            "total": expected_total,
            "coverage_available": expected_total > 0,
        },
        "covered_source_bytes": {
            "reported_specimens": len(covered),
            "reported_total": sum(covered),
            "input_total": source_bytes,
        },
        "timing_ns": timing,
        "mutation_strata": _mutation_strata(rows, manifest),
    }


def summarize(
    manifest_rows: list[dict[str, Any]], result_sets: list[list[dict[str, Any]]]
) -> dict[str, Any]:
    """Return fixed-denominator summaries for validated result sets."""
    contract = _module(ROOT / "tools/robustness_contract.py", "robustness_contract")
    by_id = {row["id"]: row for row in manifest_rows}
    runs = []
    for rows in result_sets:
        run_id, tool = contract.validate_results(rows, manifest_rows)
        families: dict[str, list[dict[str, Any]]] = {}
        for row in rows:
            family = by_id[row["specimen_id"]]["family"]
            families.setdefault(family, []).append(row)
        runs.append(
            {
                "run_id": run_id,
                "tool": {"name": tool[0], "version": tool[1], "adapter": tool[2]},
                "execution": rows[0]["execution"],
                "all": _family_summary(rows, by_id),
                "families": {
                    family: _family_summary(family_rows, by_id)
                    for family, family_rows in sorted(families.items())
                },
            }
        )
    return {
        "schema": 1,
        "manifest_sha256": contract.manifest_sha256(manifest_rows),
        "runs": runs,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("manifest", type=Path)
    parser.add_argument("results", nargs="+", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--source-root", type=Path, default=ROOT)
    args = parser.parse_args()
    contract = _module(ROOT / "tools/robustness_contract.py", "robustness_contract")
    manifest_rows = contract.load_jsonl(args.manifest)
    contract.validate_manifest(manifest_rows, args.source_root)
    result_sets = [contract.load_jsonl(path) for path in args.results]
    document = (
        json.dumps(summarize(manifest_rows, result_sets), indent=2, sort_keys=True)
        + "\n"
    )
    if args.output is None:
        print(document, end="")
    else:
        if args.output.exists():
            raise ValueError(f"refusing to overwrite summary: {args.output}")
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(document, encoding="utf-8")


if __name__ == "__main__":
    main()
