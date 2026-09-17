"""Generate native calling signatures; value/result schemas remain Any."""

from __future__ import annotations

import argparse
import inspect
import json
from pathlib import Path
from typing import Any

import cindergraph._native as native

TARGET = Path(__file__).parents[1] / "python/cindergraph/_native/__init__.pyi"


def append_definition(
    lines: list[str], indent: str, name: str, signature: object
) -> None:
    """Append one definition, wrapping generated signatures deterministically."""
    rendered = str(signature)
    if isinstance(signature, inspect.Signature):
        for parameter in signature.parameters.values():
            if isinstance(parameter.default, str):
                rendered = rendered.replace(
                    f"={parameter.default!r}", f"={json.dumps(parameter.default)}"
                )
                rendered = rendered.replace(
                    f"= {parameter.default!r}", f"= {json.dumps(parameter.default)}"
                )
    declaration = f"{indent}def {name}{rendered}: ..."
    if len(declaration) <= 88:
        lines.append(declaration)
        return

    parameters, returns = rendered.rsplit(")", 1)
    parts = [part.strip() for part in parameters[1:].split(",")]
    lines.append(f"{indent}def {name}(")
    lines.extend(f"{indent}    {part}," for part in parts)
    lines.append(f"{indent}){returns}: ...")


def render() -> str:
    lines = ["from typing import Any", ""]
    for public, module in (("Source", native.source), ("CSource", native.csource)):
        names = [name for name in dir(module) if not name.startswith("_")]
        classes = [
            (name, getattr(module, name))
            for name in names
            if inspect.isclass(getattr(module, name))
        ]
        for name, value in classes:
            class_name = f"_{public}{name}"
            lines.append(f"class {class_name}:")
            # PyO3 classes without `#[new]` report an empty inspect signature
            # but reject construction. Do not generate a lying public
            # `__init__()` for result objects users can only receive from Rust.
            if value.__text_signature__ is not None:
                constructor = inspect.signature(value).replace(
                    parameters=[
                        parameter.replace(annotation=Any)
                        for parameter in inspect.signature(value).parameters.values()
                    ],
                    return_annotation=None,
                )
                constructor_text = str(constructor)
                constructor_text = (
                    "(self) -> None"
                    if constructor_text == "() -> None"
                    else constructor_text.replace("(", "(self, ", 1)
                )
                append_definition(lines, "    ", "__init__", constructor_text)
            for member_name in (
                item for item in dir(value) if not item.startswith("_")
            ):
                member = getattr(value, member_name)
                if inspect.ismethoddescriptor(member):
                    signature = inspect.signature(member).replace(return_annotation=Any)
                    append_definition(lines, "    ", member_name, signature)
                elif inspect.isgetsetdescriptor(member):
                    lines.append("    @property")
                    lines.append(f"    def {member_name}(self) -> Any: ...")
            lines.append("")
        lines.append(f"class _{public}:")
        for name in names:
            value = getattr(module, name)
            if inspect.isbuiltin(value):
                # Fail if signature metadata is unavailable rather than silently
                # restoring an unrestricted *args/**kwargs declaration.
                signature = inspect.signature(value)
                signature = signature.replace(
                    parameters=[
                        parameter.replace(annotation=Any)
                        for parameter in signature.parameters.values()
                    ],
                    return_annotation=Any,
                )
                lines.append("    @staticmethod")
                append_definition(lines, "    ", name, signature)
            elif inspect.isclass(value):
                lines.append(f"    {name}: type[_{public}{name}]")
        if not names:
            lines.append("    pass")
        lines.append("")
    lines.extend(["__version__: str", "source: _Source", "csource: _CSource", ""])
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
