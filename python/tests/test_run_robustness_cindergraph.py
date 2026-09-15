"""The native robustness adapter emits complete, conservative evidence."""

import hashlib
from importlib.util import module_from_spec, spec_from_file_location
import json
from pathlib import Path
import subprocess

import pytest


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "run_robustness_cindergraph", ROOT / "tools/run_robustness_cindergraph.py"
)
assert SPEC is not None and SPEC.loader is not None
adapter = module_from_spec(SPEC)
SPEC.loader.exec_module(adapter)


def _row(source: Path) -> dict:
    return {
        "schema": 1,
        "id": "clean/f",
        "family": "fixture-clean",
        "source_path": source.name,
        "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
        "slice": {"start_byte": 0, "end_byte": source.stat().st_size},
        "origin": {"kind": "repository", "tool": "cindergraph", "version": "test"},
        "dialect": "ordinary",
        "build_context": None,
        "expected_functions": ["f"],
        "oracle": {"kind": "fixture", "ref": "f"},
        "mutation": None,
    }


def test_adapter_writes_contract_validated_graph_evidence(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(int x) { return x + 1; }", encoding="utf-8")
    row = _row(source)
    manifest = tmp_path / "manifest.jsonl"
    manifest.write_text(json.dumps(row) + "\n", encoding="utf-8")
    output = tmp_path / "results.jsonl"
    artifacts = tmp_path / "artifacts"

    results = adapter.run(manifest, output, artifacts, tmp_path, "test-run")

    assert len(results) == 1
    measured = results[0]
    assert measured["status"] == "success"
    assert measured["yield"]["functions"] == ["f"]
    assert measured["claims"] == {"syntax_complete": True, "cfg_complete": True}
    assert measured["timing_ns"]["startup"] > 0
    assert measured["peak_rss_bytes"] is None or measured["peak_rss_bytes"] > 0
    assert output.is_file()
    assert not output.with_suffix(".jsonl.partial").exists()
    analyzed = artifacts / measured["artifacts"]["analyzed_source"]
    native_diagnostics = artifacts / measured["artifacts"]["native_diagnostics"]
    assert analyzed.read_text(encoding="utf-8") == source.read_text(encoding="utf-8")
    assert json.loads(native_diagnostics.read_text(encoding="utf-8")) == []
    assert (artifacts / measured["artifacts"]["normalized_ast"]).is_file()
    assert (artifacts / measured["artifacts"]["normalized_cfg"]).is_file()

    with pytest.raises(ValueError, match="refusing to overwrite"):
        adapter.run(manifest, output, tmp_path / "other-artifacts", tmp_path, "again")

    partial_output = tmp_path / "partial-results.jsonl"
    partial_output.with_suffix(".jsonl.partial").write_text("incomplete\n")
    with pytest.raises(ValueError, match="partial result file"):
        adapter.run(
            manifest,
            partial_output,
            tmp_path / "partial-artifacts",
            tmp_path,
            "again",
        )


def test_supervisor_turns_timeout_and_signal_into_results(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void) {}", encoding="utf-8")
    row = _row(source)
    base = {
        "schema": 1,
        "run_id": "test-run",
        "manifest_sha256": "0" * 64,
        "tool": {"name": "cindergraph", "version": "test", "adapter": "test"},
        "specimen_id": row["id"],
    }

    def timeout(*args: object, **kwargs: object) -> None:
        raise subprocess.TimeoutExpired("worker", 0.01)

    monkeypatch.setattr(adapter.subprocess, "run", timeout)
    measured = adapter._run_isolated(
        row, base, tmp_path, tmp_path / "artifacts", 0.01, None
    )
    assert measured["status"] == "timeout"
    assert measured["claims"] == {"syntax_complete": None, "cfg_complete": None}

    monkeypatch.setattr(
        adapter.subprocess,
        "run",
        lambda *args, **kwargs: subprocess.CompletedProcess([], -9, "", ""),
    )
    measured = adapter._run_isolated(
        row, base, tmp_path, tmp_path / "artifacts", 1.0, None
    )
    assert measured["status"] == "signal"
    assert measured["failure"] == {
        "kind": "Signal9",
        "message": "worker terminated by signal 9",
    }


def test_worker_classifies_memory_exhaustion(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void) {}", encoding="utf-8")
    row = _row(source)
    base = {"specimen_id": row["id"]}

    class Exhausted:
        @staticmethod
        def AnalysisSession(*args: object, **kwargs: object) -> None:
            raise MemoryError("address-space limit reached")

    measured = adapter.analyze_specimen(
        row, tmp_path, tmp_path / "artifacts", base, Exhausted, 1
    )
    assert measured["status"] == "memory_limit"
    assert measured["failure"] == {
        "kind": "MemoryError",
        "message": "address-space limit reached",
    }
