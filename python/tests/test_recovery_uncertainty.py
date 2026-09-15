"""Parser recovery must survive the dataflow-to-summary boundary."""

import cindergraph as cg
import pytest


@pytest.mark.parametrize("body", ["{return 0;", "{return @;}", "{return 0}"])
def test_recovery_prevents_definite_absence(body: str) -> None:
    code = "int f(int x)" + body
    assert cg.analyze(code).diagnostics
    assert not cg.call_summaries(code)[0]["complete"]
    assert not cg.data_flow(code)[0]["recovery_free"]
    issues = cg.data_flow(code)[0]["semantic_issues"]
    recovery = [issue for issue in issues if issue["kind"] == "recovered_syntax"]
    assert recovery
    assert all(
        issue["start"] is not None and issue["end"] is not None for issue in recovery
    )
    assert cg.reaches(code, "f", 0, "sink") == "unknown"


def test_recovery_uncertainty_propagates_to_caller() -> None:
    code = "int f(int x){return 0} int caller(int y){return f(y);}"
    summaries = cg.call_summaries(code)
    assert len(summaries) == 2
    assert all(not summary["complete"] for summary in summaries)
    assert cg.reaches(code, "caller", 0, "sink") == "unknown"


def test_clean_translation_unit_retains_definite_absence() -> None:
    code = "int f(int x){return 0;}"
    assert cg.data_flow(code)[0]["recovery_free"]
    assert cg.call_summaries(code)[0]["complete"]
    assert cg.reaches(code, "f", 0, "sink") == "no"


@pytest.mark.parametrize("index", [1, 10, 2**32 - 1])
def test_recovered_signature_does_not_prove_index_absent(index: int) -> None:
    assert cg.reaches("int f(int x){return 0;", "f", index, "sink") == "unknown"
    assert cg.reaches("int f(int x){return 0;}", "f", index, "sink") == "no"


def test_duplicate_signatures_do_not_make_query_order_dependent() -> None:
    short = "int f(int x){return 0;}"
    long = "int f(int x,int y){return y;}"
    for code in (short + long, long + short):
        assert cg.reaches(code, "f", 1, "sink") == "unknown"
