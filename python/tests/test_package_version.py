"""``cindergraph.__version__`` is the crate version and the wheel's version."""

import importlib.metadata
import re
import tomllib
from pathlib import Path

import cindergraph

ROOT = Path(__file__).resolve().parents[2]


def test_version_is_a_pep440_string_from_the_workspace_crate_version() -> None:
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    crate_version = workspace["workspace"]["package"]["version"]
    assert cindergraph.__version__ == crate_version
    assert re.fullmatch(r"\d+\.\d+\.\d+([-+.][0-9A-Za-z.]+)?", cindergraph.__version__)
    assert "__version__" in cindergraph.__all__


def test_version_matches_the_installed_distribution_metadata() -> None:
    try:
        installed = importlib.metadata.version("cindergraph")
    except importlib.metadata.PackageNotFoundError:
        # A checkout on sys.path without an install has no metadata; the
        # attribute still exists because the extension carries it.
        assert cindergraph.__version__
        return
    assert cindergraph.__version__ == installed
