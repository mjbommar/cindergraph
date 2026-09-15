"""GraphML must remain XML 1.0 even for tolerant-parser input."""

import xml.etree.ElementTree as ET

import cindergraph as cg
import pytest


@pytest.mark.parametrize("representation", ["ast", "cfg", "ddg", "cdg", "pdg"])
@pytest.mark.parametrize(
    "character", ["\x00", "\x01", "\ufffe", "\uffff", "雪", "\U0001ffff"]
)
def test_graphml_character_boundaries(representation: str, character: str) -> None:
    code = f'int f(void){{char *s="a{character}b";return 0;}}'
    graphs = cg.export_graphs(code, repr=representation, format="graphml")
    assert graphs
    namespace = {"g": "http://graphml.graphdrawing.org/xmlns"}
    for _, xml in graphs:
        root = ET.fromstring(xml)
        if representation == "ast" and character in ("雪", "\U0001ffff"):
            assert character in "".join(root.itertext())
        assert root.tag == "{http://graphml.graphdrawing.org/xmlns}graphml"
        ids = {node.attrib["id"] for node in root.findall(".//g:node", namespace)}
        assert ids
        for edge in root.findall(".//g:edge", namespace):
            assert edge.attrib["source"] in ids
            assert edge.attrib["target"] in ids
