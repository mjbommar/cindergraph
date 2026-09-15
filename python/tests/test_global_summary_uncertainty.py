"""Unmodelled cross-function global effects cannot certify independence."""

import cindergraph as cg
import pytest


@pytest.mark.parametrize("write", ["g=x;", "g+=x;", "g++;", "g=0;"])
def test_global_effects_make_summaries_incomplete(write: str) -> None:
    code = f"int g; void put(int x){{{write}}} int f(int x){{put(x);return g;}}"
    assert not cg.analyze(code).diagnostics
    summaries = cg.call_summaries(code)
    assert {summary["name"] for summary in summaries} == {"put", "f"}
    assert all(not summary["complete"] for summary in summaries)
    assert cg.reaches(code, "f", 0, "absent") == "unknown"


def test_global_read_is_not_a_complete_local_summary() -> None:
    assert not cg.call_summaries("int g; int f(int x){return g;}")[0]["complete"]


def test_local_shadow_retains_complete_summary() -> None:
    code = "int g; int f(int x){int g=x;return g;}"
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]
