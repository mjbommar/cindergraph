"""Addressing a by-value parameter does not create a caller-visible output."""

from pathlib import Path

import cindergraph as cg
import pytest


@pytest.mark.parametrize(
    "code",
    [
        "int f(int x){int *p=&x;return x;}",
        "void f(int x){int *p=&x;*p=0;}",
        "void f(int *p){int **q=&p;*q=0;}",
        "void external(int *);void f(int x){external(&x);}",
    ],
)
def test_local_parameter_address_is_not_an_output(code: str) -> None:
    summary = cg.call_summaries(code)[0]
    assert not any(flow["sink"] == "parameter" for flow in summary["flows"])


def test_caller_pointee_write_is_a_direct_memory_effect() -> None:
    summary = cg.call_summaries("void f(int *out,int x){*out=x;}")[0]
    assert summary["complete"]
    assert summary["memory_effects_complete"]
    assert summary["memory_effects"] == [
        {"parameter": 0, "path": [], "kind": "write", "precision": "may_alias"}
    ]


def test_compiled_parameter_address_fixture_has_no_output_claims() -> None:
    path = Path(__file__).resolve().parents[2] / "tests/fixtures/parameter_address.c"
    summaries = cg.call_summaries(path.read_text(encoding="utf-8"))
    assert {summary["name"] for summary in summaries} == {
        "main",
        "replace_scalar",
        "replace_pointer",
    }
    assert not any(
        flow["sink"] == "parameter"
        for summary in summaries
        for flow in summary["flows"]
    )
