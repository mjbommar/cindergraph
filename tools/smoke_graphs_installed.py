"""Smoke-test the optional NetworkX adapters from an isolated wheel install."""

from importlib import metadata
import json
from pathlib import Path
import sys

import cindergraph as cg
import networkx as nx


def main() -> None:
    """Exercise both native graph exports and the compatibility adapter."""
    prefix = Path(sys.prefix).resolve()
    for module in (cg, cg._native, nx):
        path = Path(module.__file__).resolve()
        if not path.is_relative_to(prefix):
            raise RuntimeError(f"Module loaded outside isolated environment: {path}")

    code = "int f(int x){if(x)return 1;return 0;}"
    serialized = cg.source_cfg.parity_cfgs(code)["f"]
    parity = cg.source_cfg.cfgs_from_decompiled(code)["f"]
    assert isinstance(parity, nx.DiGraph)
    assert parity.number_of_nodes() > 1
    assert {node.id for node in parity if node.is_entrypoint} == set(
        serialized["entry"]
    )
    assert {node.id for node in parity if node.is_exitpoint} == set(serialized["exit"])
    assert {(source.id, target.id) for source, target in parity.edges} == set(
        map(tuple, serialized["edges"])
    )

    body = dict(cg.export_graphs(code, repr="cfg", format="json"))["f"]
    general = nx.node_link_graph(json.loads(body))
    assert general.is_directed()
    assert general.number_of_nodes() > parity.number_of_nodes()

    distribution = metadata.distribution("cindergraph")
    requirements = distribution.requires or []
    assert any("networkx" in item and "graphs" in item for item in requirements)
    print(
        f"Installed cindergraph {distribution.version} with "
        f"NetworkX {nx.__version__}: {cg._native.__file__}"
    )


if __name__ == "__main__":
    main()
