"""Adjudicate Joern CFG forks invented around C complex-part operators."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import time
from pathlib import Path

from adjudicate_joern_macro_calls import _compare, _joern

from cindergraph.source_cfg import cfgs_from_decompiled


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = (
    ROOT / "crates/cindergraph/tests/decompiler_fixtures/src/217_complex_arithmetic.c"
)
REPLACEMENTS = {
    "complex_multiply": "return ar;",
    "complex_add_conj": "return ar;",
    "complex_float_multiply": "return ar;",
    "complex_through_call": "return ar;",
    "complex_array_sum": "return count;",
}


def _replace_final_return(text: str, name: str, replacement: str) -> str:
    match = re.search(rf"\b{re.escape(name)}\s*\([^{{]*\)\s*{{", text)
    if match is None:
        raise RuntimeError(f"cannot find {name}")
    opening = text.find("{", match.start())
    depth = 0
    closing = opening
    for closing in range(opening, len(text)):
        if text[closing] == "{":
            depth += 1
        elif text[closing] == "}":
            depth -= 1
            if depth == 0:
                break
    body = text[opening:closing]
    returns = list(re.finditer(r"\breturn\b[^;]*;", body))
    if not returns:
        raise RuntimeError(f"cannot find a return in {name}")
    target = returns[-1]
    lo = opening + target.start()
    hi = opening + target.end()
    return text[:lo] + replacement + text[hi:]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    raw = FIXTURE.read_text()
    raw_cinder = cfgs_from_decompiled(raw)
    raw_joern, _ = _joern(raw)
    rows = []
    for name, replacement in REPLACEMENTS.items():
        transformed = _replace_final_return(raw, name, replacement)
        transformed_cinder = cfgs_from_decompiled(transformed)
        transformed_joern, _ = _joern(transformed)
        rows.append(
            {
                "function": name,
                "source_equivalent_control_transform": replacement,
                "raw": {
                    "cindergraph_nodes": raw_cinder[name].number_of_nodes(),
                    "cindergraph_edges": raw_cinder[name].number_of_edges(),
                    "joern_nodes": raw_joern[name].number_of_nodes(),
                    "joern_edges": raw_joern[name].number_of_edges(),
                },
                "cindergraph_control_topology_invariant": _compare(
                    raw_cinder.get(name), transformed_cinder.get(name)
                ),
                "providers_equal_after_straight_line_return_replacement": _compare(
                    transformed_cinder.get(name), transformed_joern.get(name)
                ),
            }
        )
    result = {
        "schema": 1,
        "generated_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "scope": "C complex-part operator control-topology adjudication",
        "semantic_basis": (
            "replacing a straight-line arithmetic return expression changes values, "
            "not control-flow alternatives"
        ),
        "population_sha256": hashlib.sha256(raw.encode()).hexdigest(),
        "summary": {
            "functions": len(rows),
            "cindergraph_control_topology_invariant": sum(
                row["cindergraph_control_topology_invariant"]["role_isomorphic"] is True
                for row in rows
            ),
            "providers_equal_after_replacement": sum(
                row["providers_equal_after_straight_line_return_replacement"][
                    "role_isomorphic"
                ]
                is True
                for row in rows
            ),
        },
        "functions": rows,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result["summary"], indent=2))


if __name__ == "__main__":
    main()
