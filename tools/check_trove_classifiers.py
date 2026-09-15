#!/usr/bin/env python3
"""Validate project classifiers against the current PyPI classifier registry."""

from pathlib import Path
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def validate_classifiers(declared: list[str], known: set[str]) -> None:
    """Reject duplicate or unregistered Trove classifiers."""
    duplicates = sorted({value for value in declared if declared.count(value) > 1})
    if duplicates:
        raise ValueError(f"duplicate Trove classifiers: {duplicates!r}")

    unknown = sorted(set(declared) - known)
    if unknown:
        raise ValueError(f"unknown Trove classifiers: {unknown!r}")


def main() -> None:
    """Load pyproject.toml and validate its declared classifiers."""
    from trove_classifiers import classifiers

    document = tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))
    declared = document["project"]["classifiers"]
    validate_classifiers(declared, set(classifiers))
    print(f"Valid Trove classifiers: {len(declared)}")


if __name__ == "__main__":
    main()
