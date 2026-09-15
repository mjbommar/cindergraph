"""Comparison cannot silently discard identities or double-count columns."""

import cindergraph as cg
import pytest


@pytest.mark.parametrize("side", ["before", "after"])
@pytest.mark.parametrize("reverse", [False, True])
def test_compare_rejects_duplicate_function_names(side: str, reverse: bool) -> None:
    bodies = ["int f(int x){return x;}", "int f(int x){if(x)return 1;return 0;}"]
    duplicate = cg.analyze("".join(reversed(bodies) if reverse else bodies))
    unique = cg.analyze(bodies[0])
    before, after = (duplicate, unique) if side == "before" else (unique, duplicate)
    with pytest.raises(ValueError, match=f"{side}.*duplicate.*f"):
        cg.compare(before, after)


def test_compare_rejects_duplicate_metric_columns() -> None:
    report = cg.analyze("int f(int x){return x;}")
    with pytest.raises(ValueError, match="duplicate.*cyclomatic"):
        cg.compare(report, report, metrics=("cyclomatic", "cyclomatic"))


def test_empty_metric_selection_preserves_matching() -> None:
    report = cg.analyze("int f(int x){return x;}")
    result = cg.compare(report, report, metrics=())
    assert result["totals"] == {}
    assert [item["name"] for item in result["matched"]] == ["f"]
