"""Comma evaluates both operands but takes its value only from the right."""

import cindergraph as cg
import pytest


@pytest.mark.parametrize(
    "body,flows",
    [
        ("return (x,0);", False),
        ("return (0,x);", True),
        ("int y;return (y=x,y=0);", False),
        ("int y;return (y=0,y=x);", True),
        ("int y;return (y=x,y);", True),
        ("int y=(x,0);return y;", False),
        ("int y=(0,x);return y;", True),
        ("int y=0;(y=x,y=0);return y;", False),
        ("int y=0;(y=0,y=x);return y;", True),
        ("if((x,0))return 1;return 0;", False),
        ("if((0,x))return 1;return 0;", True),
        ("int y=0;return (x?(y=1):(y=2),y);", True),
        ("int y=0;return (x&&(y=1),y);", True),
        ("int y=0;return (x||(y=1),y);", True),
        ("int y=0;return (x?(y=1):(y=2),0);", False),
    ],
)
def test_comma_value_and_effect_provenance(body: str, flows: bool) -> None:
    summary = cg.call_summaries("int f(int x){" + body + "}")[0]
    assert summary["complete"]
    assert bool(summary["flows"]) is flows


def test_discarded_call_result_does_not_discard_call_transfer() -> None:
    code = "int sink(int v){return v;} int f(int x){return (sink(x),0);}"
    summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
    assert summary["complete"] and not summary["flows"]
    assert cg.reaches(code, "f", 0, "sink") == "yes"


def test_discarded_argument_value_does_not_reach_callee() -> None:
    code = "int sink(int v){return v;} int f(int x){return sink((x,0));}"
    summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
    assert summary["complete"] and not summary["flows"]
    assert cg.reaches(code, "f", 0, "sink") == "no"
