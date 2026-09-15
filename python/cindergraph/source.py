"""Measure C source: how big, how branchy, how nested, what it calls.

Everything Glaurung measured about C before this module existed was a
*comparison* -- graph edit distance, tree edit distance, type match, byte match
-- four metrics that score a decompilation against a ground truth and mean
nothing with only one side. This is the other kind: a property of a single
piece of source, which is what a reviewer picking what to read first, a
researcher featurizing a corpus, a CI gate refusing a function above a
threshold, and a decompiler author asking whether today's output is more
structured than yesterday's all actually want.

The parser underneath is pure Rust and total: no JVM, no subprocess, and no
input raises. A file that is not C at all yields zero functions and the
diagnostics saying so, and a file whose third function is unparseable still
reports the other two -- a front end that lost one function must not look like
one that lost the file.

Quick start:

    >>> import cindergraph
    >>> report = cindergraph.source.analyze("int f(int a) { return a ? 1 : 0; }")
    >>> report.functions[0].name, report.functions[0].cyclomatic
    ('f', 2)

    >>> for f in report.hotspots(by="cognitive", limit=5):
    ...     print(f.name, f.cognitive, f.max_nesting)
    f 1 0

Definitions for every metric are in the Rust module documentation
(`src/csource/metrics/`) and summarized in `docs/reference/source-metrics.md`;
the ones people disagree about -- cognitive complexity's `else if` rule,
Halstead's operator split -- are written out there rather than left implied.
"""

from __future__ import annotations

import statistics
from collections import Counter
from collections.abc import Iterator, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any, Literal

from cindergraph import _native

__all__ = [
    "COMPARED_METRICS",
    "EXPORT_FORMATS",
    "EXPORT_REPRS",
    "Diagnostic",
    "FunctionMetrics",
    "AnalysisSession",
    "NativeGraph",
    "ReachabilityResult",
    "ReachabilityStep",
    "SourceReport",
    "analyze",
    "analyze_path",
    "backward_slice",
    "call_summaries",
    "compare",
    "control_dependence",
    "control_flow_graphs",
    "data_flow",
    "export_graphs",
    "export_path",
    "feature_names",
    "features",
    "functions",
    "normalize",
    "native_graphs",
    "query_reaches",
    "query_reaches_by_id",
    "reaches",
]

if TYPE_CHECKING:  # pragma: no cover - declarations for the lazy attributes
    #: Graph representations :func:`export_graphs` can serialize.
    EXPORT_REPRS: tuple[str, ...]
    #: Wire formats :func:`export_graphs` can write.
    EXPORT_FORMATS: tuple[str, ...]

#: Names of the two module attributes served lazily by :func:`__getattr__`,
#: mapped to the key each reads out of the native choice table.
_EXPORT_CHOICES = {"EXPORT_REPRS": "repr", "EXPORT_FORMATS": "format"}


def __getattr__(name: str) -> tuple[str, ...]:
    """Serve :data:`EXPORT_REPRS` and :data:`EXPORT_FORMATS` on first use.

    Both read their values from Rust so the two sides cannot drift. Reading
    them at *import* time would be the wrong trade: ``cindergraph/__init__.py``
    imports this module eagerly, so a native extension built before
    ``export_choices`` existed would make the **whole package** unimportable
    rather than making one function fail. `cindergraph.source_cfg` defers its
    `networkx` import for the same reason.

    Args:
        name: The attribute being looked up.

    Returns:
        The tuple of accepted names, for the two attributes named above.

    Raises:
        AttributeError: For any other name, as a module must.
    """
    key = _EXPORT_CHOICES.get(name)
    if key is None:
        raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
    return tuple(_native.source.export_choices()[key])


#: The metric names :meth:`SourceReport.hotspots` will sort by, mapped to the
#: attribute that holds each. Restricting the sort key to a known set turns a
#: typo into an error at the call rather than a silently unsorted list.
_RANKABLE = (
    "cyclomatic",
    "cognitive",
    "max_nesting",
    "max_loop_depth",
    "lines",
    "code_lines",
    "tokens",
    "statements",
    "calls",
    "parameters",
    "decision_points",
    "loops",
    "unreachable_statements",
    "halstead_volume",
    "halstead_difficulty",
    "halstead_effort",
)


@dataclass(frozen=True, slots=True)
class ReachabilityStep:
    """One formal parameter carrying a value on a may-path."""

    function_id: int | None
    function: str
    parameter: int


@dataclass(frozen=True, slots=True)
class ReachabilityResult:
    """Evidence-bearing interprocedural reachability claim."""

    claim: Literal["found_may_path", "no_may_path", "unknown"]
    verdict: Literal["yes", "no", "unknown"]
    source_id: str
    input_dialect: Literal["ordinary", "preprocessed", "decompiled"]
    external_call_policy: Literal["unknown", "taint_return", "assume_pure_no_flow"]
    analysis_revision: int
    coverage_complete: bool
    path: tuple[ReachabilityStep, ...]
    explored: tuple[ReachabilityStep, ...]
    uncertainty: tuple[str, ...]


def _reachability_result(raw: Mapping[str, Any]) -> ReachabilityResult:
    """Convert the native wire mapping into the typed public result."""

    def step(item: Sequence[Any]) -> ReachabilityStep:
        return ReachabilityStep(
            function_id=item[0], function=item[1], parameter=item[2]
        )

    return ReachabilityResult(
        claim=raw["claim"],
        verdict=raw["verdict"],
        source_id=raw["source_id"],
        input_dialect=raw["input_dialect"],
        external_call_policy=raw["external_call_policy"],
        analysis_revision=raw["analysis_revision"],
        coverage_complete=raw["coverage_complete"],
        path=tuple(step(item) for item in raw["path"]),
        explored=tuple(step(item) for item in raw["explored"]),
        uncertainty=tuple(raw["uncertainty"]),
    )


