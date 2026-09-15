"""Representative downstream typing against an installed Cindergraph wheel."""

from collections.abc import Mapping, Sequence
from typing import Any, assert_type

import cindergraph as cg


code = "int f(int x){return x ? 1 : 0;}"
report = cg.analyze(code, name="sample.c")
assert_type(report, cg.SourceReport)
assert_type(report.name, str | None)
assert_type(report.functions, tuple[cg.FunctionMetrics, ...])
function = report.functions[0]
assert_type(function.name, str)
assert_type(function.cyclomatic, int)
assert_type(report.hotspots(by="cyclomatic", limit=1), tuple[cg.FunctionMetrics, ...])

assert_type(cg.normalize(code, "decompiled"), str)
assert_type(cg.data_flow(code), list[dict[str, Any]])
assert_type(cg.control_dependence(code), list[dict[str, Any]])
assert_type(cg.backward_slice(code, "f", 0), list[int])
assert_type(cg.call_summaries(code), list[dict[str, Any]])
assert_type(cg.reaches(code, "f", 0, "f"), str)
assert_type(cg.export_graphs(code, repr="cfg", format="json"), list[tuple[str, str]])
assert_type(cg.functions(code), Sequence[Mapping[str, Any]])
assert_type(cg.feature_names(), Sequence[str])
assert_type(cg.features(code), Sequence[tuple[str, Sequence[float]]])
assert_type(cg.compare(report, report), Mapping[str, Any])
