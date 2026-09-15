"""Downstream typing for the optional NetworkX graph adapters."""

from typing import Any, assert_type

import cindergraph as cg
import networkx as nx


code = "int f(int x){if(x)return 1;return 0;}"
serialized = cg.source_cfg.parity_cfgs(code)
assert_type(serialized, dict[str, dict[str, Any]])

graph = cg.source_cfg.graph_from_serialized(serialized["f"])
assert_type(graph, nx.DiGraph)
assert_type(cg.source_cfg.cfgs_from_decompiled(code), dict[str, nx.DiGraph])

node = next(iter(graph))
assert_type(node, Any)
source_node = cg.source_cfg.SourceCfgNode(0, is_entrypoint=True)
assert_type(source_node.id, int)
assert_type(source_node.is_entrypoint, bool)
assert_type(source_node.is_exitpoint, bool)
