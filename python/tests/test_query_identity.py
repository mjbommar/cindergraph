"""Name-based queries cannot select a source among duplicate definitions."""

import cindergraph as cg
import pytest
from pathlib import Path


@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize("parameter", [0, 1, 2])
def test_duplicate_source_self_query_is_unknown(reverse: bool, parameter: int) -> None:
    bodies = ["int f(void){return 0;}", "int f(int x,int y){return x+y;}"]
    code = "".join(reversed(bodies) if reverse else bodies)
    assert cg.reaches(code, "f", parameter, "f") == "unknown"


def test_unique_source_self_query_has_zero_length_path() -> None:
    assert cg.reaches("int f(int x){return 0;}", "f", 0, "f") == "yes"


def test_missing_source_is_not_a_zero_length_path() -> None:
    assert cg.reaches("int f(int x){return x;}", "missing", 0, "missing") == "unknown"


def test_duplicate_summary_arity_is_order_independent() -> None:
    bodies = ["int f(void){return 0;}", "int f(int x,int y){return x+y;}"]
    first = cg.call_summaries("".join(bodies))
    second = cg.call_summaries("".join(reversed(bodies)))
    assert first[0]["source_id"] != second[0]["source_id"]
    assert len(first) == len(second) == 2
    assert [item["function_id"] for item in first] == [0, 1]
    assert [item["function_id"] for item in second] == [0, 1]
    assert [item["parameters"] for item in first] == [0, 2]
    assert [item["parameters"] for item in second] == [2, 0]
    assert first[1]["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None},
        {"parameter": 1, "sink": "return", "sink_parameter": None},
    ]
    assert second[0]["flows"] == first[1]["flows"]


def test_unique_summary_and_dataflow_share_snapshot_identity() -> None:
    code = "int f(int x){return x;}"
    flow = cg.data_flow(code)[0]
    summary = cg.call_summaries(code)[0]
    assert summary["source_id"] == flow["source_id"]
    assert summary["function_id"] == flow["function_id"] == 0
    assert summary["analysis_revision"] == flow["analysis_revision"] == 5


def test_analysis_session_reuses_one_snapshot_and_returns_fresh_views() -> None:
    session = cg.AnalysisSession("int f(int x){return x;}")
    cfg = session.control_flow_graphs()
    flow = session.data_flow()
    summaries = session.call_summaries()

    assert cfg[0]["source_id"] == flow[0]["source_id"] == summaries[0]["source_id"]
    assert cfg[0]["source_id"] == session.source_id
    assert (
        cfg[0]["function_id"] == flow[0]["function_id"] == summaries[0]["function_id"]
    )

    flow[0]["name"] = "mutated"
    summaries.clear()
    assert session.data_flow()[0]["name"] == "f"
    assert session.call_summaries()[0]["name"] == "f"


def test_analysis_session_exports_and_slices_its_owned_graphs() -> None:
    code = "int f(int x){if(x)return x;return 0;}"
    session = cg.AnalysisSession(code)
    cfg = session.control_flow_graphs()[0]
    exit_node = cfg["cfg"]["exit"]

    assert session.backward_slice(
        "f", exit_node, function_id=cfg["function_id"]
    ) == cg.backward_slice(code, "f", exit_node)
    for representation in ("ast", "cfg", "ddg", "cdg", "pdg"):
        assert session.export_graphs(
            repr=representation, format="json"
        ) == cg.export_graphs(code, repr=representation, format="json")


def test_analysis_session_slice_checks_owned_function_identity() -> None:
    session = cg.AnalysisSession("int f(void){return 0;}")
    with pytest.raises(ValueError, match="function_id"):
        session.backward_slice("f", 0, function_id=99)


