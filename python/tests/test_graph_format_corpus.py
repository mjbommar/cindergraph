"""Independent JSON/XML decoding must preserve the same bundled graph data."""

from collections import Counter
import json
from pathlib import Path
import xml.etree.ElementTree as ET

import cindergraph as cg
import pytest


ROOT = Path(__file__).resolve().parents[2] / "crates/cindergraph/tests"
FILES = sorted((ROOT / "decompiler_fixtures/src").glob("*.c")) + sorted(
    (ROOT / "decbench_corpus/src").glob("*.c")
)
NS = {"g": "http://graphml.graphdrawing.org/xmlns"}


def test_graph_comparison_corpus_is_present() -> None:
    assert len(FILES) == 210


@pytest.mark.parametrize("path", FILES, ids=lambda path: path.name)
@pytest.mark.parametrize("representation", ["ast", "cfg", "ddg", "cdg", "pdg"])
def test_json_graphml_agree(path: Path, representation: str) -> None:
    code = path.read_text(encoding="utf-8")
    json_graphs = cg.export_graphs(code, repr=representation, format="json")
    xml_graphs = cg.export_graphs(code, repr=representation, format="graphml")
    assert json_graphs
    assert len(json_graphs) == len(xml_graphs)
    for (name, body), (xml_name, xml) in zip(json_graphs, xml_graphs):
        assert name == xml_name
        expected = json.loads(body)
        root = ET.fromstring(xml)
        keys = {
            key.attrib["id"]: key.attrib["attr.name"]
            for key in root.findall("g:key", NS)
        }
        graph = root.find("g:graph", NS)
        assert graph is not None
        assert graph.attrib["id"] == expected["graph"]["name"]
        assert graph.attrib["edgedefault"] == "directed"

        def attributes(element: ET.Element) -> dict[str, str]:
            return {
                keys[item.attrib["key"]]: item.text or ""
                for item in element.findall("g:data", NS)
            }

        nodes = graph.findall("g:node", NS)
        assert len(nodes) == len(expected["nodes"])
        actual_nodes = {
            int(node.attrib["id"].removeprefix("n")): attributes(node) for node in nodes
        }
        assert len(actual_nodes) == len(nodes)
        assert actual_nodes == {
            node["id"]: {key: value for key, value in node.items() if key != "id"}
            for node in expected["nodes"]
        }
        actual_edges = []
        for edge in graph.findall("g:edge", NS):
            source = int(edge.attrib["source"].removeprefix("n"))
            target = int(edge.attrib["target"].removeprefix("n"))
            assert source in actual_nodes and target in actual_nodes
            actual_edges.append(dict(source=source, target=target, **attributes(edge)))
        # Compare multisets, not sets: duplicate dependence edges matter.
        assert Counter(
            json.dumps(edge, sort_keys=True) for edge in actual_edges
        ) == Counter(json.dumps(edge, sort_keys=True) for edge in expected["edges"])