class AnalysisSession:
    """One persistent parsed source snapshot for related graph queries.

    Use a session when asking for more than one graph family from the same
    source. Parsing happens once, and data-flow and summary results are cached
    by the native analysis unit. Returned dictionaries are fresh views, so a
    caller may modify them without corrupting the cached analysis.

    Args:
        code: C source text to analyze.
    """

    __slots__ = ("_native_session",)

    def __init__(
        self,
        code: str,
        *,
        dialect: Literal["ordinary", "preprocessed", "decompiled"] | None = None,
        external_calls: Literal[
            "unknown", "taint_return", "assume_pure_no_flow"
        ] = "unknown",
    ) -> None:
        """Create a persistent native analysis unit for ``code``."""
        self._native_session = _native.source.AnalysisSession(
            code, dialect=dialect, external_calls=external_calls
        )

    @property
    def source_id(self) -> str:
        """Stable identity of this exact source snapshot."""
        return self._native_session.source_id

    @property
    def source(self) -> str:
        """Exact normalized source whose bytes all result spans address."""
        return self._native_session.source

    @property
    def dialect(self) -> Literal["ordinary", "preprocessed", "decompiled"]:
        """Input preparation policy used to build this snapshot."""
        return self._native_session.dialect

    @property
    def external_calls(
        self,
    ) -> Literal["unknown", "taint_return", "assume_pure_no_flow"]:
        """Assumption used for named callees without a body."""
        return self._native_session.external_calls

    @property
    def diagnostics(self) -> tuple[Diagnostic, ...]:
        """Parser and CFG recovery diagnostics for the owned snapshot."""
        return tuple(Diagnostic(item) for item in self._native_session.diagnostics)

    def control_flow_graphs(self) -> list[dict[str, Any]]:
        """Return control-flow graphs for every recovered definition."""
        return self._native_session.control_flow_graphs()

    def data_flow(self) -> list[dict[str, Any]]:
        """Return cached data-flow analyses for every recovered definition."""
        return self._native_session.data_flow()

    def call_summaries(self) -> list[dict[str, Any]]:
        """Return cached interprocedural summaries for this source snapshot."""
        return self._native_session.call_summaries()

    def export_graphs(
        self, *, repr: str = "cfg", format: str = "dot"
    ) -> list[tuple[str, str]]:
        """Serialize graphs without rebuilding this session's analysis.

        Args:
            repr: ``"ast"``, ``"cfg"``, ``"ddg"``, ``"cdg"`` or ``"pdg"``.
            format: ``"dot"``, ``"graphml"``, ``"json"``, ``"mermaid"`` or
                ``"text"``.
        """
        return self._native_session.export_graphs(repr, format)

    def native_graphs(self, *, repr: str = "cfg") -> list[NativeGraph]:
        """Return read-only Rust-backed graphs without serialization."""
        return [
            NativeGraph(graph)
            for graph in self._native_session.native_graphs(repr=repr)
        ]

    def backward_slice(
        self, function: str, node: int, *, function_id: int | None = None
    ) -> list[int]:
        """Slice one owned CFG node over cached control and data dependence.

        Args:
            function: Recovered function name; duplicate names are rejected.
            node: Node ID in that function's executable CFG.
            function_id: Optional identity check from a session result.
        """
        return self._native_session.backward_slice(
            function, node, function_id=function_id
        )

    def query_reaches(
        self, source: str, parameter: int, sink: str
    ) -> ReachabilityResult:
        """Return a typed reachability claim from this session's summaries."""
        return _reachability_result(
            self._native_session.query_reaches(source, parameter, sink)
        )

    def query_reaches_by_id(
        self, source_function_id: int, parameter: int, sink_function_id: int
    ) -> ReachabilityResult:
        """Query exact source/sink identities, including duplicate names."""
        return _reachability_result(
            self._native_session.query_reaches_by_id(
                source_function_id, parameter, sink_function_id
            )
        )


class NativeGraph:
    """Read-only graph topology owned and traversed by the Rust extension.

    Node and edge dictionaries are materialized only when requested. Topology
    queries execute against compact native adjacency rather than Python graph
    objects. Use :meth:`to_networkx` only at an interoperability boundary.
    """

    __slots__ = ("_native_graph",)

    def __init__(self, native_graph: Any) -> None:
        """Wrap one extension-owned graph view."""
        self._native_graph = native_graph

    @property
    def name(self) -> str:
        """Recovered function name."""
        return self._native_graph.name

    @property
    def source_id(self) -> str:
        """Identity of the analyzed source snapshot."""
        return self._native_graph.source_id

    @property
    def function_id(self) -> int:
        """Dense function identity within :attr:`source_id`."""
        return self._native_graph.function_id

    @property
    def graph_kind(self) -> str:
        """Stable identity of graph-local node coordinates."""
        return self._native_graph.graph_kind

    @property
    def representation(self) -> Literal["ast", "cfg", "ddg", "cdg", "pdg"]:
        """Canonical requested graph representation."""
        return self._native_graph.representation

    @property
    def analysis_revision(self) -> int:
        """Semantic result-contract revision."""
        return self._native_graph.analysis_revision

    @property
    def input_dialect(self) -> Literal["ordinary", "preprocessed", "decompiled"]:
        """Source preparation policy used for this graph."""
        return self._native_graph.input_dialect

    @property
    def external_call_policy(
        self,
    ) -> Literal["unknown", "taint_return", "assume_pure_no_flow"]:
        """External-callee assumption attached to this graph snapshot."""
        return self._native_graph.external_call_policy

    @property
    def directed(self) -> bool:
        """Always true for Cindergraph graph representations."""
        return self._native_graph.directed

    @property
    def node_count(self) -> int:
        """Number of explicitly declared nodes."""
        return self._native_graph.node_count

    @property
    def edge_count(self) -> int:
        """Number of edges, including parallel edges."""
        return self._native_graph.edge_count

    def nodes(self) -> tuple[dict[str, Any], ...]:
        """Materialize node IDs, labels, and attributes in stable order."""
        return tuple(dict(node) for node in self._native_graph.nodes())

    def edges(self) -> tuple[dict[str, Any], ...]:
        """Materialize directed edges and attributes in stable order."""
        return tuple(dict(edge) for edge in self._native_graph.edges())

    def edge_list(self) -> tuple[tuple[int, int], ...]:
        """Return topology alone in one native-to-Python transfer."""
        return tuple(tuple(edge) for edge in self._native_graph.edge_list())

    def successors(self, node: int) -> tuple[int, ...]:
        """Immediate successors, preserving native edge order."""
        return tuple(self._native_graph.successors(node))

    def predecessors(self, node: int) -> tuple[int, ...]:
        """Immediate predecessors, preserving native edge order."""
        return tuple(self._native_graph.predecessors(node))

    def out_degree(self, node: int) -> int:
        """Count edges leaving a declared node."""
        return self._native_graph.out_degree(node)

    def in_degree(self, node: int) -> int:
        """Count edges entering a declared node."""
        return self._native_graph.in_degree(node)

    def descendants(self, node: int) -> tuple[int, ...]:
        """All reachable nodes after ``node``, sorted by ID."""
        return tuple(self._native_graph.descendants(node))

    def ancestors(self, node: int) -> tuple[int, ...]:
        """All nodes that can reach ``node``, sorted by ID."""
        return tuple(self._native_graph.ancestors(node))

    def to_networkx(self, *, multigraph: bool = False) -> Any:
        """Materialize an optional NetworkX graph for interoperability.

        Args:
            multigraph: Preserve parallel edges with ``MultiDiGraph``. The
                default ``DiGraph`` matches existing compatibility APIs and
                may collapse edges sharing endpoints.

        Raises:
            ImportError: NetworkX is not installed.
        """
        try:
            import networkx as nx
        except ImportError as error:
            raise ImportError(
                "Install cindergraph[graphs] to convert a native graph to NetworkX"
            ) from error
        graph = (nx.MultiDiGraph if multigraph else nx.DiGraph)(
            name=self.name,
            source_id=self.source_id,
            function_id=self.function_id,
            graph_kind=self.graph_kind,
            analysis_revision=self.analysis_revision,
        )
        for node in self.nodes():
            attributes = dict(node["attributes"])
            attributes["label"] = node["label"]
            graph.add_node(node["id"], **attributes)
        for edge in self.edges():
            attributes = dict(edge["attributes"])
            attributes["label"] = edge["label"]
            graph.add_edge(edge["source"], edge["target"], **attributes)
        return graph


