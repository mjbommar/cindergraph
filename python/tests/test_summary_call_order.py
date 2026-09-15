"""Ordinary acyclic call chains must not depend on declaration order."""

import random

import cindergraph as cg
import pytest


def semantic_summaries(summaries: list[dict]) -> list[dict]:
    """Compare meaning independently of snapshot-local IDs."""
    return sorted(
        [
            {
                key: value
                for key, value in summary.items()
                if key not in {"source_id", "function_id"}
            }
            for summary in summaries
        ],
        key=lambda summary: summary["name"],
    )


@pytest.mark.parametrize("count", [25, 128])
@pytest.mark.parametrize("terminal", ["x", "0"])
def test_call_chain_order(count: int, terminal: str) -> None:
    prototypes = "".join(f"int f{i}(int);" for i in range(count))
    bodies = [f"int f{i}(int x){{return f{i + 1}(x);}}" for i in range(count - 1)] + [
        f"int f{count - 1}(int x){{return {terminal};}}"
    ]
    expected = None
    shuffled = bodies.copy()
    random.Random(701).shuffle(shuffled)
    for ordered in (bodies, bodies[::-1], shuffled):
        summaries = cg.call_summaries(prototypes + "".join(ordered))
        assert len(summaries) == count
        assert all(summary["complete"] for summary in summaries)
        assert all(bool(summary["flows"]) == (terminal == "x") for summary in summaries)
        if expected is None:
            expected = summaries
        else:
            assert [summary["source_id"] for summary in summaries] != [
                summary["source_id"] for summary in expected
            ]
        assert semantic_summaries(summaries) == semantic_summaries(expected)


def test_recursive_parameter_rotation_retains_work_bound() -> None:
    parameters = ",".join(f"int p{i}" for i in range(32))
    arguments = ",".join(f"p{i}" for i in list(range(1, 32)) + [0])
    code = f"int f({parameters}){{if(p0)return p0;return f({arguments});}}"
    summary = cg.call_summaries(code)[0]
    assert not summary["complete"]
    assert cg.reaches(code, "f", 0, "absent") == "unknown"


def test_parenthesized_defined_callee_propagates_without_false_uncertainty() -> None:
    code = "int id(int x){return x;} int f(int y){return ((id))(y);}"
    summaries = {summary["name"]: summary for summary in cg.call_summaries(code)}
    assert summaries["f"]["complete"]
    assert summaries["f"]["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


@pytest.mark.parametrize("callee", ["((id))", "(&id)", "(*id)", "(*&id)"])
def test_wrapped_prototyped_callee_retains_a_known_positive_path(callee: str) -> None:
    code = f"extern int id(int); int f(int y){{return {callee}(y);}}"
    summary = cg.call_summaries(code)[0]
    assert not summary["complete"]  # The external body remains unknown.
    assert cg.reaches(code, "f", 0, "id") == "yes"


def test_function_typedef_declaration_retains_a_known_positive_path() -> None:
    code = "typedef int Unary(int); extern Unary id; int f(int y){return (id)(y);}"
    assert cg.reaches(code, "f", 0, "id") == "yes"
