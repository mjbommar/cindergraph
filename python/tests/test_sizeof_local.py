"""Known non-array locals do not contribute values to sizeof."""

from pathlib import Path

import cindergraph as cg
import pytest


@pytest.mark.parametrize("declaration", ["int x", "double x", "int *x"])
@pytest.mark.parametrize("expression", ["sizeof(x)", "sizeof x", "sizeof((x))"])
def test_sizeof_known_local_does_not_read_its_value(
    declaration: str, expression: str
) -> None:
    code = f"int f({declaration}){{return {expression};}}"
    assert not cg.data_flow(code)[0]["uses"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and not summary["flows"]


def test_normal_read_of_same_local_is_retained() -> None:
    summary = cg.call_summaries("int f(int x){return sizeof(x)+x;}")[0]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_compiler_checked_sizeof_fixture() -> None:
    fixture = Path(__file__).resolve().parents[2] / "tests/fixtures/sizeof_local.c"
    summaries = {s["name"]: s for s in cg.call_summaries(fixture.read_text())}
    for name in ("scalar", "pointer", "pointee", "pointee_chain", "nested"):
        assert summaries[name]["complete"], name
        assert not summaries[name]["flows"], name


@pytest.mark.parametrize(
    "operand", ["sink(x)", "sink(sink(x))", "x++", "++x", "x+1", "x?1:2", "(x=3)"]
)
def test_sizeof_scalar_expression_has_no_runtime_events(operand: str) -> None:
    code = f"int sink(int x){{return x;}} int f(int x){{return sizeof({operand});}}"
    flow = cg.data_flow(code)[1]
    assert not flow["uses"]
    assert all(d["kind"] == "parameter" for d in flow["definitions"])
    summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
    assert summary["complete"] and not summary["flows"]


def test_unevaluated_call_does_not_reach_sink() -> None:
    code = "int sink(int x){return x;} int f(int x){return sizeof(sink(x));}"
    assert cg.reaches(code, "f", 0, "sink") == "no"


def test_sizeof_assignment_does_not_overwrite_local() -> None:
    code = "int f(int x){int y=x;sizeof(y=0);return y;}"
    assert cg.call_summaries(code)[0]["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_sizeof_indirect_write_does_not_modify_pointee() -> None:
    code = "int f(int x){int y=0;int *p=&y;sizeof(*p=x);return y;}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert not any(d["kind"] == "memory_write" for d in flow["definitions"])
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and not summary["flows"]


def test_evaluated_call_outside_sizeof_still_reaches_sink() -> None:
    code = "int sink(int x){return x;} int f(int x){sizeof(sink(x));return sink(x);}"
    assert cg.reaches(code, "f", 0, "sink") == "yes"


@pytest.mark.parametrize("count", [1, 32, 128, 512])
def test_many_unevaluated_regions_preserve_final_read(count: int) -> None:
    code = "int f(int x){" + "sizeof(sizeof(x+1));" * count + "return x;}"
    flow = cg.data_flow(code)[0]
    assert len(flow["uses"]) == 1
    assert flow["uses"][0]["name"] == "x"
    assert cg.call_summaries(code)[0]["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


@pytest.mark.parametrize(
    "type_name", ["int", "double", "volatile unsigned long", "int *"]
)
def test_sizeof_known_pointee_does_not_read_memory(type_name: str) -> None:
    code = f"int f({type_name} *p){{return sizeof(*p);}}"
    flow = cg.data_flow(code)[0]
    assert not flow["uses"]
    assert flow["memory_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and not summary["flows"]


def test_sizeof_known_local_pointee_does_not_propagate_value() -> None:
    code = "int f(int x){int y=x;int *p=&y;return sizeof(*p);}"
    assert not cg.call_summaries(code)[0]["flows"]


@pytest.mark.parametrize("operand", ["*(p)", "*((p))", "**p", "*(*(p))", "*((*(p)))"])
def test_parenthesized_pointer_chain_size_is_unevaluated(operand: str) -> None:
    code = f"int f(int **p){{return sizeof({operand});}}"
    flow = cg.data_flow(code)[0]
    assert not flow["uses"] and flow["memory_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and not summary["flows"]


def test_sizeof_pointer_binding_respects_inner_shadow() -> None:
    code = "int f(int *p){int x=1;{int **p=0;sizeof(*(*(p)));}return *p;}"
    # The outer dereference remains evaluated. Its address reads p, while its
    # returned scalar comes from caller-owned memory rather than from p's value.
    assert [use["binding"] for use in cg.data_flow(code)[0]["uses"]] == [0]
    summary = cg.call_summaries(code)[0]
    assert summary["flows"] == []
    assert summary["memory_effects"] == [
        {"parameter": 0, "path": [], "kind": "read", "precision": "may_alias"}
    ]


@pytest.mark.parametrize("count", [32, 128, 512])
def test_many_pointer_sizeof_operands_have_no_value_flow(count: int) -> None:
    code = "int f(int **p){" + "sizeof(*(*(p)));" * count + "return 0;}"
    flow = cg.data_flow(code)[0]
    assert not flow["uses"] and flow["memory_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and not summary["flows"]
