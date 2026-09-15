"""Glaurung's C source CFGs, adapted to what DecBench's GED metric reads.

`decbench.metrics.vj_ged.vj_ged` compares two `networkx.DiGraph` objects using
only ``list(g.nodes)``, ``g.in_degree(n)``, ``g.out_degree(n)`` and each node's
``is_entrypoint`` / ``is_exitpoint`` attributes. Nothing else about a node
reaches the distance -- no statements, no types, no labels. So the adaptation
from :func:`cindergraph._native.csource.parity_cfgs` is purely mechanical, and the
node class below carries exactly those two flags and an identity.

`networkx` is imported inside the function rather than at module scope: it is a
DecBench dependency, not a Glaurung one, and `cindergraph/__init__.py` imports this
module so that ``cindergraph.source_cfg`` resolves without a separate import. A
module-level import would make the whole package unimportable wherever the graph
library is absent.

Entry point for `tools/source_cfg_parity.py --provider cindergraph`; the plan it
gates is `docs/design/static-c-analysis/parity-plan.md`.
"""

from __future__ import annotations

import logging
import re
import shutil
import subprocess
import tempfile
from dataclasses import dataclass
from typing import TYPE_CHECKING, Any, Literal

from cindergraph import _native

if TYPE_CHECKING:  # pragma: no cover - typing only
    import networkx

__all__ = [
    "DecompiledCfgAnalysis",
    "FunctionProvenance",
    "PreprocessingReport",
    "SourceCfgNode",
    "analyze_decompiled",
    "cfgs_from_decompiled",
    "graph_from_serialized",
    "parity_cfgs",
    "preprocess_decompiled",
]

logger = logging.getLogger(__name__)
_PREPROCESSOR_CONTROL = re.compile(
    r"^\s*#\s*(?:define|undef|if|ifdef|ifndef|elif|else|endif)\b", re.MULTILINE
)
_INCLUDE_DIRECTIVE = re.compile(r"^\s*#\s*include\b[^\n]*(?:\n|$)", re.MULTILINE)


@dataclass(frozen=True)
class PreprocessingReport:
    """Observable outcome of preparing decompiler C for CFG recovery.

    Attributes:
        text: Text that should be analyzed. This is the original input when no
            preprocessing was needed or preprocessing could not run.
        status: ``"not-needed"``, ``"succeeded"``, ``"unavailable"``,
            ``"failed"``, or ``"timed-out"``.
        compiler: Resolved preprocessor executable, if one was found.
        command: Exact command used, excluding the temporary input filename.
        stderr: Captured preprocessor error output or exception text.
        includes_removed: Whether include directives were removed before
            invoking the preprocessor. Includes are never loaded implicitly.

    The type contains only standard-library values. A wheel therefore retains
    zero mandatory runtime dependencies and callers can decide whether a
    fail-open result is acceptable instead of having to infer it from logs.
    """

    text: str
    status: Literal["not-needed", "succeeded", "unavailable", "failed", "timed-out"]
    compiler: str | None = None
    command: tuple[str, ...] = ()
    stderr: str = ""
    includes_removed: bool = False

    @property
    def succeeded(self) -> bool:
        """Whether preprocessing ran and produced non-empty output."""
        return self.status == "succeeded"


@dataclass(frozen=True)
class DecompiledCfgAnalysis:
    """GED-ready graphs together with their preprocessing evidence."""

    graphs: dict[str, Any]
    preprocessing: PreprocessingReport
    provenance: dict[str, FunctionProvenance]
    diagnostics: tuple[dict[str, Any], ...]


@dataclass(frozen=True)
class FunctionProvenance:
    """How one recovered definition reached the analyzed translation unit.

    ``origin`` and ``recovery_qualified`` are deliberately independent. A
    macro-generated definition can itself require parser recovery, and callers
    must not have to collapse those two facts into one ambiguous label.
    """

    name: str
    origin: Literal["source", "expansion-generated"]
    recovery_qualified: bool
    start: int
    end: int
    diagnostic_count: int


