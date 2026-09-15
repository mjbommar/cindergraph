"""Native graph views keep large topology out of Python dictionaries."""

import subprocess
import sys

import pytest

import cindergraph as cg


CODE = "int f(int x){if(x){return x;}return 0;}"


def test_native_cfg_preserves_export_topology_and_snapshot_identity() -> None:
    session = cg.AnalysisSession(CODE)
    graph = session.native_graphs(repr="control-flow")[0]
    serialized = session.control_flow_graphs()[0]

    assert isinstance(graph, cg.NativeGraph)
    assert graph.name == "f"
    assert graph.representation == "cfg"
    assert graph.graph_kind == "executable_cfg"
    assert graph.source_id == session.source_id == serialized["source_id"]
    assert graph.function_id == serialized["function_id"] == 0
    assert graph.analysis_revision == serialized["analysis_revision"]
    assert graph.input_dialect == "ordinary"
    assert graph.external_call_policy == "unknown"
    assert graph.directed
    assert graph.node_count == len(graph.nodes())
    assert graph.edge_count == len(graph.edges()) == len(graph.edge_list())
    assert graph.edge_list() == tuple(
        (edge["source"], edge["target"]) for edge in graph.edges()
    )
    assert cg.backward_slice(
        CODE,
        "f",
        graph.nodes()[-1]["id"],
        source_id=graph.source_id,
        function_id=graph.function_id,
        graph_kind=graph.graph_kind,
        analysis_revision=graph.analysis_revision,
    )


def test_native_adjacency_and_transitive_traversal_are_consistent() -> None:
    graph = cg.native_graphs(CODE)[0]
    node_ids = {node["id"] for node in graph.nodes()}

    for node in node_ids:
        assert graph.out_degree(node) == len(graph.successors(node))
        assert graph.in_degree(node) == len(graph.predecessors(node))
        assert set(graph.successors(node)) <= node_ids
        assert set(graph.predecessors(node)) <= node_ids
        assert node not in graph.descendants(node)
        assert node not in graph.ancestors(node)
    for source, target in graph.edge_list():
        assert target in graph.successors(source)
        assert source in graph.predecessors(target)
        assert target in graph.descendants(source)
        assert source in graph.ancestors(target)


def test_native_adjacency_preserves_parallel_edge_multiplicity() -> None:
    graph = cg.native_graphs("int f(int x){x=x+1;return x;}", repr="pdg")[0]
    expected_successors: dict[int, list[int]] = {}
    expected_predecessors: dict[int, list[int]] = {}
    for source, target in graph.edge_list():
        expected_successors.setdefault(source, []).append(target)
        expected_predecessors.setdefault(target, []).append(source)

    for node in (entry["id"] for entry in graph.nodes()):
        assert graph.successors(node) == tuple(expected_successors.get(node, ()))
        assert graph.predecessors(node) == tuple(expected_predecessors.get(node, ()))


def test_native_graph_rejects_unknown_nodes_and_representations() -> None:
    graph = cg.native_graphs(CODE)[0]
    for operation in (
        graph.successors,
        graph.predecessors,
        graph.in_degree,
        graph.out_degree,
        graph.ancestors,
        graph.descendants,
    ):
        with pytest.raises(KeyError):
            operation(4_294_967_295)
    with pytest.raises(ValueError, match="unknown repr"):
        cg.native_graphs(CODE, repr="wishful")


def test_every_graph_family_uses_the_same_native_contract() -> None:
    session = cg.AnalysisSession(CODE, external_calls="taint_return")
    for representation in ("ast", "cfg", "ddg", "cdg", "pdg"):
        graph = session.native_graphs(repr=representation)[0]
        assert graph.representation == representation
        assert graph.source_id == session.source_id
        assert graph.external_call_policy == "taint_return"
        assert graph.node_count >= 1


def test_networkx_conversion_is_an_explicit_optional_boundary() -> None:
    nx = pytest.importorskip("networkx")
    graph = cg.native_graphs(CODE)[0]
    converted = graph.to_networkx(multigraph=True)

    assert isinstance(converted, nx.MultiDiGraph)
    assert converted.graph["source_id"] == graph.source_id
    assert converted.number_of_nodes() == graph.node_count
    assert converted.number_of_edges() == graph.edge_count


def test_native_graph_use_does_not_import_networkx() -> None:
    script = """
import sys
import cindergraph as cg
assert 'networkx' not in sys.modules
graph = cg.native_graphs('int f(void){return 0;}')[0]
assert graph.node_count
assert 'networkx' not in sys.modules
"""
    subprocess.run([sys.executable, "-c", script], check=True)
