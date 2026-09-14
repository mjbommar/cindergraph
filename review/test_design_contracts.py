"""Independent review probes. Run explicitly; failures are review findings.

    uv run pytest review/test_design_contracts.py -q

These assert intended contracts, not the observed defective behavior. They are
outside the default testpaths so the review does not change the release gate.
"""

import itertools
import json
import random
from pathlib import Path

import pytest

import cindergraph as cg


def summary(code, name="f"):
    return next(s for s in cg.call_summaries(code) if s["name"] == name)


def returns_parameter(code, parameter=0):
    return any(
        f["parameter"] == parameter and f["sink"] == "return"
        for f in summary(code)["flows"]
    )


def test_unrelated_functions_have_no_interprocedural_path():
    code = "int f(int x){return x;} int g(int y){return y;}"
    assert cg.reaches(code, "f", 0, "g") == "no"


def test_direct_argument_reaches_callee_even_if_callee_returns_constant():
    code = "int g(int y){return 0;} int f(int x){g(x);return 0;}"
    assert cg.reaches(code, "f", 0, "g") == "yes"


def test_overwritten_parameter_does_not_reach_return():
    assert not returns_parameter("int f(int x){x=0;return x;}")


def test_read_in_empty_branch_is_not_return_flow():
    assert not returns_parameter("int f(int x){if(x){} return 0;}")


def test_unrelated_call_argument_does_not_hide_later_return_use():
    code = "int g(int y){return 0;} int f(int x){g(x);return x;}"
    assert returns_parameter(code)


def test_incomplete_callee_makes_caller_incomplete():
    code = "int g(int y){return external(y);} int f(int x){return g(x);}"
    assert summary(code)["complete"] is False


def test_duplicate_definitions_cannot_produce_a_complete_merged_summary():
    code = "int f(int x){return x;} int f(int x,int y){return y;}"
    assert summary(code)["complete"] is False


def test_slice_rejects_a_node_outside_the_selected_function():
    with pytest.raises((ValueError, IndexError)):
        cg.backward_slice("int f(void){return 1;}", "f", 999)


def test_pointer_store_is_present_in_slice_of_loaded_value():
    code = "int f(int x){int y=0;int *p=&y;*p=x;return y;}"
    nodes = cg.control_flow_graphs(code)[0]["cfg"]["nodes"]
    raw = code.encode()
    store = next(n["id"] for n in nodes if b"*p=x" in raw[n["start"] : n["end"]])
    ret = next(n["id"] for n in nodes if n["kind"] == "return")
    assert store in cg.backward_slice(code, "f", ret)


def test_binding_indices_can_be_joined_as_documented():
    flow = cg.data_flow("int a,b; int f(void){a=1;return b;}")[0]
    for item in flow["definitions"] + flow["uses"]:
        assert 0 <= item["binding"] < len(flow["bindings"])


def test_export_path_and_analyze_path_preserve_the_same_source_bytes(tmp_path):
    path = tmp_path / "crlf.c"
    path.write_bytes(b"// heading\r\nint f(void){\r\nreturn 1;\r\n}\r\n")
    report = cg.analyze_path(path)
    assert cg.export_path(path, format="json") == cg.export_graphs(
        report.source, format="json"
    )


@pytest.mark.parametrize(
    "assignments", list(itertools.product(("x=0;", "x=1;", "x=x+1;"), repeat=3))
)
def test_scalar_reaching_definitions_match_last_write_oracle(assignments):
    code = "int f(int x){" + "".join(assignments) + "return x;}"
    flow = cg.data_flow(code)[0]
    return_use = max(range(len(flow["uses"])), key=lambda i: flow["uses"][i]["start"])
    reaching = [e["definition"] for e in flow["edges"] if e["use"] == return_use]
    last_write = max(
        range(len(flow["definitions"])), key=lambda i: flow["definitions"][i]["start"]
    )
    assert reaching == [last_write]


@pytest.mark.parametrize(
    "code",
    [
        "",
        "not C",
        "int f( { ???",
        "// é\nint f(int x){return x+1;}",
        "int f(int x){while(x){if(x>2)break;--x;}return x;}",
        "int f(int x){return x;} int f(int y){return y+1;}",
    ],
)
def test_graph_endpoints_spans_and_function_order_across_apis(code):
    report = cg.analyze(code)
    names = [f.name for f in report.functions]
    assert [f["name"] for f in cg.functions(code)] == names
    assert [f["name"] for f in cg.data_flow(code)] == names
    assert [f["name"] for f in cg.control_flow_graphs(code)] == names
    for representation in cg.EXPORT_REPRS:
        graphs = cg.export_graphs(code, repr=representation, format="json")
        assert graphs == cg.export_graphs(code, repr=representation, format="json")
        assert [name for name, _ in graphs] == names
        for _, text in graphs:
            graph = json.loads(text)
            ids = {n["id"] for n in graph["nodes"]}
            assert len(ids) == len(graph["nodes"])
            for edge in graph["edges"]:
                assert edge["source"] in ids and edge["target"] in ids
            for node in graph["nodes"]:
                if "span" in node:
                    lo, hi = map(int, node["span"].split(":"))
                    assert 0 <= lo <= hi <= len(code.encode())


def test_documented_reference_pages_are_shipped():
    root = Path(__file__).resolve().parents[1]
    for path in ("docs/reference/source-metrics.md", "docs/reference/source-python.md"):
        assert (root / path).is_file(), path


def test_seeded_recovery_mutations_preserve_graph_integrity():
    rng = random.Random(20260914)
    original = "int f(int x){int y=x+1;while(y){if(y==3)break;y--;}return y;}"
    for _ in range(100):
        lo = rng.randrange(len(original))
        hi = rng.randrange(lo, len(original) + 1)
        code = original[:lo] + rng.choice(("", "?", "/*", "é", "}")) + original[hi:]
        test_graph_endpoints_spans_and_function_order_across_apis(code)