def preprocess_decompiled(
    text: str,
    *,
    compiler: str | None = None,
    timeout: float = 30.0,
) -> PreprocessingReport:
    """Expand local directives and return the complete preparation outcome.

    The native parser intentionally has no compiler dependency. This adapter
    is the DecBench-facing boundary, where Joern likewise receives text after
    local macro expansion and conditional selection. Includes are removed so
    a decompiler cannot make the host preprocessor import arbitrary headers;
    failure is explicit in the report and its ``text`` remains the original
    tolerant-parser input.

    Args:
        text: Decompiled C source text.
        compiler: Preprocessor executable. When omitted, ``gcc`` and then
            ``cc`` are discovered on ``PATH``. Passing a value makes the
            selected executable deterministic.
        timeout: Maximum preprocessing time in seconds.

    Returns:
        A dependency-free :class:`PreprocessingReport`.
    """
    if _PREPROCESSOR_CONTROL.search(text) is None:
        return PreprocessingReport(text=text, status="not-needed")
    resolved = shutil.which(compiler) if compiler is not None else None
    if compiler is None:
        resolved = shutil.which("gcc") or shutil.which("cc")
    if resolved is None:
        message = (
            "no host C preprocessor"
            if compiler is None
            else f"preprocessor not found: {compiler}"
        )
        logger.warning("cannot preprocess decompiled C: %s", message)
        return PreprocessingReport(text=text, status="unavailable", stderr=message)
    safe_text = _INCLUDE_DIRECTIVE.sub("\n", text)
    includes_removed = safe_text != text
    command = (resolved, "-E", "-P", "-x", "c")
    try:
        with tempfile.NamedTemporaryFile(mode="w", suffix=".c") as source:
            source.write(safe_text)
            source.flush()
            result = subprocess.run(
                [*command, source.name],
                check=False,
                capture_output=True,
                text=True,
                timeout=timeout,
            )
    except subprocess.TimeoutExpired as error:
        logger.warning("cannot preprocess decompiled C: %s", error)
        return PreprocessingReport(
            text=text,
            status="timed-out",
            compiler=resolved,
            command=command,
            stderr=str(error),
            includes_removed=includes_removed,
        )
    except OSError as error:
        logger.warning("cannot preprocess decompiled C: %s", error)
        return PreprocessingReport(
            text=text,
            status="failed",
            compiler=resolved,
            command=command,
            stderr=str(error),
            includes_removed=includes_removed,
        )
    if result.returncode != 0 or not result.stdout.strip():
        message = (
            result.stderr.strip()
            or f"preprocessor exited {result.returncode} without output"
        )
        logger.warning("cannot preprocess decompiled C: %s", message)
        return PreprocessingReport(
            text=text,
            status="failed",
            compiler=resolved,
            command=command,
            stderr=message,
            includes_removed=includes_removed,
        )
    return PreprocessingReport(
        text=result.stdout,
        status="succeeded",
        compiler=resolved,
        command=command,
        stderr=result.stderr.strip(),
        includes_removed=includes_removed,
    )


def _preprocess_decompiled_c(text: str) -> str:
    """Compatibility shim returning only the prepared text."""
    return preprocess_decompiled(text).text


class SourceCfgNode:
    """A CFG basic block reduced to the two role flags GED inspects.

    Identity is the block id, so two nodes of the same graph are distinct
    dictionary keys and `networkx` keeps them apart. The shape deliberately
    mirrors `decbench.publish.cfg_export.CfgNode`, which is what the published
    side of every comparison is rebuilt from.

    Attributes:
        id: Dense block id, ``0..n-1`` within its function.
        is_entrypoint: Whether the block is the function's entry.
        is_exitpoint: Whether the block leaves the function.
    """

    __slots__ = ("id", "is_entrypoint", "is_exitpoint")

    def __init__(
        self, id: int, is_entrypoint: bool = False, is_exitpoint: bool = False
    ) -> None:
        """Build a node.

        Args:
            id: Dense block id within the function.
            is_entrypoint: Whether the block is the function's entry.
            is_exitpoint: Whether the block leaves the function.
        """
        self.id = id
        self.is_entrypoint = is_entrypoint
        self.is_exitpoint = is_exitpoint

    def __hash__(self) -> int:
        return hash(self.id)

    def __eq__(self, other: object) -> bool:
        return isinstance(other, SourceCfgNode) and other.id == self.id

    def __repr__(self) -> str:
        return f"n{self.id}"


def parity_cfgs(text: str) -> dict[str, dict[str, Any]]:
    """Raw per-function CFGs for one translation unit of C.

    Args:
        text: C source text, typically a decompiler's whole output file.

    Returns:
        ``{function name: {"nodes", "edges", "entry", "exit", "degenerate"}}``,
        the serialized shape DecBench stores its published source CFGs in.
    """
    return _native.csource.parity_cfgs(text)


