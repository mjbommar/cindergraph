"""Tolerant, deterministic C source analysis."""

from cindergraph import _native, source, source_cfg
from cindergraph.source import *
from cindergraph.source import EXPORT_FORMATS as EXPORT_FORMATS
from cindergraph.source import EXPORT_REPRS as EXPORT_REPRS

#: The version of the Rust crate this extension was built from. It is the same
#: string the wheel's metadata carries; a consumer records it beside a result.
__version__: str = _native.__version__

__all__ = [*source.__all__, "__version__", "_native", "source", "source_cfg"]
