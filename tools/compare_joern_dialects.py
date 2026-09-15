"""Compare raw Cindergraph recovery with DecBench-sanitized Joern recovery."""

from __future__ import annotations

import argparse
import hashlib
import importlib.metadata
import importlib.util
import json
import platform
import re
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

from decbench.metrics.vj_ged import vj_ged
from decbench.utils.cfg import preprocess_decompiled_c, sanitize_decompiled_c
from pyjoern import parse_source

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests/fixtures/decompiler_dialects"
CASE_START = re.compile(r"^/\* case: ", re.MULTILINE)
META = re.compile(
    r"^(?: \*|/\*) (case|provenance|source|expect|gap): (.*)$", re.MULTILINE
)


def _sha256(parts: list[bytes]) -> str:
    digest = hashlib.sha256()
    for part in parts:
        digest.update(len(part).to_bytes(8, "big"))
        digest.update(part)
    return digest.hexdigest()


def _source_digest() -> str:
    paths = sorted(
        path
        for base in (ROOT / "crates", ROOT / "python/cindergraph")
        for path in base.rglob("*")
        if path.is_file() and path.suffix in {".rs", ".py", ".toml"}
    )
    return _sha256(
        [
            str(path.relative_to(ROOT)).encode() + b"\0" + path.read_bytes()
            for path in paths
        ]
    )


def _cases() -> list[dict[str, str | None]]:
    cases = []
    for path in sorted(FIXTURES.glob("*.c")):
        text = path.read_text()
        starts = [match.start() for match in CASE_START.finditer(text)]
        for start, end in zip(starts, starts[1:] + [len(text)]):
            header, separator, body = text[start:end].partition(" */\n")
            if not separator:
                raise ValueError(f"unterminated case in {path}")
            metadata = dict(META.findall(header))
            cases.append(
                {
                    "id": f"{path.stem}:{metadata['case']}",
                    "fixture": path.name,
                    "provenance": metadata["provenance"],
                    "source": metadata["source"],
                    "expect": metadata["expect"],
                    "gap": metadata.get("gap"),
                    "body": body,
                }
            )
    return cases


def _joern(text: str) -> dict[str, Any]:
    prepared = preprocess_decompiled_c(sanitize_decompiled_c(text))
    with tempfile.NamedTemporaryFile(mode="w", suffix=".c") as source:
        source.write(prepared)
        source.flush()
        parsed = parse_source(Path(source.name), no_ddg=True, no_ast=True)
    if parsed is None:
        return {}
    return {
        function.name: function.cfg
        for function in parsed.values()
        if getattr(function, "cfg", None) is not None
    }


def compare() -> dict[str, Any]:
    cases = _cases()
    rows = []
    for index, case in enumerate(cases, 1):
        print(f"[{index}/26] {case['id']}", flush=True)
        body = str(case["body"])
        metadata = {key: value for key, value in case.items() if key != "body"}
        started = time.perf_counter()
        cinder = cfgs_from_decompiled(body)
        cinder_seconds = time.perf_counter() - started
        started = time.perf_counter()
        try:
            joern = _joern(body)
            joern_error = None
        except Exception as error:  # noqa: BLE001 - provider failures are measurements
            joern = {}
            joern_error = f"{type(error).__name__}: {error}"
        joern_seconds = time.perf_counter() - started
        expected = str(case["expect"])
        both = expected != "-" and expected in cinder and expected in joern
        rows.append(
            {
                **metadata,
                "cindergraph_functions": sorted(cinder),
                "joern_functions": sorted(joern),
                "cindergraph_expected_recovered": expected in cinder,
                "joern_expected_recovered": expected in joern,
                "cindergraph_seconds": cinder_seconds,
                "joern_seconds": joern_seconds,
                "joern_error": joern_error,
                "vj_ged": float(vj_ged(cinder[expected], joern[expected]))
                if both
                else None,
            }
        )
    positive = [row for row in rows if row["expect"] != "-"]
    cinder_times = [row["cindergraph_seconds"] for row in rows]
    joern_times = [row["joern_seconds"] for row in rows]
    decbench_spec = importlib.util.find_spec("decbench")
    if decbench_spec is None or decbench_spec.origin is None:
        raise RuntimeError("cannot locate the imported DecBench checkout")
    decbench_root = Path(decbench_spec.origin).resolve().parents[1]
    status = subprocess.run(
        ["git", "status", "--porcelain=v1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    return {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "raw decompiler dialect cases for Cindergraph; DecBench-sanitized cases for Joern",
        "provenance": {
            "cindergraph_git_head": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=ROOT,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip(),
            "cindergraph_worktree_dirty": bool(status),
            "cindergraph_status_sha256": _sha256([status.encode()]),
            "cindergraph_source_sha256": _source_digest(),
            "decbench_git_head": subprocess.run(
                ["git", "rev-parse", "HEAD"],
                cwd=decbench_root,
                check=True,
                capture_output=True,
                text=True,
            ).stdout.strip(),
            "python": sys.version,
            "platform": platform.platform(),
            "pyjoern": importlib.metadata.version("pyjoern"),
            "decbench": importlib.metadata.version("decbench"),
        },
        "population": {
            "cases": len(cases),
            "sha256": _sha256(
                [
                    str(case["id"]).encode() + b"\0" + str(case["body"]).encode()
                    for case in cases
                ]
            ),
            "root": str(FIXTURES.relative_to(ROOT)),
        },
        "summary": {
            "cases": len(rows),
            "captured_cases": sum(row["provenance"] == "captured" for row in rows),
            "positive_expected_cases": len(positive),
            "cindergraph_expected_recovered": sum(
                row["cindergraph_expected_recovered"] for row in positive
            ),
            "joern_expected_recovered": sum(
                row["joern_expected_recovered"] for row in positive
            ),
            "both_expected_recovered": sum(
                row["cindergraph_expected_recovered"]
                and row["joern_expected_recovered"]
                for row in positive
            ),
            "shared_vj_ged_zero": sum(row["vj_ged"] == 0 for row in rows),
            "shared_vj_ged_nonzero": sum(
                row["vj_ged"] is not None and row["vj_ged"] != 0 for row in rows
            ),
            "joern_failures": sum(row["joern_error"] is not None for row in rows),
            "cindergraph_total_seconds": sum(cinder_times),
            "joern_total_seconds": sum(joern_times),
            "cindergraph_median_seconds": statistics.median(cinder_times),
            "joern_median_seconds": statistics.median(joern_times),
        },
        "cases": rows,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = compare()
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
