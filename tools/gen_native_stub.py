"""Generate the minimal type surface of the built Cindergraph extension."""

from __future__ import annotations

import argparse
import inspect
from pathlib import Path

import cindergraph._native as native

TARGET = Path(__file__).parents[1] / "python/cindergraph/_native/__init__.pyi"


def render() -> str:
    lines = ["from typing import Any", ""]
    for public, module in (("Source", native.source), ("CSource", native.csource)):
        lines.append(f"class _{public}:")
        names = [name for name in dir(module) if not name.startswith("_")]
        for name in names:
            value = getattr(module, name)
            if inspect.isbuiltin(value):
                lines.append(
                    f"    def {name}(self, *args: Any, **kwargs: Any) -> Any: ..."
                )
        if not names:
            lines.append("    pass")
        lines.append("")
    lines.extend(["source: _Source", "csource: _CSource", ""])
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    check = parser.parse_args().check
    expected = render()
    if check:
        return 0 if TARGET.is_file() and TARGET.read_text() == expected else 1
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    TARGET.write_text(expected)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