#: What :func:`compare` reports by default: the metrics whose movement between
#: two builds is worth reading. Every one is an integer, so a delta is exact.
COMPARED_METRICS = (
    "cyclomatic",
    "cognitive",
    "max_nesting",
    "gotos",
    "unreachable_statements",
    "lines",
    "statements",
)


class Diagnostic:
    """One problem the parser reported, with the text that shows it.

    Attributes:
        severity: ``"error"`` or ``"warning"``.
        message: What went wrong.
        start: Start byte offset in the analyzed text.
        end: End byte offset, exclusive.
        text: The rendered excerpt, with a caret under the span.
    """

    __slots__ = ("end", "message", "severity", "start", "text")

    def __init__(self, raw: Mapping[str, Any]) -> None:
        """Wrap one diagnostic dict from the extension.

        Args:
            raw: The mapping the native layer produced.
        """
        self.severity: str = raw["severity"]
        self.message: str = raw["message"]
        self.start: int = raw["start"]
        self.end: int = raw["end"]
        self.text: str = raw["text"]

    def __repr__(self) -> str:
        """A one-line form naming the severity and message."""
        return f"<Diagnostic {self.severity}: {self.message}>"


class FunctionMetrics:
    """Everything measured about one function definition.

    The nested dicts the extension returns stay available as :attr:`raw`; the
    attributes below are the flattened names worth typing interactively. Both
    describe the same measurement, so neither is a summary of the other.
    """

    __slots__ = ("raw",)

    def __init__(self, raw: Mapping[str, Any]) -> None:
        """Wrap one function's measurement.

        Args:
            raw: The nested mapping the native layer produced.
        """
        self.raw = raw

    # --- identity ------------------------------------------------------------

    @property
    def name(self) -> str:
        """The declared name, empty when the declarator had none."""
        return self.raw["name"]

    @property
    def first_line(self) -> int:
        """1-based first line of the definition."""
        return self.raw["size"]["first_line"]

    @property
    def last_line(self) -> int:
        """1-based last line of the definition."""
        return self.raw["size"]["last_line"]

    @property
    def has_body(self) -> bool:
        """Whether the parser recovered a body for it."""
        return self.raw["has_body"]

    @property
    def parameters(self) -> int:
        """Declared parameters; ``(void)`` and ``()`` are both zero."""
        return self.raw["parameters"]

    # --- size ----------------------------------------------------------------

    @property
    def lines(self) -> int:
        """Physical lines the definition spans."""
        return self.raw["size"]["lines"]

    @property
    def code_lines(self) -> int:
        """Lines on which at least one token begins."""
        return self.raw["size"]["code_lines"]

    @property
    def tokens(self) -> int:
        """Tokens in the definition."""
        return self.raw["size"]["tokens"]

    # --- control flow --------------------------------------------------------

    @property
    def cyclomatic(self) -> int:
        """McCabe's number, ``E - N + 2`` over the reachable subgraph."""
        return self.raw["graph"]["cyclomatic"]

    @property
    def decision_points(self) -> int:
        """Branch points, as ``sum(max(0, out_degree - 1))``.

        Reported beside :attr:`cyclomatic` rather than folded into it: the two
        agree only when the graph has a single sink, and the gap is a count of
        constructs that never reach the function end.
        """
        return self.raw["graph"]["decision_points"]

    @property
    def loops(self) -> int:
        """Natural loops, counted as distinct back-edge destinations."""
        return self.raw["graph"]["loops"]

    @property
    def unreachable_statements(self) -> int:
        """Statements no path from the entry reaches. A lower bound."""
        return self.raw["unreachable_statements"]

    @property
    def node_kinds(self) -> Mapping[str, int]:
        """How many CFG nodes carry each kind: ``goto``, ``switch``, ...."""
        return self.raw["graph"]["node_kinds"]

    @property
    def gotos(self) -> int:
        """`goto` transfers in the function.

        The headline number for judging decompiler output: a structurer that
        gave up emits `goto` where a loop or an `if`/`else` belongs, and the
        count is how much it gave up.
        """
        return self.raw["graph"]["node_kinds"].get("goto", 0)

    @property
    def is_structured(self) -> bool:
        """Whether the function's control flow is expressible without `goto`.

        True when it contains no `goto` transfer. This is a statement about the
        *text*, not about what is possible: a function that could have been
        written with structured control flow but was not still reads False,
        which is the question a decompiler-output review is asking.
        """
        return self.gotos == 0

    # --- shape ---------------------------------------------------------------

    @property
    def cognitive(self) -> int:
        """Cognitive complexity, per the SonarSource specification."""
        return self.raw["shape"]["cognitive"]

    @property
    def max_nesting(self) -> int:
        """Deepest nesting of control structures, in levels."""
        return self.raw["shape"]["max_nesting"]

    @property
    def max_loop_depth(self) -> int:
        """Deepest nesting of loops specifically."""
        return self.raw["shape"]["max_loop_depth"]

    @property
    def statements(self) -> int:
        """Statement nodes of any kind."""
        return self.raw["shape"]["statements"]

    @property
    def calls(self) -> int:
        """Call expressions, one per argument list."""
        return self.raw["shape"]["calls"]

    @property
    def callees(self) -> Sequence[str]:
        """Distinct directly-named callees, sorted.

        A call through a function pointer or a struct member counts toward
        :attr:`calls` and appears here under no name, because there is none.
        """
        return self.raw["shape"]["callees"]

    @property
    def tag_counts(self) -> Mapping[str, int]:
        """How many AST nodes carry each C node tag."""
        return self.raw["shape"]["tag_counts"]

    # --- Halstead ------------------------------------------------------------

    @property
    def halstead(self) -> Mapping[str, float]:
        """Halstead's four counts and the three figures derived from them."""
        return self.raw["halstead"]

    @property
    def halstead_volume(self) -> float:
        """``N * log2(n)``."""
        return self.raw["halstead"]["volume"]

    @property
    def halstead_difficulty(self) -> float:
        """``(n1 / 2) * (N2 / n2)``."""
        return self.raw["halstead"]["difficulty"]

    @property
    def halstead_effort(self) -> float:
        """``difficulty * volume``."""
        return self.raw["halstead"]["effort"]

    # --- output --------------------------------------------------------------

    def to_dict(self) -> Mapping[str, Any]:
        """The measurement as plain nested data, ready for JSON."""
        return self.raw

    def __repr__(self) -> str:
        """A one-line form naming the function and its two complexities."""
        return (
            f"<FunctionMetrics {self.name!r} lines={self.lines} "
            f"cyclomatic={self.cyclomatic} cognitive={self.cognitive}>"
        )


