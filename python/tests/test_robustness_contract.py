"""The hostile-input benchmark cannot silently change its denominator."""

from copy import deepcopy
import hashlib
from importlib.util import module_from_spec, spec_from_file_location
import json
from pathlib import Path

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "robustness_contract", ROOT / "tools/robustness_contract.py"
)
assert SPEC is not None and SPEC.loader is not None
contract = module_from_spec(SPEC)
SPEC.loader.exec_module(contract)


def specimen(source: Path, specimen_id: str = "broken/example") -> dict:
    """Build one valid row around bytes owned by the test."""
    return {
        "schema": 1,
        "id": specimen_id,
        "family": "broken",
        "source_path": source.name,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "slice": {"start_byte": 0, "end_byte": source.stat().st_size},
        "origin": {"kind": "derived", "tool": "test", "version": "1"},
        "dialect": "ordinary",
        "build_context": None,
        "expected_functions": ["f"],
        "oracle": {"kind": "paired-source", "ref": "test:f"},
        "mutation": None,
    }


def result(row: dict, status: str = "success") -> dict:
    """Build one result that identifies the canonical manifest."""
    return {
        "schema": 1,
        "run_id": "run-1",
        "manifest_sha256": contract.manifest_sha256([row]),
        "tool": {"name": "cindergraph", "version": "0.1", "adapter": "abc"},
        "execution": {
            "host_id": "host-hash",
            "platform": "test-platform",
            "python": "3.test",
            "command": ["adapter", "manifest.jsonl"],
            "timeout_ns": 1_000_000_000,
            "memory_limit_bytes": None,
            "source_revision": "abc123",
            "source_dirty": False,
        },
        "specimen_id": row["id"],
        "status": status,
        "failure": None
        if status == "success"
        else {"kind": status, "message": "measured failure"},
        "timing_ns": {"startup": 0, "analysis": 10, "serialization": 2},
        "peak_rss_bytes": 100,
        "diagnostics": {"errors": 1, "warnings": 0, "recovery_nodes": 1},
        "yield": {"functions": ["f"], "covered_source_bytes": 8},
        "claims": {"syntax_complete": False, "cfg_complete": False},
        "artifacts": {"normalized_ast": "ast.json", "normalized_cfg": "cfg.json"},
    }


