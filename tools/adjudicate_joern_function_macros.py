"""Adjudicate function-like/X-macro and statement-expression CFG differences."""

from __future__ import annotations

import argparse
import hashlib
import json
import time
from pathlib import Path

from adjudicate_joern_macro_calls import _compare, _joern, _preprocess

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "crates/cindergraph/tests/decompiler_fixtures/src/104_statement_expression.c": [
        "statement_expression_max",
        "single_evaluation",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/126_x_macros.c": [
        "total_weight",
        "apply_opcode",
    ],
    "crates/cindergraph/tests/decompiler_fixtures/src/131_obfuscated_composite.c": [
        "obfuscated_pipeline"
    ],
}


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    rows = []
    population = hashlib.sha256()
    for relative, names in TARGETS.items():
        raw = (ROOT / relative).read_text()
        population.update(relative.encode() + b"\0" + raw.encode())
        expanded = _preprocess(raw)
        cinder_raw = cfgs_from_decompiled(raw)
        cinder_expanded = cfgs_from_decompiled(expanded)
        joern_raw, _ = _joern(raw)
        joern_expanded, _ = _joern(expanded)
        for name in names:
            raw_invariant = _compare(cinder_raw.get(name), cinder_expanded.get(name))
            providers_expanded = _compare(
                cinder_expanded.get(name), joern_expanded.get(name)
            )
            rows.append(
                {
                    "path": relative,
                    "function": name,
                    "cindergraph_raw_vs_preprocessed": raw_invariant,
                    "joern_raw_vs_preprocessed": _compare(
                        joern_raw.get(name), joern_expanded.get(name)
                    ),
                    "providers_on_preprocessed": providers_expanded,
                    "cindergraph_proven_correct": (
                        raw_invariant["role_isomorphic"] is True
                        and providers_expanded["role_isomorphic"] is True
                    ),
                    "cindergraph_preprocessing_gap": (
                        raw_invariant["role_isomorphic"] is False
                        and providers_expanded["role_isomorphic"] is True
                    ),
                }
            )
    result = {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "local function-like macro preprocessing adjudication",
        "population_sha256": population.hexdigest(),
        "summary": {
            "functions": len(rows),
            "cindergraph_proven_correct": sum(
                row["cindergraph_proven_correct"] for row in rows
            ),
            "cindergraph_preprocessing_gaps": sum(
                row["cindergraph_preprocessing_gap"] for row in rows
            ),
        },
        "functions": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