class SourceReport:
    """Everything measured about one translation unit."""

    __slots__ = ("_functions", "name", "raw", "source")

    def __init__(
        self, raw: Mapping[str, Any], source: str, name: str | None = None
    ) -> None:
        """Wrap one file's measurement.

        Args:
            raw: The mapping the native layer produced.
            source: The exact text the offsets in `raw` refer to. When a
                dialect was applied this is the *normalized* text, not the
                caller's original.
            name: A display name for the unit, if the caller has one.
        """
        self.raw = raw
        self.source = source
        self.name = name
        self._functions = tuple(FunctionMetrics(f) for f in raw["functions"])

    @property
    def functions(self) -> tuple[FunctionMetrics, ...]:
        """Every function definition with a body, in source order."""
        return self._functions

    @property
    def diagnostics(self) -> tuple[Diagnostic, ...]:
        """Every problem the parser reported, in source order."""
        return tuple(Diagnostic(d) for d in self.raw["diagnostics"])

    @property
    def lines(self) -> int:
        """Physical lines in the file."""
        return self.raw["lines"]["lines"]

    @property
    def code_lines(self) -> int:
        """Lines on which at least one token begins."""
        return self.raw["lines"]["code_lines"]

    @property
    def blank_lines(self) -> int:
        """Lines with no non-whitespace byte."""
        return self.raw["lines"]["blank_lines"]

    @property
    def other_lines(self) -> int:
        """Non-blank lines carrying no token: comments, and the continuation
        lines of tokens that span several lines.

        Not called ``comment_lines``: the lexer stores no trivia, so the token
        buffer cannot tell a comment from a string literal's second line, and
        naming the bucket for its majority case would be a claim it cannot
        support.
        """
        return self.raw["lines"]["other_lines"]

    def hotspots(
        self, by: str = "cognitive", limit: int | None = 10
    ) -> tuple[FunctionMetrics, ...]:
        """The functions that score highest on one metric.

        Args:
            by: Any name in :data:`_RANKABLE` -- ``"cognitive"``,
                ``"cyclomatic"``, ``"max_nesting"``, ``"lines"``, and the rest.
            limit: Non-negative count to return, or `None` for all of them.

        Returns:
            Functions in descending order of `by`, with the name as a
            tie-break so the order is total and reproducible.

        Raises:
            ValueError: If `by` is not a rankable metric or `limit` is negative.
        """
        if by not in _RANKABLE:
            raise ValueError(
                f"cannot rank by {by!r}; expected one of {', '.join(_RANKABLE)}"
            )
        if limit is not None and limit < 0:
            raise ValueError("hotspot limit must be non-negative or None")
        ranked = sorted(self._functions, key=lambda f: (-getattr(f, by), f.name))
        return tuple(ranked if limit is None else ranked[:limit])

    def summary(self) -> Mapping[str, Any]:
        """Whole-unit aggregates, for a dashboard row or a build-over-build diff.

        Returns:
            A mapping with the file's line counts, the function count, how many
            functions are :attr:`FunctionMetrics.is_structured`, the total
            `goto` and unreachable-statement counts, and ``min``/``median``/
            ``mean``/``max`` for each metric in :data:`_RANKABLE`.

            A unit with no functions returns the same keys with the
            distributions absent rather than zeroed: a mean of zero over no
            functions is a number that reads like a measurement and is not one.
        """
        base: dict[str, Any] = {
            "name": self.name,
            "lines": self.lines,
            "code_lines": self.code_lines,
            "blank_lines": self.blank_lines,
            "other_lines": self.other_lines,
            "functions": len(self._functions),
            "structured_functions": sum(1 for f in self._functions if f.is_structured),
            "gotos": sum(f.gotos for f in self._functions),
            "unreachable_statements": sum(
                f.unreachable_statements for f in self._functions
            ),
            "diagnostics": len(self.raw["diagnostics"]),
        }
        if not self._functions:
            base["distributions"] = {}
            return base

        distributions: dict[str, Mapping[str, float]] = {}
        for metric in _RANKABLE:
            values = [float(getattr(f, metric)) for f in self._functions]
            distributions[metric] = {
                "min": min(values),
                "median": statistics.median(values),
                "mean": statistics.fmean(values),
                "max": max(values),
            }
        base["distributions"] = distributions
        return base

    def call_graph(self) -> Mapping[str, tuple[str, ...]]:
        """Which function calls which, within this translation unit.

        Returns:
            ``{caller: (callee, ...)}`` over every measured function, with the
            callees sorted. Names not defined in this unit are included --
            a call to `malloc` is a real edge, and dropping it would make the
            graph claim the function calls nothing.

            Indirect calls contribute no edge, because there is no name to
            record; :attr:`FunctionMetrics.calls` counts them and this does
            not, so the two disagree exactly where a pointer was called.

        Raises:
            ValueError: Duplicate function definitions cannot be represented
                by this name-keyed mapping. Inspect `functions` instead.
        """
        duplicates = sorted(
            name
            for name, count in Counter(f.name for f in self._functions).items()
            if count > 1
        )
        if duplicates:
            raise ValueError(f"duplicate function definitions: {', '.join(duplicates)}")
        return {f.name: tuple(f.callees) for f in self._functions}

    def defined_names(self) -> frozenset[str]:
        """The function names this unit defines.

        Intersect it with :meth:`call_graph` values to keep only the edges that
        stay inside the unit.
        """
        return frozenset(f.name for f in self._functions)

    def to_dict(self) -> Mapping[str, Any]:
        """The report as plain nested data, ready for JSON."""
        return self.raw

    def __len__(self) -> int:
        """How many functions were measured."""
        return len(self._functions)

    def __iter__(self) -> Iterator[FunctionMetrics]:
        """Iterate the functions in source order."""
        return iter(self._functions)

    def __repr__(self) -> str:
        """A one-line form naming the unit and its size."""
        where = f" {self.name!r}" if self.name else ""
        return (
            f"<SourceReport{where} functions={len(self._functions)} lines={self.lines}>"
        )


