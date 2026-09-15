"""The Tree-sitter robustness adapter validates its native worker protocol."""

from importlib.util import module_from_spec, spec_from_file_location
import json
from pathlib import Path
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "run_robustness_tree_sitter",
    ROOT / "tools/run_robustness_tree_sitter.py",
)
assert SPEC is not None and SPEC.loader is not None
adapter = module_from_spec(SPEC)
SPEC.loader.exec_module(adapter)


def _worker_document() -> dict:
    return {
        "schema": 1,
        "tree_sitter_version": "0.27.0",
        "grammar_version": "0.24.2",
        "language_abi": 15,
        "parse_ns": 100,
        "has_error": True,
        "error_nodes": 1,
        "missing_nodes": 2,
        "root_end_byte": 14,
        "functions": [{"name": "f", "start": 0, "end": 14}],
        "nodes": [
            {"id": 0, "kind": "translation_unit", "start": 0, "end": 14},
            {"id": 1, "kind": "function_definition", "start": 0, "end": 14},
        ],
        "edges": [{"source": 0, "target": 1}],
    }


def test_tree_sitter_partial_tree_is_a_qualified_success(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void) {}", encoding="utf-8")
    row = {
        "source_path": source.name,
        "slice": {"start_byte": 0, "end_byte": source.stat().st_size},
        "id": "broken/f",
    }
    completed = subprocess.CompletedProcess(
        [], 0, json.dumps(_worker_document()).encode(), b""
    )
    monkeypatch.setattr(adapter.subprocess, "run", lambda *args, **kwargs: completed)

    measured = adapter.analyze(
        row,
        {"specimen_id": row["id"]},
        tmp_path / "worker",
        tmp_path,
        tmp_path / "artifacts",
        1.0,
    )

    assert measured["status"] == "success"
    assert measured["yield"]["functions"] == ["f"]
    assert measured["diagnostics"] == {
        "errors": 1,
        "warnings": None,
        "recovery_nodes": 3,
    }
    assert measured["claims"] == {"syntax_complete": False, "cfg_complete": None}


def test_tree_sitter_worker_output_must_be_a_closed_graph() -> None:
    document = _worker_document()
    document["edges"][0]["target"] = 99
    with pytest.raises(ValueError, match="open edge"):
        adapter._validate_worker(document)


def test_tree_sitter_timeout_is_an_explicit_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void) {}", encoding="utf-8")
    row = {
        "source_path": source.name,
        "slice": {"start_byte": 0, "end_byte": source.stat().st_size},
        "id": "broken/f",
    }

    def timeout(*args: object, **kwargs: object) -> None:
        raise subprocess.TimeoutExpired("worker", 0.01)

    monkeypatch.setattr(adapter.subprocess, "run", timeout)
    measured = adapter.analyze(
        row,
        {"specimen_id": row["id"]},
        tmp_path / "worker",
        tmp_path,
        tmp_path / "artifacts",
        0.01,
    )
    assert measured["status"] == "timeout"
    assert measured["claims"] == {"syntax_complete": None, "cfg_complete": None}
