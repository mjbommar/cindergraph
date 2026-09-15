"""Generated native stubs must preserve the runtime calling convention."""

import ast
import inspect
from pathlib import Path

import cindergraph._native as native
import pytest


@pytest.mark.parametrize(
    "class_name,module", [("_Source", native.source), ("_CSource", native.csource)]
)
def test_stub_parameters_match_runtime(class_name: str, module: object) -> None:
    stub = Path(native.__file__).parent / "_native/__init__.pyi"
    tree = ast.parse(stub.read_text())
    cls = next(
        node
        for node in tree.body
        if isinstance(node, ast.ClassDef) and node.name == class_name
    )
    functions = {
        node.name: node for node in cls.body if isinstance(node, ast.FunctionDef)
    }
    for name, function in inspect.getmembers(module, inspect.isbuiltin):
        signature = inspect.signature(function)
        definition = functions[name]
        args = definition.args
        assert args.vararg is None and args.kwarg is None, name
        assert any(
            isinstance(d, ast.Name) and d.id == "staticmethod"
            for d in definition.decorator_list
        ), name
        names = [arg.arg for arg in args.posonlyargs + args.args + args.kwonlyargs]
        assert names == list(signature.parameters), name
        assert len(args.defaults) + sum(v is not None for v in args.kw_defaults) == sum(
            p.default is not inspect.Parameter.empty
            for p in signature.parameters.values()
        ), name