def normalize(code: str, dialect: str) -> str:
    """Apply the one text normalization pass a dialect is allowed to go through.

    Args:
        code: The source text.
        dialect: ``"preprocessed"`` for a gcc-preprocessed translation unit
            (`.i`), or ``"decompiled"`` for a decompiler backend's output.

    Returns:
        The normalized text. It is a *different string*, so any offset measured
        against it does not address the original.

    Raises:
        ValueError: If `dialect` is neither of the two names.
    """
    return _native.source.normalize(code, dialect)


def data_flow(code: str) -> list[dict[str, Any]]:
    """Reaching definitions, uses, and the dependences between them.

    Which write each read can see, computed by a reaching-definitions fixpoint
    over the general control-flow graph. Scope-aware, so a shadowed ``int x``
    in a nested block is a different variable from the one outside it.

    Args:
        code: The source text. Any byte sequence is acceptable; this never
            raises on account of the input.

    Returns:
        One dict per function, with ``name``, ``definitions``, ``uses``,
        ``edges``, ``unresolved_uses`` and ``dead_stores``. The two defect
        lists hold indices into ``uses`` and ``definitions`` respectively.
        ``source_id`` identifies the exact input bytes, ``function_id`` is
        dense within that snapshot, and ``analysis_revision`` identifies the
        semantic result contract. Graph-local IDs must not be mixed between
        different source IDs.

        A **dead store** is a write no read can see. Hand-written C has almost
        none; a decompiler that invents a temporary and never reads it produces
        one per invention, which is a readability defect an execution test
        cannot see because the return value is still right.

        An **unresolved use** is a read no write reaches: a global, a macro
        constant, a name from a header this parser never saw, or a genuine
        read of uninitialized storage.

        ``bindings`` is one entry per variable, with the type **as the source
        spells it** -- ``type``, ``specifiers``, ``pointer_depth``,
        ``array_rank``, ``is_const``, ``is_volatile``. Types are not resolved:
        this reads one translation unit and does not process ``#include``, so a
        typedef from a header is an opaque name. ``uint32_t`` is recorded as
        ``uint32_t`` and nothing claims to know it is four bytes.
        Pointer and array shape is scoped to each declarator; operators in an
        initializer or neighbouring declarator do not contribute to it.
        Parameter commas are balanced across nested signatures, and nested
        names are not promoted to outer bindings. A lone identifier remains
        ambiguous when its typedef declaration is unavailable. A preceding
        file-scope typedef is recognized, but ``size_t`` from an unprocessed
        header may be interpreted as a parameter name.

        ``type_conflicts`` lists bindings whose declaration sites disagree
        about type, which cannot happen in code a C compiler accepted and is
        the shape of a decompiler's type-recovery failure.
        ``unused_bindings`` lists variables declared and never read -- distinct
        from a dead store, because ``int *b;`` is not a store at all.

        Each definition and use carries ``binding``, an index into
        ``bindings``, so the three can be joined.
        A binding with ``is_unresolved=True`` has no recovered declaration;
        its type is ``None``. Different unresolved spellings have distinct IDs.
        Definition and use tables have stable analysis order, not source order:
        projected memory events can be appended after ordinary events. Use each
        event's byte ``start``/``end`` span when source ordering matters, while
        retaining its table index when joining edges or defect lists.

        ``effects_complete`` is false when an opaque value-producing builtin
        such as ``_Generic``, ``__builtin_choose_expr`` or
        ``__builtin_va_arg`` may contain reads, writes or calls that this
        syntax-level event pass cannot reconstruct. A clean negative result is
        not conclusive in that case, and summary ``complete`` includes this
        signal. Evaluated inline assembly also fails it because opaque operands
        and clobbers can read or write locals and arbitrary memory. A
        ``cleanup(function)`` declaration attribute fails it because the
        implicit scope-exit call is not yet represented.

        ``semantic_issues`` is the structured coverage ledger behind these
        compatibility flags. Each entry has a stable ``kind``, the affected
        analysis ``dimensions``, and an optional byte ``start``/``end`` span.
        A ``None`` span means the issue qualifies the whole function. Effect
        memory, and VLA producers attach the unsupported syntax span. Recovery
        attaches each diagnostic origin while conservatively qualifying the
        whole function.
        Parser recovery affects every dimension even though the narrower
        compatibility flags continue to report their individual passes.

        ``control_targets_complete`` is false when a computed goto needs a
        conservative target superset. Its semantic issue is attached to the
        indirect-dispatch span. An exact valid-target set whose evaluation may
        instead be invalid retains that fact in :func:`control_flow` without
        claiming that a valid destination is missing.

        ``memory_regions`` gives projected storage an identity separate from
        scalar bindings. Regions form a parent-linked table of binding roots,
        named fields, and conservative all-element summaries;
        ``memory_accesses`` associates operation-owned reads and writes with
        those IDs and labels each association ``exact`` or ``may_alias``.
        ``memory_definitions`` and ``memory_uses`` split these events for the
        region lattice; ``memory_edges`` joins possible reaching writes to
        reads by their table indices. ``memory_overlaps`` records structural
        containment and known union-member overlap. Cross-union edges retain
        distinct definition/use region IDs and name the overlap reason.
        Accessed fields of by-value aggregate parameters receive
        ``incoming_parameter`` definitions at entry. Direct aggregate and
        local-array bases identify storage and are not ordinary scalar uses.
        Pointer-like call arguments create weak ``call_clobber`` definitions
        for known local targets and keep memory coverage qualified until
        callee effects are modeled. Scalar-only calls do not do so; local
        arrays decay without becoming scalar reads or explicit ``&`` events.
        ``memory_complete`` therefore remains false when access exceeds the
        current local-pointer model, including unknown pointees and projected
        field/element flow. Known local pointees still receive compatibility
        ``memory_write`` definitions and projected reads.

        ``vla_complete`` is false when a variable-length-array typedef captures
        a value dependency that cannot yet be propagated through a later use
        of the typedef name. Direct local and outer-parameter VLA bounds are
        represented; nested function-pointer signatures stay separate.
        Ordinary ``sizeof`` and ``_Alignof`` operands inside a bound remain
        unevaluated, while a VLA type such as ``sizeof(int[n])`` reads ``n``.
        An array-bound ``_Generic`` fails this signal and reports no
        association as a definite read because selection needs C type
        compatibility unavailable to the token-level recovery.
        Direct assignments and increments in a bound are represented as
        writes, but arbitrary side-effect ordering is not reconstructed, so
        these forms also fail this signal.
        Treat a negative summary as conclusive only when its ``complete``
        field is true; summary completeness includes this signal.
    """
    return [dict(entry) for entry in _native.source.data_flow(code)]


