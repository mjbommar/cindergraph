#!/usr/bin/env python3
"""Materialize the existing Cindergraph robustness populations reproducibly."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import random
import re
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MANIFEST = ROOT / "docs/benchmarks/manifests/robustness-v1.jsonl"
DEFAULT_MUTATIONS = ROOT / "docs/benchmarks/corpus/mutations"
CONTROLLED_MUTATIONS = ROOT / "docs/benchmarks/corpus/controlled-recovery"
FROZEN_CLEAN = ROOT / "docs/benchmarks/data/joern-decbench-2026-09-15.json"
DIALECT_FIXTURES = ROOT / "tests/fixtures/decompiler_dialects"
CASE_START = re.compile(r"^/\* case: ", re.MULTILINE)
META = re.compile(
    r"^(?: \*|/\*) (case|provenance|source|expect|gap): (.*)$", re.MULTILINE
)
INSERTIONS = ("\x00", "\ufffe", "λ", "\r\n", '"', "/*", "}", "(", "#if X\n")


def _sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _relative(path: Path, root: Path) -> str:
    return path.relative_to(root).as_posix()


def mutation_bases(root: Path = ROOT) -> list[Path]:
    """Return the exact ordered 211-file population used by the totality test."""
    return (
        [root / "tests/fixtures/sizeof_local.c"]
        + sorted(
            (root / "crates/cindergraph/tests/decompiler_fixtures/src").glob("*.c")
        )
        + sorted((root / "tests/decbench_corpus/src").glob("*.c"))
    )


def mutated_source(seed: int, root: Path = ROOT) -> tuple[str, Path]:
    """Reproduce one existing seeded corruption and name its parent fixture."""
    bases = mutation_bases(root)
    rng = random.Random(seed)
    parent = bases[seed % len(bases)]
    text = parent.read_text(encoding="utf-8")[:8192]
    for _ in range(4):
        start = rng.randrange(len(text) + 1)
        end = min(len(text), start + rng.randrange(12))
        text = text[:start] + rng.choice(INSERTIONS) + text[end:]
    if seed % 3 == 0:
        text = text[: rng.randrange(len(text) + 1)]
    return text, parent


def clean_rows(root: Path = ROOT) -> list[dict[str, Any]]:
    """Project the frozen, audited 210-file comparison population."""
    record = json.loads(
        (root / FROZEN_CLEAN.relative_to(ROOT)).read_text(encoding="utf-8")
    )
    rows = []
    for file_row in record["files"]:
        relative = Path(file_row["path"])
        source = root / relative
        data = source.read_bytes()
        rows.append(
            {
                "schema": 1,
                "id": f"clean/{relative.as_posix()}",
                "family": "fixture-clean",
                "source_path": relative.as_posix(),
                "source_sha256": _sha256(data),
                "slice": {"start_byte": 0, "end_byte": len(data)},
                "origin": {
                    "kind": "repository",
                    "tool": "cindergraph-fixtures",
                    "version": "joern-decbench-2026-09-15",
                },
                "dialect": "ordinary",
                "build_context": None,
                "expected_functions": file_row["cindergraph_functions"],
                "oracle": {
                    "kind": "frozen-comparison-record",
                    "ref": (
                        "docs/benchmarks/data/joern-decbench-2026-09-15.json#"
                        f"files[path={relative.as_posix()}].cindergraph_functions"
                    ),
                },
                "mutation": None,
            }
        )
    return rows


def dialect_rows(root: Path = ROOT) -> list[dict[str, Any]]:
    """Materialize every explicitly declared raw decompiler-dialect case."""
    rows = []
    for path in sorted((root / DIALECT_FIXTURES.relative_to(ROOT)).glob("*.c")):
        text = path.read_text(encoding="utf-8")
        data = path.read_bytes()
        starts = [match.start() for match in CASE_START.finditer(text)]
        for start, end in zip(starts, starts[1:] + [len(text)]):
            chunk = text[start:end]
            header, separator, body = chunk.partition(" */\n")
            if not separator:
                raise ValueError(f"unterminated dialect metadata in {path}:{start}")
            metadata = dict(META.findall(header))
            required = {"case", "provenance", "source", "expect"}
            if missing := required.difference(metadata):
                raise ValueError(
                    f"{path}:{start}: missing dialect metadata {sorted(missing)}"
                )
            body_start = len(text[:start].encode("utf-8")) + len(
                (header + separator).encode("utf-8")
            )
            body_end = body_start + len(body.encode("utf-8"))
            expected = [] if metadata["expect"] == "-" else [metadata["expect"]]
            relative = path.relative_to(root)
            rows.append(
                {
                    "schema": 1,
                    "id": f"dialect/{path.stem}/{metadata['case']}",
                    "family": "decompiler",
                    "source_path": relative.as_posix(),
                    "source_sha256": _sha256(data),
                    "slice": {"start_byte": body_start, "end_byte": body_end},
                    "origin": {
                        "kind": metadata["provenance"].lower(),
                        "tool": path.stem,
                        "version": metadata["source"],
                    },
                    "dialect": "decompiled",
                    "build_context": None,
                    "expected_functions": expected,
                    "oracle": {
                        "kind": "declared-fixture-case",
                        "ref": f"python/tests/test_decompiler_dialects.py::{path.stem}:{metadata['case']}",
                    },
                    "mutation": None,
                }
            )
    return rows


def mutation_rows(
    root: Path = ROOT, mutation_dir: Path = DEFAULT_MUTATIONS
) -> tuple[list[dict[str, Any]], dict[Path, bytes]]:
    """Build all 256 deterministic mutation rows and their stored bytes."""
    rows = []
    files = {}
    for seed in range(256):
        text, parent = mutated_source(seed, root)
        data = text.encode("utf-8")
        path = mutation_dir / f"seed-{seed:03}.c"
        relative = _relative(path, root)
        files[path] = data
        rows.append(
            {
                "schema": 1,
                "id": f"mutation/seed-{seed:03}",
                "family": "broken",
                "source_path": relative,
                "source_sha256": _sha256(data),
                "slice": {"start_byte": 0, "end_byte": len(data)},
                "origin": {
                    "kind": "generated",
                    "generator": "cindergraph-seeded-corruption",
                    "version": "1",
                    "seed": seed,
                },
                "dialect": "ordinary",
                "build_context": None,
                "expected_functions": [],
                "oracle": {
                    "kind": "totality-determinism",
                    "ref": "python/tests/test_mutated_source_totality.py",
                },
                "mutation": {
                    "generator": "cindergraph-seeded-corruption",
                    "version": "1",
                    "seed": seed,
                    "parent": _relative(parent, root),
                    "random_edits": 4,
                    "prefix_truncation": seed % 3 == 0,
                },
            }
        )
    return rows, files


def _controlled_variant(operator: str, severity: int, target: str) -> tuple[str, bool]:
    """Damage only the target region and say whether a right neighbour remains."""
    if operator == "missing_semicolon":
        return target.replace(";", "", severity), True
    if operator == "missing_paren":
        choices = (
            target.replace("y > 4)", "y > 4", 1),
            target.replace("int x)", "int x", 1),
            target.replace(")", "", 2),
        )
        return choices[severity - 1], True
    if operator == "missing_brace":
        if severity == 1:
            return target.replace("y *= 2; }", "y *= 2;", 1), True
        if severity == 2:
            return target.rsplit("}", 1)[0], True
        return target.replace("}", ""), True
    if operator == "garbage_token":
        garbage = ("@", "\x00", "\ufffe")[severity - 1]
        return target.replace("x + 1", f"x {garbage} + 1", 1), True
    if operator == "unmatched_open":
        opening = ("(", "[", "{")[severity - 1]
        return target.replace("x + 1", f"{opening}x + 1", 1), True
    if operator == "missing_identifier":
        choices = (
            target.replace("int y", "int", 1),
            target.replace("y > 4", "> 4", 1),
            target.replace("return y", "return", 1),
        )
        return choices[severity - 1], True
    if operator == "missing_type":
        choices = (
            target.replace("int y", "y", 1),
            target.replace("static int", "static", 1),
            target.replace("int x", "x", 1),
        )
        return choices[severity - 1], True
    if operator == "unterminated_literal":
        return target.replace("x + 1", '"damaged' + ("x" * severity), 1), True
    if operator == "unterminated_comment":
        return target.replace("x + 1", f"x + /* damage-{severity}", 1), True
    if operator == "damaged_directive":
        directive = ("#if\n", "#ifdef\n", "#if (\n")[severity - 1]
        damaged = target.replace("int y", f"{directive}  int y", 1)
        return damaged.replace("if (y > 4)", "#endif\n  if (y > 4)", 1), True
    if operator == "truncation":
        fractions = (3, 2, 1)
        return target[: len(target) * fractions[severity - 1] // 4], False
    raise ValueError(f"unknown controlled mutation operator: {operator}")


def controlled_recovery_rows(
    root: Path = ROOT, mutation_dir: Path = CONTROLLED_MUTATIONS
) -> tuple[list[dict[str, Any]], dict[Path, bytes]]:
    """Construct damaged targets with byte-identical neighbour oracles."""
    operators = (
        "missing_semicolon",
        "missing_paren",
        "missing_brace",
        "garbage_token",
        "unmatched_open",
        "missing_identifier",
        "missing_type",
        "unterminated_literal",
        "unterminated_comment",
        "damaged_directive",
        "truncation",
    )
    rows = []
    files = {}
    for operator_index, operator in enumerate(operators):
        for severity in range(1, 4):
            seed = operator_index * 3 + severity
            suffix = f"{operator_index:02}_{severity}"
            left = f"left_guard_{suffix}"
            target_name = f"damaged_target_{suffix}"
            right = f"right_guard_{suffix}"
            left_source = f"static int {left}(int x) {{ return x + 11; }}\n"
            target = (
                f"static int {target_name}(int x) {{ int y = x + 1; "
                "if (y > 4) { y *= 2; } return y; }\n"
            )
            right_source = f"static int {right}(int x) {{ return x - 7; }}\n"
            damaged, retain_right = _controlled_variant(operator, severity, target)
            text = left_source + damaged + (right_source if retain_right else "")
            data = text.encode("utf-8")
            path = mutation_dir / f"{operator}-s{severity}.c"
            files[path] = data
            protected = [left] + ([right] if retain_right else [])
            if operator == "truncation":
                damage_scope = "physical_truncation"
            elif operator in {"unterminated_literal", "unterminated_comment"}:
                damage_scope = "lexical_spill"
            elif operator == "damaged_directive":
                damage_scope = "preprocessor_local"
            else:
                damage_scope = "target_local"
            rows.append(
                {
                    "schema": 1,
                    "id": f"controlled/{operator}/severity-{severity}",
                    "family": "broken-controlled",
                    "source_path": _relative(path, root),
                    "source_sha256": _sha256(data),
                    "slice": {"start_byte": 0, "end_byte": len(data)},
                    "origin": {
                        "kind": "generated",
                        "generator": "cindergraph-controlled-recovery",
                        "version": "1",
                        "seed": seed,
                    },
                    "dialect": "ordinary",
                    "build_context": None,
                    "expected_functions": protected,
                    "oracle": {
                        "kind": "constructed-unaffected-neighbours",
                        "ref": "tools/materialize_robustness_manifest.py#controlled_recovery_rows",
                    },
                    "mutation": {
                        "generator": "cindergraph-controlled-recovery",
                        "version": "1",
                        "seed": seed,
                        "operator": operator,
                        "severity": severity,
                        "damage_scope": damage_scope,
                        "damaged_function": target_name,
                        "protected_functions": protected,
                    },
                }
            )
    return rows, files


def materialized(root: Path = ROOT) -> tuple[list[dict[str, Any]], dict[Path, bytes]]:
    """Return every existing population in stable lane and specimen order."""
    mutation_dir = root / DEFAULT_MUTATIONS.relative_to(ROOT)
    mutations, files = mutation_rows(root, mutation_dir)
    controlled_dir = root / CONTROLLED_MUTATIONS.relative_to(ROOT)
    controlled, controlled_files = controlled_recovery_rows(root, controlled_dir)
    files.update(controlled_files)
    rows = clean_rows(root) + dialect_rows(root) + mutations + controlled
    return rows, files


def render(rows: list[dict[str, Any]]) -> bytes:
    """Serialize stable, human-diffable JSONL."""
    return b"".join(
        (json.dumps(row, sort_keys=True, separators=(",", ":")) + "\n").encode()
        for row in rows
    )


def _contract_module(root: Path) -> Any:
    spec = importlib.util.spec_from_file_location(
        "robustness_contract", root / "tools/robustness_contract.py"
    )
    if spec is None or spec.loader is None:
        raise RuntimeError("cannot load robustness contract")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify(root: Path = ROOT) -> tuple[int, str]:
    """Fail when committed generated bytes or manifest identity have drifted."""
    rows, files = materialized(root)
    manifest = root / DEFAULT_MANIFEST.relative_to(ROOT)
    if not manifest.is_file() or manifest.read_bytes() != render(rows):
        raise ValueError(f"generated robustness manifest is stale: {manifest}")
    for path, expected in files.items():
        if not path.is_file() or path.read_bytes() != expected:
            raise ValueError(f"generated mutation specimen is stale: {path}")
    unexpected = set(
        (root / DEFAULT_MUTATIONS.relative_to(ROOT)).glob("*.c")
    ).difference(files)
    unexpected.update(
        set((root / CONTROLLED_MUTATIONS.relative_to(ROOT)).glob("*.c")).difference(
            files
        )
    )
    if unexpected:
        raise ValueError(
            f"unexpected generated mutation specimens: {sorted(unexpected)}"
        )
    contract = _contract_module(root)
    loaded = contract.load_jsonl(manifest)
    contract.validate_manifest(loaded, root)
    return len(rows), contract.manifest_sha256(loaded)


def write(root: Path = ROOT) -> tuple[int, str]:
    """Write deterministic generated specimens and manifest, then verify them."""
    rows, files = materialized(root)
    for path, data in files.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    manifest = root / DEFAULT_MANIFEST.relative_to(ROOT)
    manifest.parent.mkdir(parents=True, exist_ok=True)
    manifest.write_bytes(render(rows))
    return verify(root)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    count, digest = verify() if args.check else write()
    print(json.dumps({"schema": 1, "specimens": count, "manifest_sha256": digest}))


if __name__ == "__main__":
    main()