def graph_from_serialized(cfg: dict[str, Any]) -> networkx.DiGraph:
    """Build one GED-ready graph from a serialized CFG.

    The mirror of `decbench.publish.cfg_export.rebuild_cfg`, which does this for
    the published side of every comparison. ``degenerate`` is read but not
    represented: it drives DecBench's offline translation-unit resolution, and
    `vj_ged` never sees it.

    Args:
        cfg: One function's ``{"nodes", "edges", "entry", "exit", "degenerate"}``
            mapping, as :func:`parity_cfgs` returns it.

    Returns:
        A `networkx.DiGraph` over :class:`SourceCfgNode`.

    Raises:
        ImportError: If `networkx` is not installed in the running environment.
        KeyError: If an edge names a node the ``nodes`` list does not declare.
    """
    import networkx as nx

    entries = set(cfg["entry"])
    exits = set(cfg["exit"])
    by_id = {
        node_id: SourceCfgNode(node_id, node_id in entries, node_id in exits)
        for node_id in cfg["nodes"]
    }
    graph: networkx.DiGraph = nx.DiGraph()
    # Add nodes first: a block with no edges still occupies a row of the cost
    # matrix `vj_ged` builds, and inferring nodes from the edge list alone would
    # silently drop it -- shrinking the matrix and cheapening the distance.
    graph.add_nodes_from(by_id.values())
    for src, dst in cfg["edges"]:
        graph.add_edge(by_id[src], by_id[dst])
    return graph


def _function_provenance(
    original: str,
    preprocessing: PreprocessingReport,
    scoreable_names: set[str],
) -> tuple[dict[str, FunctionProvenance], tuple[dict[str, Any], ...]]:
    """Derive definition origin and local recovery from observable artifacts."""
    original_names = set(parity_cfgs(original))
    session = _native.source.AnalysisSession(preprocessing.text)
    diagnostics = session.diagnostics
    provenance: dict[str, FunctionProvenance] = {}
    for function in session.control_flow_graphs():
        name = function["name"]
        if name not in scoreable_names:
            continue
        start = function["start"]
        end = function["end"]
        local = [
            diagnostic
            for diagnostic in diagnostics
            if diagnostic["start"] <= end and diagnostic["end"] >= start
        ]
        origin: Literal["source", "expansion-generated"] = "source"
        if preprocessing.succeeded and name not in original_names:
            origin = "expansion-generated"
        candidate = FunctionProvenance(
            name=name,
            origin=origin,
            recovery_qualified=bool(local),
            start=start,
            end=end,
            diagnostic_count=len(local),
        )
        previous = provenance.get(name)
        if previous is None or (candidate.end - candidate.start) > (
            previous.end - previous.start
        ):
            provenance[name] = candidate
    return provenance, tuple(dict(diagnostic) for diagnostic in diagnostics)


def analyze_decompiled(
    text: str,
    *,
    compiler: str | None = None,
    timeout: float = 30.0,
) -> DecompiledCfgAnalysis:
    """Build GED-ready CFGs and retain preprocessing evidence.

    This is the provider entry point `tools/source_cfg_parity.py` resolves.

    Args:
        text: Decompiled C source text. May be partly unparseable; the front end
            is total, so the functions it did recover are still returned.
        compiler: Optional explicit C preprocessor executable.
        timeout: Maximum preprocessing time in seconds.

    Returns:
        Graphs and the observable preprocessing report that produced them.

    Raises:
        ImportError: If `networkx` is not installed in the running environment.
    """
    preprocessing = preprocess_decompiled(text, compiler=compiler, timeout=timeout)
    serialized = parity_cfgs(preprocessing.text)
    graphs = {name: graph_from_serialized(cfg) for name, cfg in serialized.items()}
    provenance, diagnostics = _function_provenance(text, preprocessing, set(serialized))
    return DecompiledCfgAnalysis(
        graphs=graphs,
        preprocessing=preprocessing,
        provenance=provenance,
        diagnostics=diagnostics,
    )


def cfgs_from_decompiled(text: str) -> dict[str, networkx.DiGraph]:
    """GED-ready CFGs for every scoreable function in decompiled C.

    This compatibility entry point preserves the mapping expected by DecBench.
    New callers that need to enforce or record preprocessing outcomes should use
    :func:`analyze_decompiled`.

    Args:
        text: Decompiled C source text.

    Returns:
        ``{function name: DiGraph}`` whose nodes are :class:`SourceCfgNode`.

    Raises:
        ImportError: If `networkx` is not installed in the running environment.
    """
    return analyze_decompiled(text).graphs