def control_dependence(code: str) -> list[dict[str, Any]]:
    """Which branch decides each statement, per function.

    .. warning::

       Every offset this module reports is a **byte** offset, and Python
       slices ``str`` by character. On a file containing any non-ASCII byte --
       an em-dash in a comment, a UTF-8 string literal -- ``code[start:end]``
       silently returns the wrong text, shifted by the byte-minus-character
       difference. Slice ``code.encode()`` instead, or decode the result::

           raw = code.encode()
           text = raw[node["start"]:node["end"]].decode(errors="replace")

       This is not hypothetical: it read as a span-attribution bug in the CFG
       builder for an hour before the offsets turned out to be right and the
       verification wrong.

    Ferrante-Ottenstein-Warren control dependence over the general CFG, built
    on a post-dominator tree with a virtual exit so a function containing
    ``while (1) {}`` still has one.

    Args:
        code: The source text. Any byte sequence is acceptable.

    Returns:
        One dict per function with ``name``, ``nodes``, ``edges`` and
        ``unreachable_exit``. Each node carries its CFG ``id``, its ``kind``,
        its ``depth`` -- the longest chain of decisions above it -- and its
        immediate post-dominator ``ipdom``. Each edge carries ``on`` (the
        branch), ``node`` (what it decides) and ``kind`` (which arm).

        ``depth`` is a structural nesting measure computed on the graph, so
        unlike a brace count it is not fooled by a ``goto`` that leaves a block
        or by a decompiler's flattened dispatch.

        ``unreachable_exit`` lists nodes from which the function end cannot be
        reached: an infinite loop, a ``noreturn`` call, or a transfer the
        builder could not resolve.
    """
    return [dict(entry) for entry in _native.source.control_dependence(code)]


def backward_slice(
    code: str,
    function: str,
    node: int,
    *,
    source_id: str | None = None,
    function_id: int | None = None,
    graph_kind: str | None = None,
    analysis_revision: int | None = None,
) -> list[int]:
    """Every CFG node that can affect `node`, over the program-dependence graph.

    Walks control and data dependence backwards to a fixed point. This is the
    question a program-dependence graph exists to answer, and it needs both
    relations over one node set: following only one would silently omit the
    other's reasons.

    Check :func:`data_flow`'s ``memory_complete`` before interpreting omitted
    nodes: fields, arrays and unknown pointees can leave memory dependence
    incomplete. Known local-pointer writes are included conservatively.

    Args:
        code: The source text.
        function: The function to slice, by name.
        node: The CFG node id to slice on, as ``control_dependence`` numbers
            them.
        source_id: Optional identity copied from the graph that supplied
            ``node``. A mismatch is rejected.
        function_id: Optional snapshot-local function identity to validate.
        graph_kind: Optional graph identity; slicing accepts only
            ``"executable_cfg"``.
        analysis_revision: Optional semantic contract revision to validate.

    Returns:
        The node ids in the slice, sorted, including the seed.

    Raises:
        KeyError: If no function of that name was recovered.
        ValueError: If multiple definitions have the requested function name.
        IndexError: If the node ID is outside the selected function's graph.
    """
    return list(
        _native.source.backward_slice(
            code,
            function,
            node,
            source_id=source_id,
            function_id=function_id,
            graph_kind=graph_kind,
            analysis_revision=analysis_revision,
        )
    )


def call_summaries(code: str) -> list[dict[str, Any]]:
    """What each function does with the values passed to it, across calls.

    Interprocedural summaries at a fixed point over the call graph: which
    parameter reaches the return, and which reaches which other parameter. A
    caller applies the summary rather than re-analysing the callee, which is
    what makes recursion terminate -- summaries only grow, over a finite
    lattice.

    Return provenance includes control dependence: a parameter can select a
    return statement or a write whose value is returned. These are conservative
    may-dependences; equal-valued branches and algebraic cancellation are not
    simplified, and a reported flow is not a proof of a feasible execution.

    Args:
        code: The source text.

    Returns:
        One dict per function with ``name``, ``source_id``, ``function_id``,
        ``analysis_revision``, ``parameters``, ``complete`` and ``flows``.
        Identity fields match :func:`data_flow` for the same snapshot. Duplicate
        names retain separate function IDs and exact-body summaries, while
        name-based reachability queries fail closed as ambiguous. Each flow is
        ``{"parameter": n, "sink": "return" |
        "parameter", "sink_parameter": m | None}``.

        ``complete`` is ``False`` when the body held something this analysis
        could not resolve: an indirect call, which names no callee, or a call
        to a function this translation unit does not define. A caller applying
        an incomplete summary inherits ``unknown`` rather than a clean no.
    """
    return [dict(entry) for entry in _native.source.call_summaries(code)]