def test_typed_reachability_exposes_path_and_complete_negative() -> None:
    code = "int sink(int z){return z;} int f(int x){return sink(x);}"
    found = cg.query_reaches(code, "f", 0, "sink")
    assert found.claim == "found_may_path"
    assert found.verdict == "yes"
    assert found.path == (
        cg.ReachabilityStep(function_id=1, function="f", parameter=0),
        cg.ReachabilityStep(function_id=0, function="sink", parameter=0),
    )
    assert not found.coverage_complete

    absent = cg.AnalysisSession(code).query_reaches("f", 0, "other")
    assert absent.claim == "no_may_path"
    assert absent.verdict == "no"
    assert absent.coverage_complete
    assert not absent.uncertainty


def test_typed_reachability_explains_unknown_results() -> None:
    answer = cg.query_reaches("int f(int x){external(x);return 0;}", "f", 0, "sink")
    assert answer.claim == "unknown"
    assert answer.verdict == "unknown"
    assert not answer.coverage_complete
    assert set(answer.uncertainty) == {"incomplete_summary", "missing_callee"}
    assert answer.explored


def test_identity_first_reachability_distinguishes_duplicate_bodies() -> None:
    code = "int f(int x){return g(x);} int f(int x){return 0;} int g(int y){return y;}"
    session = cg.AnalysisSession(code)
    found = session.query_reaches_by_id(0, 0, 2)
    absent = session.query_reaches_by_id(1, 0, 2)
    assert found.claim == "found_may_path"
    assert [step.function_id for step in found.path] == [0, 2]
    assert absent.claim == "no_may_path"
    assert cg.query_reaches(code, "f", 0, "g").claim == "unknown"
    assert cg.query_reaches_by_id(code, 0, 0, 2) == found


def test_analysis_session_dialect_owns_normalized_source_and_metadata() -> None:
    raw = "int f(int x @ eax){return x;}"
    ordinary = cg.AnalysisSession(raw)
    session = cg.AnalysisSession(raw, dialect="decompiled")

    assert session.dialect == "decompiled"
    assert "@ eax" not in session.source
    assert session.source_id != ordinary.source_id
    assert isinstance(session.diagnostics, tuple)
    products = (
        session.control_flow_graphs()[0],
        session.data_flow()[0],
        session.call_summaries()[0],
    )
    assert all(item["source_id"] == session.source_id for item in products)
    assert all(item["input_dialect"] == "decompiled" for item in products)
    assert session.query_reaches("f", 0, "f").input_dialect == "decompiled"


def test_analysis_session_rejects_unknown_dialect() -> None:
    with pytest.raises(ValueError, match="unknown dialect"):
        getattr(cg, "AnalysisSession")("int f(void){return 0;}", dialect="imaginary")


def test_analysis_session_external_call_policy_is_visible_and_transitive() -> None:
    code = (
        "extern int external(int);"
        "int f(int x){return external(x);}"
        "int g(int y){return f(y);}"
    )
    unknown = cg.AnalysisSession(code)
    tainted = cg.AnalysisSession(code, external_calls="taint_return")
    pure = cg.AnalysisSession(code, external_calls="assume_pure_no_flow")

    assert unknown.external_calls == "unknown"
    assert tainted.external_calls == "taint_return"
    assert pure.external_calls == "assume_pure_no_flow"
    assert all(
        not item["flows"] and not item["complete"] for item in unknown.call_summaries()
    )
    assert all(
        item["flows"] == [{"parameter": 0, "sink": "return", "sink_parameter": None}]
        and not item["complete"]
        for item in tainted.call_summaries()
    )
    assert all(not item["flows"] and item["complete"] for item in pure.call_summaries())
    products = (
        pure.control_flow_graphs()[0],
        pure.data_flow()[0],
        pure.call_summaries()[0],
    )
    assert all(
        item["external_call_policy"] == "assume_pure_no_flow" for item in products
    )
    answer = pure.query_reaches("g", 0, "absent")
    assert answer.claim == "no_may_path"
    assert answer.coverage_complete
    assert answer.external_call_policy == "assume_pure_no_flow"


def test_analysis_session_rejects_unknown_external_call_policy() -> None:
    with pytest.raises(ValueError, match="unknown external-call policy"):
        getattr(cg, "AnalysisSession")(
            "int f(void){return 0;}", external_calls="optimistic"
        )