def test_valid_manifest_and_failure_complete_result_set(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(\n", encoding="utf-8")
    row = specimen(source)
    assert contract.validate_manifest([row], tmp_path) == {row["id"]: row}
    identity = contract.validate_results([result(row, "timeout")], [row])
    assert identity == ("run-1", ("cindergraph", "0.1", "abc"))


def test_manifest_hash_is_ordered_canonical_json() -> None:
    left = [{"schema": 1, "id": "a"}, {"schema": 1, "id": "b"}]
    reordered_keys = [{"id": "a", "schema": 1}, {"id": "b", "schema": 1}]
    assert contract.manifest_sha256(left) == contract.manifest_sha256(reordered_keys)
    assert contract.manifest_sha256(left) != contract.manifest_sha256(left[::-1])


def test_jsonl_loader_rejects_blank_or_non_object_rows(tmp_path: Path) -> None:
    path = tmp_path / "manifest.jsonl"
    path.write_text("{}\n\n", encoding="utf-8")
    with pytest.raises(ValueError, match="blank JSONL row"):
        contract.load_jsonl(path)
    path.write_text("[]\n", encoding="utf-8")
    with pytest.raises(ValueError, match="row must be an object"):
        contract.load_jsonl(path)


@pytest.mark.parametrize("fault", ["hash", "slice", "path", "functions", "schema"])
def test_manifest_rejects_identity_and_boundary_faults(
    tmp_path: Path, fault: str
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){}", encoding="utf-8")
    row = specimen(source)
    if fault == "hash":
        row["source_sha256"] = "0" * 64
    elif fault == "slice":
        row["slice"]["end_byte"] += 1
    elif fault == "path":
        row["source_path"] = "../case.c"
    elif fault == "functions":
        row["expected_functions"] = ["f", "f"]
    else:
        row["schema"] = 2
    with pytest.raises(ValueError):
        contract.validate_manifest([row], tmp_path)


def test_manifest_rejects_duplicate_ids_and_overlapping_slices(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){} int g(void){}", encoding="utf-8")
    first = specimen(source, "case/f")
    second = specimen(source, "case/g")
    with pytest.raises(ValueError, match="overlapping slices"):
        contract.validate_manifest([first, second], tmp_path)
    second["id"] = first["id"]
    with pytest.raises(ValueError, match="duplicate specimen id"):
        contract.validate_manifest([first, second], tmp_path)


def test_generated_and_mutated_rows_require_replay_metadata(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){}", encoding="utf-8")
    generated = specimen(source)
    generated["origin"] = {"kind": "generated"}
    with pytest.raises(ValueError, match="generation.generator"):
        contract.validate_manifest([generated], tmp_path)
    generated["origin"] = {
        "kind": "generated",
        "generator": "nest",
        "version": "1",
        "seed": 7,
    }
    contract.validate_manifest([generated], tmp_path)

    mutated = specimen(source)
    mutated["mutation"] = {"generator": "delete", "version": "1"}
    with pytest.raises(ValueError, match="generation.seed"):
        contract.validate_manifest([mutated], tmp_path)


def test_manifest_requires_explicit_nulls_and_captured_tool_versions(
    tmp_path: Path,
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){}", encoding="utf-8")
    missing = specimen(source)
    del missing["build_context"]
    with pytest.raises(ValueError, match="missing required fields"):
        contract.validate_manifest([missing], tmp_path)

    captured = specimen(source)
    captured["origin"] = {"kind": "captured", "tool": "decompiler"}
    with pytest.raises(ValueError, match="origin.version"):
        contract.validate_manifest([captured], tmp_path)


def test_build_context_is_content_identifying_not_just_arguments(
    tmp_path: Path,
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){}", encoding="utf-8")
    row = specimen(source)
    row["build_context"] = {
        "language_standard": "gnu11",
        "target_triple": "x86_64-unknown-linux-gnu",
        "compiler_arguments": ["-DFEATURE=1"],
        "working_directory": ".",
        "macro_definitions": {"FEATURE": "1"},
        "include_snapshot": "sha256:abc",
    }
    contract.validate_manifest([row], tmp_path)
    del row["build_context"]["include_snapshot"]
    with pytest.raises(ValueError, match="include_snapshot"):
        contract.validate_manifest([row], tmp_path)


def test_results_reject_missing_duplicate_unknown_and_mixed_rows(
    tmp_path: Path,
) -> None:
    first_source = tmp_path / "first.c"
    second_source = tmp_path / "second.c"
    first_source.write_text("int f(void){}", encoding="utf-8")
    second_source.write_text("int g(void){}", encoding="utf-8")
    first = specimen(first_source, "case/f")
    second = specimen(second_source, "case/g")
    rows = [first, second]
    manifest_hash = contract.manifest_sha256(rows)
    first_result = result(first)
    first_result["manifest_sha256"] = manifest_hash
    with pytest.raises(ValueError, match="missing result specimens"):
        contract.validate_results([first_result], rows)
    with pytest.raises(ValueError, match="duplicate result specimens"):
        contract.validate_results([first_result, deepcopy(first_result)], rows)

    second_result = result(second)
    second_result["manifest_sha256"] = manifest_hash
    second_result["specimen_id"] = "not/in/manifest"
    with pytest.raises(ValueError, match="not in the manifest"):
        contract.validate_results([first_result, second_result], rows)
    second_result["specimen_id"] = second["id"]
    second_result["run_id"] = "run-2"
    with pytest.raises(ValueError, match="one run/tool identity"):
        contract.validate_results([first_result, second_result], rows)

    second_result["run_id"] = "run-1"
    second_result["execution"]["timeout_ns"] = 2_000_000_000
    with pytest.raises(ValueError, match="one execution identity"):
        contract.validate_results([first_result, second_result], rows)


def test_results_never_confuse_success_and_failure_payloads(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){}", encoding="utf-8")
    row = specimen(source)
    success = result(row)
    success["failure"] = {"kind": "exception", "message": "hidden"}
    with pytest.raises(ValueError, match="must be null for success"):
        contract.validate_results([success], [row])
    failure = result(row, "exception")
    failure["failure"] = None
    with pytest.raises(ValueError, match="failure must be an object"):
        contract.validate_results([failure], [row])

    omitted = result(row)
    del omitted["diagnostics"]
    with pytest.raises(ValueError, match="missing required fields"):
        contract.validate_results([omitted], [row])


def test_results_preserve_duplicate_recovered_function_names(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){} int f(void){}", encoding="utf-8")
    row = specimen(source)
    measured = result(row)
    measured["yield"]["functions"] = ["f", "f"]
    contract.validate_results([measured], [row])

    measured["yield"]["functions"] = [""]
    contract.validate_results([measured], [row])


def test_cli_reports_the_validated_manifest_identity(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void){}", encoding="utf-8")
    row = specimen(source)
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(json.dumps(row) + "\n", encoding="utf-8")
    monkeypatch.setattr(
        "sys.argv",
        [
            "robustness_contract.py",
            "manifest",
            str(manifest),
            "--source-root",
            str(tmp_path),
        ],
    )
    contract.main()
    summary = json.loads(capsys.readouterr().out)
    assert summary == {
        "schema": 1,
        "manifest_sha256": contract.manifest_sha256([row]),
        "specimens": 1,
    }