def reaches(code: str, source: str, parameter: int, sink: str) -> str:
    """Whether one function's parameter can reach another function.

    Returns ``"yes"``, ``"no"`` or ``"unknown"``. Unknown means the bounded
    analysis encountered an indirect call or a callee outside this translation
    unit; callers must not treat it as no.
    """
    return _native.source.reaches(code, source, parameter, sink)


def query_reaches(
    code: str, source: str, parameter: int, sink: str
) -> ReachabilityResult:
    """Return a typed may-path, complete negative, or explained unknown.

    Unlike :func:`reaches`, this preserves the path or uncertainty evidence
    behind the compatibility verdict.
    """
    return _reachability_result(
        _native.source.query_reaches(code, source, parameter, sink)
    )


def query_reaches_by_id(
    code: str, source_function_id: int, parameter: int, sink_function_id: int
) -> ReachabilityResult:
    """Query exact function identities without selecting duplicate names."""
    return _reachability_result(
        _native.source.query_reaches_by_id(
            code, source_function_id, parameter, sink_function_id
        )
    )


def export_graphs(
    code: str, *, repr: str = "cfg", format: str = "dot"
) -> list[tuple[str, str]]:
    """Serialize every function's graph in one wire format.

    A native alternative to ``joern-export`` for AST, CFG, CDG, DDG and PDG
    representations in DOT, GraphML, JSON or Mermaid. This is Cindergraph's
    own graph model, not a Joern code-property graph or JIL representation.

    ``repr="cfg"`` exports the *general* control-flow graph, never the
    parity one -- the graph a person would draw, with real successors,
    real join points and real loop back edges. Use
    :func:`cindergraph.source_cfg.parity_cfgs` when the parity shape is what you
    want.

    Args:
        code: The source text. Any byte sequence is acceptable; a file the
            parser only partly recovered exports the functions it did build.
        repr: One of :data:`EXPORT_REPRS`.
        format: One of :data:`EXPORT_FORMATS`. ``"json"`` is node-link JSON
            with the edge array under ``"edges"``, which NetworkX 3.6 reads by
            default; ``"mermaid"`` renders in Markdown without Graphviz.

    Returns:
        One ``(function name, serialized graph)`` pair per function, in source
        order. A list rather than a dict, because two definitions in one file
        can carry the same name after recovery.

    Raises:
        ValueError: If `repr` or `format` is not a known name.
    """
    return [
        (name, body) for name, body in _native.source.export_graphs(code, repr, format)
    ]


def native_graphs(code: str, *, repr: str = "cfg") -> list[NativeGraph]:
    """Build read-only Rust-backed graphs without serializing topology.

    Use this for traversal or large graph products. Node and edge attributes
    cross into Python only when :meth:`NativeGraph.nodes` or
    :meth:`NativeGraph.edges` is requested.

    Args:
        code: C source text to analyze.
        repr: ``"ast"``, ``"cfg"``, ``"ddg"``, ``"cdg"`` or ``"pdg"``.

    Returns:
        One native graph per recovered function, in source order.

    Raises:
        ValueError: If `repr` is not recognized.
    """
    return [
        NativeGraph(graph) for graph in _native.source.native_graphs(code, repr=repr)
    ]


def export_path(
    path: str | Path,
    *,
    repr: str = "cfg",
    format: str = "dot",
    dialect: str | None = None,
) -> list[tuple[str, str]]:
    """:func:`export_graphs` over a file, read lossily.

    Args:
        path: The file to read. Decoded with ``errors="replace"``, because a
            decompiler's output is not always valid UTF-8 and losing the file
            over one byte would be worse than losing the byte.
        repr: One of :data:`EXPORT_REPRS`.
        format: One of :data:`EXPORT_FORMATS`.
        dialect: Passed to :func:`normalize` first when given. Note that
            ``"preprocessed"`` empties an ordinary ``.c`` file; it is only for a
            real ``gcc -E`` unit.

    Returns:
        One ``(function name, serialized graph)`` pair per function.

    Raises:
        OSError: If the file cannot be read.
        ValueError: If `repr`, `format` or `dialect` is not a known name.
    """
    code = Path(path).read_bytes().decode("utf-8", errors="replace")
    if dialect is not None:
        code = normalize(code, dialect)
    return export_graphs(code, repr=repr, format=format)


def analyze(
    code: str, *, name: str | None = None, dialect: str | None = None
) -> SourceReport:
    """Measure one translation unit of C.

    Args:
        code: The source text. Any byte sequence is acceptable; this never
            raises on account of the input.
        name: A display name for the unit, used only in `repr`.
        dialect: When given, :func:`normalize` runs first and the report
            describes the normalized text, which :attr:`SourceReport.source`
            then holds. Pass ``"decompiled"`` for decompiler output and
            ``"preprocessed"`` for a `.i` file; omit it for ordinary C.

    Returns:
        The report. A file with no recoverable function yields an empty
        :attr:`SourceReport.functions` and the diagnostics explaining why --
        never an exception, because a caller cannot otherwise tell an empty
        file from a failed one.
    """
    text = normalize(code, dialect) if dialect is not None else code
    return SourceReport(_native.source.analyze(text), text, name)


def analyze_path(path: str | Path, *, dialect: str | None = None) -> SourceReport:
    """Measure one file of C.

    Args:
        path: The file to read. Invalid UTF-8 is replaced rather than raising,
            matching how the corpus tooling reads decompiler output.
        dialect: As :func:`analyze`.

    Returns:
        The report, named after the file.
    """
    path = Path(path)
    code = path.read_bytes().decode("utf-8", errors="replace")
    return analyze(code, name=str(path), dialect=dialect)


def functions(code: str) -> Sequence[Mapping[str, Any]]:
    """List a file's function definitions without measuring any of them.

    One parse and no graph construction, for deciding what to measure.

    Args:
        code: The source text.

    Returns:
        One mapping per definition with ``name``, ``start``, ``end``,
        ``first_line``, ``last_line`` and ``has_body``.
    """
    return _native.source.functions(code)