@pytest.mark.parametrize("reverse", [False, True])
@pytest.mark.parametrize("node", [0, 2, 4294967295])
def test_slice_rejects_duplicate_function_identity(reverse: bool, node: int) -> None:
    bodies = ["int f(int x){return x;}", "int f(int x){if(x)return 1;return 0;}"]
    code = "".join(reversed(bodies) if reverse else bodies)
    with pytest.raises(ValueError, match="ambiguous.*f"):
        cg.backward_slice(code, "f", node)


def test_unrelated_duplicate_does_not_prevent_unique_slice() -> None:
    code = "int f(int x){return x;} int g(void){return 0;} int g(void){return 1;}"
    assert cg.backward_slice(code, "f", 0) == [0]


def test_missing_slice_function_remains_key_error() -> None:
    with pytest.raises(KeyError):
        cg.backward_slice("int f(int x){return x;}", "missing", 0)


def test_graph_identity_is_shared_and_checked_at_slice_boundary() -> None:
    code = "int f(int x){if(x)return x;return 0;}"
    cfg = cg.control_flow_graphs(code)[0]
    control = cg.control_dependence(code)[0]
    flow = cg.data_flow(code)[0]
    identity = {
        "source_id": cfg["source_id"],
        "function_id": cfg["function_id"],
        "graph_kind": cfg["graph_kind"],
        "analysis_revision": cfg["analysis_revision"],
    }
    assert identity == {
        "source_id": control["source_id"],
        "function_id": control["function_id"],
        "graph_kind": control["graph_kind"],
        "analysis_revision": control["analysis_revision"],
    }
    assert flow["source_id"] == identity["source_id"]
    assert flow["function_id"] == identity["function_id"]
    assert flow["analysis_revision"] == identity["analysis_revision"]
    assert cg.backward_slice(code, "f", cfg["cfg"]["exit"], **identity)

    bad_coordinates = (
        (
            "different",
            identity["function_id"],
            identity["graph_kind"],
            identity["analysis_revision"],
        ),
        (
            identity["source_id"],
            99,
            identity["graph_kind"],
            identity["analysis_revision"],
        ),
        (
            identity["source_id"],
            identity["function_id"],
            "parity_cfg",
            identity["analysis_revision"],
        ),
        (identity["source_id"], identity["function_id"], identity["graph_kind"], 99),
    )
    for source_id, function_id, graph_kind, analysis_revision in bad_coordinates:
        with pytest.raises(ValueError):
            cg.backward_slice(
                code,
                "f",
                cfg["cfg"]["exit"],
                source_id=source_id,
                function_id=function_id,
                graph_kind=graph_kind,
                analysis_revision=analysis_revision,
            )


@pytest.mark.parametrize("reverse", [False, True])
def test_callgraph_rejects_duplicate_definition_merging(
    tmp_path: Path, reverse: bool
) -> None:
    bodies = ["int f(void){return left();}", "int f(void){return right();}"]
    path = tmp_path / "duplicate.c"
    path.write_text("".join(reversed(bodies) if reverse else bodies), encoding="utf-8")
    with pytest.raises(ValueError, match="duplicate.*f"):
        cg.parse_callgraph(path)


def test_callgraph_prototypes_do_not_create_duplicate_definitions(
    tmp_path: Path,
) -> None:
    path = tmp_path / "prototype.c"
    path.write_text(
        "int f(int); int f(int x){return x?f(x-1):external();}", encoding="utf-8"
    )
    graph = cg.parse_callgraph(path)
    assert set(graph.edges) == {("f", "f"), ("f", "external")}


@pytest.mark.parametrize("reverse", [False, True])
def test_report_call_graph_does_not_discard_duplicate_body(reverse: bool) -> None:
    bodies = ["int f(void){return left();}", "int f(void){return right();}"]
    report = cg.analyze("".join(reversed(bodies) if reverse else bodies))
    with pytest.raises(ValueError, match="duplicate.*f"):
        report.call_graph()
    assert len(report.functions) == 2
    assert report.defined_names() == frozenset({"f"})
