"""Fail closed unless every current nonzero Joern comparison has proof evidence."""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / "docs/benchmarks/data"


def _load(name: str) -> dict[str, Any]:
    return json.loads((DATA / name).read_text())


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--comparison", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.comparison = args.comparison.resolve()
    args.output = args.output.resolve()
    comparison = json.loads(args.comparison.read_text())
    baseline = _load("joern-decbench-2026-09-15.json")
    original = {
        (row["path"], function["name"])
        for row in baseline["files"]
        for function in row["functions"]
        if function["vj_ged"] != 0
    }
    current = {
        (row["path"], function["name"])
        for row in comparison["files"]
        for function in row["functions"]
        if function["vj_ged"] != 0
    }
    evidence: dict[tuple[str, str], str] = {}

    macro = _load("joern-macro-adjudication-2026-09-15.json")
    for row in macro["functions"]:
        if (
            row["cindergraph_raw_vs_preprocessed"]["role_isomorphic"]
            and not row["joern_raw_vs_preprocessed"]["role_isomorphic"]
            and row["providers_on_preprocessed"]["role_isomorphic"]
        ):
            evidence[(row["path"], row["function"])] = "object-like macro artifact"

    function_macro = _load("joern-function-macro-adjudication-2026-09-15.json")
    for row in function_macro["functions"]:
        key = (row["path"], row["function"])
        if row["cindergraph_proven_correct"]:
            evidence[key] = "function macro or statement-expression artifact"
        elif (
            row["cindergraph_preprocessing_gap"]
            and row["providers_on_preprocessed"]["role_isomorphic"]
        ):
            evidence[key] = "provider preprocessing fix plus raw Joern macro artifact"

    for filename, label, predicate in (
        (
            "joern-entry-role-adjudication-2026-09-15.json",
            "invalid repeated Joern entry role",
            lambda row: (
                row["topology_isomorphic_ignoring_roles"]
                and row["cindergraph_entry_count"] == 1
                and row["cindergraph_entry_indegrees"] == [0]
            ),
        ),
        (
            "joern-offsetof-adjudication-2026-09-15.json",
            "compiler-verified offsetof artifact",
            lambda row: (
                row["cindergraph_invariant_under_literal_substitution"]
                and row["providers_equal_after_literal_substitution"]
            ),
        ),
    ):
        for row in _load(filename)["functions"]:
            if predicate(row):
                evidence[(row["path"], row["function"])] = label

    complex_rows = _load("joern-complex-adjudication-2026-09-15.json")["functions"]
    complex_path = (
        "crates/cindergraph/tests/decompiler_fixtures/src/217_complex_arithmetic.c"
    )
    for row in complex_rows:
        if (
            row["cindergraph_control_topology_invariant"]["role_isomorphic"]
            and row["providers_equal_after_straight_line_return_replacement"][
                "role_isomorphic"
            ]
        ):
            evidence[(complex_path, row["function"])] = "complex-operator artifact"

    branch = _load("joern-branch-hint-adjudication-2026-09-15.json")
    if (
        branch["cindergraph"]["role_isomorphic"]
        and not branch["joern"]["role_isomorphic"]
        and branch["providers_on_unhinted_control_role_isomorphic"]
    ):
        evidence[
            (
                "crates/cindergraph/tests/decompiler_fixtures/src/119_branch_hints.c",
                "hinted_validation",
            )
        ] = "branch-hint artifact"

    final = _load("joern-final-adjudication-2026-09-15.json")
    computed = final["computed_goto"]
    if computed["role_isomorphic_after_removing_only_joern_orphans"]:
        evidence[(computed["path"], computed["function"])] = "unreachable Joern orphan"
    conditional = final["conditional_store"]
    if (
        conditional["cindergraph_raw_vs_sequenced_role_isomorphic"]
        and conditional["providers_on_sequenced_control_role_isomorphic"]
    ):
        evidence[(conditional["path"], conditional["function"])] = (
            "C-permitted conditional-store evaluation order"
        )

    missing = sorted(current - evidence.keys())
    stale = sorted(evidence.keys() - current)
    covered = sorted(current & evidence.keys())
    result = {
        "schema": 1,
        "comparison": str(args.comparison.relative_to(ROOT)),
        "shared_functions": comparison["summary"]["shared_function_instances"],
        "exact_functions": comparison["summary"]["shared_vj_ged_zero"],
        "nonzero_functions": len(current),
        "baseline_nonzero_functions": len(original),
        "became_exact_functions": len(original - current),
        "new_nonzero_functions": [list(item) for item in sorted(current - original)],
        "covered_nonzero_functions": len(covered),
        "missing": [list(item) for item in missing],
        "stale_evidence": [list(item) for item in stale],
        "classes": dict(sorted(Counter(evidence[item] for item in covered).items())),
        "functions": [
            {
                "path": path,
                "function": function,
                "disposition": evidence[(path, function)],
            }
            for path, function in covered
        ],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    if (
        missing
        or stale
        or len(covered) != len(current)
        or len(original) != 72
        or len(original - current) != 36
        or current - original
    ):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