def control_flow_graphs(code: str) -> Sequence[Mapping[str, Any]]:
    """Every function's general control-flow graph.

    This is the graph a person would draw -- real successors, real join points,
    real loop back edges, typed nodes and edges. It is deliberately *not*
    :func:`cindergraph.source_cfg.parity_cfgs`, which reproduces another tool's
    artifacts so that one similarity score can be compared against it.

    Args:
        code: The source text.

    Returns:
        One mapping per function with ``name``, ``source_id``, ``function_id``,
        ``analysis_revision``, ``graph_kind``, ``start``, ``end``,
        ``short_circuits`` and a ``cfg`` holding ``nodes``, ``edges``,
        ``entry``, ``exit`` and ``indirect_dispatches``. Each indirect dispatch
        reports its node and target IDs, ``exact`` or ``conservative``
        precision, invalid-transfer risk, and stable uncertainty reasons. A
        list rather than a name-keyed mapping,
        because two definitions in one file can carry the same name after
        recovery and a mapping would silently drop one.
    """
    return _native.source.control_flow_graphs(code)


def feature_names() -> Sequence[str]:
    """The column names :func:`features` returns, in order.

    Returns:
        A fixed, ordered, documented vector of column names. Stability is the
        point: rows stacked across files and across releases have to mean the
        same thing column by column.
    """
    return _native.source.feature_names()


def features(code: str) -> Sequence[tuple[str, Sequence[float]]]:
    """One fixed-width numeric row per function, for corpus-scale work.

    The same data :func:`analyze` reports, flattened.

    This is for the *stable column vector*, not for speed. Measured over
    ``tests/decompiler_fixtures/src`` (196 files, 900 functions, 0.78 MB) on a
    ``maturin develop --release`` build, best of five: ``analyze`` 43.8 ms,
    ``features`` 41.3 ms -- 0.94x. Parsing and graph construction dominate.
    What a row buys is a meaning fixed by :func:`feature_names` that does not
    move when the report's dict schema gains a key.

    Args:
        code: The source text.

    Returns:
        ``(name, row)`` pairs in source order, each row as long as
        :func:`feature_names`.
    """
    return _native.source.features(code)


def compare(
    before: SourceReport,
    after: SourceReport,
    *,
    metrics: Sequence[str] = COMPARED_METRICS,
) -> Mapping[str, Any]:
    """Per-function metric movement between two reports.

    The shape both build-over-build regression tracking and cross-decompiler
    comparison reduce to: measure two pieces of C, match their functions, and
    read what moved. `before` might be last release's output and `after` this
    one's, or one decompiler's output and another's, or the original source and
    the decompilation of it.

    Args:
        before: The baseline report.
        after: The report to compare against it.
        metrics: Which :class:`FunctionMetrics` attributes to difference.
            Defaults to :data:`COMPARED_METRICS`.

    Returns:
        A mapping with:

        * ``matched`` -- one entry per name present in both, each
          ``{"name", "before", "after", "deltas"}`` where ``deltas`` maps each
          metric to ``after - before``. Sorted by the largest absolute delta
          first, so the functions that moved most read first.
        * ``added`` / ``removed`` -- names present in only one side, sorted.
        * ``totals`` -- for each metric, the sum over *matched* functions on
          each side and the difference. Matched only, deliberately: a total
          that mixed in added and removed functions would attribute their whole
          weight to a regression.

    Raises:
        ValueError: If a name in `metrics` is not a rankable metric or
            ``gotos``, a metric is repeated, or either report has duplicate
            function names. Ambiguous matching cannot produce valid totals.

    Note:
        Functions are matched by unique name. Duplicate definitions must be
        resolved by the caller before comparison; no body is silently selected.
        Check ``added``/``removed`` when the counts do not line up.
    """
    allowed = set(_RANKABLE) | {"gotos"}
    unknown = [m for m in metrics if m not in allowed]
    if unknown:
        raise ValueError(
            f"cannot compare on {', '.join(unknown)}; "
            f"expected names from {', '.join(sorted(allowed))}"
        )

    repeated = sorted(name for name, count in Counter(metrics).items() if count > 1)
    if repeated:
        raise ValueError(f"duplicate comparison metrics: {', '.join(repeated)}")
    for side, report in (("before", before), ("after", after)):
        duplicates = sorted(
            name
            for name, count in Counter(f.name for f in report.functions).items()
            if count > 1
        )
        if duplicates:
            raise ValueError(
                f"{side} report has duplicate function names: {', '.join(duplicates)}"
            )

    left = {f.name: f for f in before.functions}
    right = {f.name: f for f in after.functions}
    shared = sorted(set(left) & set(right))

    matched: list[Mapping[str, Any]] = []
    totals: dict[str, Mapping[str, float]] = {
        metric: {"before": 0.0, "after": 0.0, "delta": 0.0} for metric in metrics
    }
    for name in shared:
        deltas: dict[str, float] = {}
        values_before: dict[str, float] = {}
        values_after: dict[str, float] = {}
        for metric in metrics:
            a = float(getattr(left[name], metric))
            b = float(getattr(right[name], metric))
            values_before[metric] = a
            values_after[metric] = b
            deltas[metric] = b - a
            bucket = totals[metric]
            totals[metric] = {
                "before": bucket["before"] + a,
                "after": bucket["after"] + b,
                "delta": bucket["delta"] + (b - a),
            }
        matched.append(
            {
                "name": name,
                "before": values_before,
                "after": values_after,
                "deltas": deltas,
            }
        )

    # Largest movement first, name as the tie-break so the order is total.
    matched.sort(
        key=lambda entry: (
            -max((abs(v) for v in entry["deltas"].values()), default=0.0),
            entry["name"],
        )
    )
    return {
        "matched": matched,
        "added": sorted(set(right) - set(left)),
        "removed": sorted(set(left) - set(right)),
        "totals": totals,
    }


# File-based graph and function APIs. The implementation keeps NetworkX lazy.
from cindergraph.compat.pyjoern import (
    Function as Function,
)
from cindergraph.compat.pyjoern import (
    SourceParseError as SourceParseError,
)
from cindergraph.compat.pyjoern import (
    SourceParseWarning as SourceParseWarning,
)
from cindergraph.compat.pyjoern import (
    fast_cfgs_from_source as fast_cfgs_from_source,
)
from cindergraph.compat.pyjoern import (
    parse_callgraph as parse_callgraph,
)
from cindergraph.compat.pyjoern import (
    parse_source as parse_source,
)

__all__ += [
    "Function",
    "SourceParseError",
    "SourceParseWarning",
    "fast_cfgs_from_source",
    "parse_callgraph",
    "parse_source",
]
