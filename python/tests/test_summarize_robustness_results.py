"""Robustness summaries retain failures and unavailable denominators."""

import hashlib
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SPEC = spec_from_file_location(
    "summarize_robustness_results",
    ROOT / "tools/summarize_robustness_results.py",
)
assert SPEC is not None and SPEC.loader is not None
summarizer = module_from_spec(SPEC)
SPEC.loader.exec_module(summarizer)


def test_summary_keeps_fixed_family_denominators(tmp_path: Path) -> None:
    source = tmp_path / "case.c"
    source.write_text("int f(void) {}", encoding="utf-8")
    manifest = [
        {
            "schema": 1,
            "id": "broken/f",
            "family": "broken",
            "source_path": source.name,
            "source_sha256": hashlib.sha256(source.read_bytes()).hexdigest(),
            "slice": {"start_byte": 0, "end_byte": source.stat().st_size},
            "origin": {"kind": "derived", "tool": "test", "version": "1"},
            "dialect": "ordinary",
            "build_context": None,
            "expected_functions": ["f"],
            "oracle": {"kind": "totality", "ref": "test"},
            "mutation": {
                "operator": "missing_semicolon",
                "severity": 2,
                "damage_scope": "target_local",
            },
        }
    ]
    contract = summarizer._module(
        ROOT / "tools/robustness_contract.py", "test_robustness_contract"
    )
    results = [
        {
            "schema": 1,
            "run_id": "run",
            "manifest_sha256": contract.manifest_sha256(manifest),
            "tool": {"name": "tool", "version": "1", "adapter": "a"},
            "execution": {
                "host_id": "host",
                "platform": "platform",
                "python": "3",
                "command": ["tool"],
                "timeout_ns": 10,
                "memory_limit_bytes": None,
                "source_revision": "revision",
                "source_dirty": True,
            },
            "specimen_id": "broken/f",
            "status": "timeout",
            "failure": {"kind": "Timeout", "message": "limit"},
            "timing_ns": {"startup": 10, "analysis": None, "serialization": None},
            "peak_rss_bytes": None,
            "diagnostics": {"errors": None, "warnings": None, "recovery_nodes": None},
            "yield": {"functions": [], "covered_source_bytes": None},
            "claims": {"syntax_complete": None, "cfg_complete": None},
            "artifacts": {"normalized_ast": None, "normalized_cfg": None},
        }
    ]

    summary = summarizer.summarize(manifest, [results])

    broken = summary["runs"][0]["families"]["broken"]
    assert broken["specimens"] == 1
    assert broken["statuses"] == {"timeout": 1}
    assert broken["syntax_complete"] == {
        "true": 0,
        "false": 0,
        "unavailable": 1,
    }
    assert broken["oracle_functions"] == {
        "found": 0,
        "total": 1,
        "coverage_available": True,
    }
    stratum = broken["mutation_strata"]["missing_semicolon"]
    assert stratum["damage_scope"] == "target_local"
    assert stratum["all"]["oracle_functions"] == broken["oracle_functions"]
    assert stratum["severities"]["2"]["specimens"] == 1
