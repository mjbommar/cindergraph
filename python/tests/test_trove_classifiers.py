"""Unit tests for fail-closed Trove classifier validation."""

from pathlib import Path
import importlib.util

import pytest


ROOT = Path(__file__).resolve().parents[2]


def _checker():
    path = ROOT / "tools" / "check_trove_classifiers.py"
    spec = importlib.util.spec_from_file_location("check_trove_classifiers", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_classifier_validator_accepts_distinct_known_values() -> None:
    _checker().validate_classifiers(["A", "B"], {"A", "B", "C"})


def test_classifier_validator_rejects_unknown_values() -> None:
    with pytest.raises(ValueError, match="unknown Trove classifiers.*Unknown"):
        _checker().validate_classifiers(["Known", "Unknown"], {"Known"})


def test_classifier_validator_rejects_duplicates() -> None:
    with pytest.raises(ValueError, match="duplicate Trove classifiers.*Known"):
        _checker().validate_classifiers(["Known", "Known"], {"Known"})
