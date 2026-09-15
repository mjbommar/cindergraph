"""The Clang robustness adapter preserves partial recovery and failures."""

import hashlib
from importlib.util import module_from_spec, spec_from_file_location
import json
from pathlib import Path
import shutil
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[2]
CLANG = shutil.which("clang")
pytestmark = pytest.mark.skipif(CLANG is None, reason="clang is not installed")
SPEC = spec_from_file_location(
    "run_robustness_clang", ROOT / "tools/run_robustness_clang.py"
)
assert SPEC is not None and SPEC.loader is not None
adapter = module_from_spec(SPEC)
SPEC.loader.exec_module(adapter)


def _row(source: Path, specimen_id: str = "clean/f") -> dict:
    return {
        "schema": 1,
        "id": specimen_id,
        "family": specimen_id.split("/", 1)[0],
        "source_path": source.name,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "slice": {"start_byte": 0, "end_byte": source.stat().st_size},
        "origin": {"kind": "repository", "tool": "test", "version": "1"},
        "dialect": "ordinary",
        "build_context": None,
        "expected_functions": ["f"],
        "oracle": {"kind": "fixture", "ref": "f"},
        "mutation": None,
    }


def test_clang_run_emits_valid_normalized_ast(tmp_path: Path) -> None:
    assert CLANG is not None
    source = tmp_path / "clean.c"
    source.write_text("int f(int x) { return x + 1; }", encoding="utf-8")
    row = _row(source)
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(json.dumps(row) + "\n", encoding="utf-8")
    output = tmp_path / "results.jsonl"
    artifacts = tmp_path / "artifacts"

    results = adapter.run(
        manifest, output, artifacts, tmp_path, "clang-test", Path(CLANG)
    )

    measured = results[0]
    assert measured["status"] == "success"
    assert measured["native_exit_code"] == 0
    assert measured["yield"]["functions"] == ["f"]
    assert measured["claims"] == {"syntax_complete": True, "cfg_complete": None}
    graphs = json.loads(
        (artifacts / measured["artifacts"]["normalized_ast"]).read_text()
    )
    assert graphs[0]["name"] == "f"
    node_ids = {node["id"] for node in graphs[0]["nodes"]}
    assert all(
        edge["source"] in node_ids and edge["target"] in node_ids
        for edge in graphs[0]["edges"]
    )


def test_clang_error_exit_retains_partial_ast(tmp_path: Path) -> None:
    assert CLANG is not None
    source = tmp_path / "broken.c"
    source.write_text("int f(void) { return 1;", encoding="utf-8")
    row = _row(source, "broken/f")
    base = {"specimen_id": row["id"]}

    measured = adapter.analyze(
        row, base, Path(CLANG), tmp_path, tmp_path / "artifacts", 5.0
    )

    assert measured["status"] == "success"
    assert measured["native_exit_code"] == 1
    assert measured["diagnostics"]["errors"] == 1
    assert measured["yield"]["functions"] == ["f"]
    assert measured["claims"] == {"syntax_complete": False, "cfg_complete": None}


def test_clang_timeout_is_an_explicit_failure(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    assert CLANG is not None
    source = tmp_path / "case.c"
    source.write_text("int f(void) {}", encoding="utf-8")
    row = _row(source)

    def timeout(*args: object, **kwargs: object) -> None:
        raise subprocess.TimeoutExpired("clang", 0.01)

    monkeypatch.setattr(adapter.subprocess, "run", timeout)
    measured = adapter.analyze(
        row,
        {"specimen_id": row["id"]},
        Path(CLANG),
        tmp_path,
        tmp_path / "artifacts",
        0.01,
    )
    assert measured["status"] == "timeout"
    assert measured["claims"] == {"syntax_complete": None, "cfg_complete": None}
