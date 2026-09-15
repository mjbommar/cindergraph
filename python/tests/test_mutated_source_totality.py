"""Seeded corruption of a real C fixture exercises public recovery boundaries."""

import json
from pathlib import Path
import random
import xml.etree.ElementTree as ET

import cindergraph as cg
import pytest


ROOT = Path(__file__).resolve().parents[2]
MATERIALIZED = ROOT / "docs/benchmarks/corpus/mutations"
FIXTURES = (
    [ROOT / "tests/fixtures/sizeof_local.c"]
    + sorted((ROOT / "crates/cindergraph/tests/decompiler_fixtures/src").glob("*.c"))
    + sorted((ROOT / "crates/cindergraph/tests/decbench_corpus/src").glob("*.c"))
)


def mutated_source(seed: int) -> str:
    """Reproduce deletions, truncation, and unusual text at a fixed seed."""
    rng = random.Random(seed)
    text = FIXTURES[seed % len(FIXTURES)].read_text(encoding="utf-8")[:8192]
    insertions = ["\x00", "\ufffe", "λ", "\r\n", '"', "/*", "}", "(", "#if X\n"]
    for _ in range(4):
        start = rng.randrange(len(text) + 1)
        end = min(len(text), start + rng.randrange(12))
        text = text[:start] + rng.choice(insertions) + text[end:]
    if seed % 3 == 0:
        text = text[: rng.randrange(len(text) + 1)]
    return text


def test_mutation_corpus_is_present() -> None:
    assert len(FIXTURES) == 211


@pytest.mark.parametrize("seed", range(256))
def test_mutated_fixture_is_total_and_deterministic(seed: int) -> None:
    code = mutated_source(seed)
    assert (MATERIALIZED / f"seed-{seed:03}.c").read_bytes() == code.encode("utf-8")
    assert cg.analyze(code).to_dict() == cg.analyze(code).to_dict(), seed
    for operation in (
        cg.data_flow,
        cg.call_summaries,
        cg.control_dependence,
        cg.control_flow_graphs,
    ):
        first = operation(code)
        second = operation(code)
        assert first == second, (seed, operation.__name__)
    for representation in ("ast", "cfg", "ddg", "cdg", "pdg"):
        graphs = cg.export_graphs(code, repr=representation, format="json")
        assert graphs == cg.export_graphs(code, repr=representation, format="json")
        for _, serialized in graphs:
            graph = json.loads(serialized)
            ids = [node["id"] for node in graph["nodes"]]
            assert len(ids) == len(set(ids)), (seed, representation)
            nodes = set(ids)
            assert all(
                edge["source"] in nodes and edge["target"] in nodes
                for edge in graph["edges"]
            ), (seed, representation)
        xml_graphs = cg.export_graphs(code, repr=representation, format="graphml")
        assert len(xml_graphs) == len(graphs), (seed, representation)
        for _, document in xml_graphs:
            ET.fromstring(document)
