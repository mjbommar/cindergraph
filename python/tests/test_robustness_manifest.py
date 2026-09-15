"""The committed robustness population is reproducible and contract-valid."""

from importlib.util import module_from_spec, spec_from_file_location
import json
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "materialize_robustness_manifest",
    ROOT / "tools/materialize_robustness_manifest.py",
)
assert SPEC is not None and SPEC.loader is not None
materializer = module_from_spec(SPEC)
SPEC.loader.exec_module(materializer)


def test_committed_population_reproduces_and_validates() -> None:
    count, digest = materializer.verify(ROOT)
    assert count == 525
    assert len(digest) == 64


def test_population_lanes_have_fixed_denominators() -> None:
    path = ROOT / "docs/benchmarks/manifests/robustness-v1.jsonl"
    rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines()]
    families = {}
    for row in rows:
        families[row["family"]] = families.get(row["family"], 0) + 1
    assert families == {
        "fixture-clean": 210,
        "decompiler": 26,
        "broken": 256,
        "broken-controlled": 33,
    }
    assert sum(len(row["expected_functions"]) for row in rows[:210]) == 930


def test_dialect_slices_start_after_their_provenance_headers() -> None:
    rows = materializer.dialect_rows(ROOT)
    for row in rows:
        source = (ROOT / row["source_path"]).read_bytes()
        body = source[row["slice"]["start_byte"] : row["slice"]["end_byte"]]
        assert not body.startswith(b"/* case:")
        assert body.strip(), row["id"]


def test_controlled_recovery_oracles_are_constructed_outside_damaged_region() -> None:
    rows, files = materializer.controlled_recovery_rows(ROOT)
    assert len(rows) == 33
    assert {row["mutation"]["severity"] for row in rows} == {1, 2, 3}
    assert len({row["mutation"]["operator"] for row in rows}) == 11
    assert sum(len(row["expected_functions"]) for row in rows) == 63
    for row in rows:
        source = files[ROOT / row["source_path"]].decode("utf-8")
        for name in row["mutation"]["protected_functions"]:
            assert f"static int {name}(int x)" in source
        if row["mutation"]["damage_scope"] == "physical_truncation":
            assert len(row["expected_functions"]) == 1
        else:
            assert len(row["expected_functions"]) == 2
            assert row["mutation"]["damaged_function"] in source
