"""Run with a clean wheel environment's `python -I`, never the editable env."""

from importlib import metadata
import ast
import inspect
import json
from pathlib import Path
import sys
from xml.etree import ElementTree

import cindergraph as cg


def main() -> None:
    """Check installed extension identity, packaged metadata and core operations."""
    prefix = Path(sys.prefix).resolve()
    for module in (cg, cg._native):
        path = Path(module.__file__).resolve()
        if not path.is_relative_to(prefix):
            raise RuntimeError(f"Module loaded outside isolated environment: {path}")
    distribution = metadata.distribution("cindergraph")
    files = {str(path).replace("\\", "/") for path in distribution.files or []}
    for required in ("cindergraph/py.typed", "cindergraph/_native/__init__.pyi"):
        assert required in files, required
    for required in ("LICENSE", "NOTICE"):
        assert any(path.endswith("/licenses/" + required) for path in files), required
    stub = Path(cg.__file__).parent / "_native/__init__.pyi"
    tree = ast.parse(stub.read_text())
    classes = {node.name: node for node in tree.body if isinstance(node, ast.ClassDef)}
    for class_name, module in (
        ("_Source", cg._native.source),
        ("_CSource", cg._native.csource),
    ):
        definitions = {
            node.name: node
            for node in classes[class_name].body
            if isinstance(node, ast.FunctionDef)
        }
        runtime_functions = dict(inspect.getmembers(module, inspect.isbuiltin))
        assert set(definitions) == set(runtime_functions), class_name
        for name, function in runtime_functions.items():
            definition = definitions[name]
            assert definition.args.vararg is None
            assert definition.args.kwarg is None
            stub_names = [
                argument.arg
                for argument in (
                    definition.args.posonlyargs
                    + definition.args.args
                    + definition.args.kwonlyargs
                )
            ]
            assert stub_names == list(inspect.signature(function).parameters), name

    code = "int f(int x){int y=0;int *p=&y;*p=x;return y;}"
    report = cg.analyze(code)
    assert not report.diagnostics
    assert report.functions[0].name == "f"
    flow = cg.data_flow(code)[0]
    assert flow["recovery_free"] and flow["memory_complete"]
    assert any(item["kind"] == "memory_write" for item in flow["definitions"])
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and summary["flows"]
    parenthesized_store = "int f(int x){int y=0;int *p=&y;(((*p)))=x;return y;}"
    parenthesized_summary = cg.call_summaries(parenthesized_store)[0]
    assert parenthesized_summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return"
        for item in parenthesized_summary["flows"]
    )
    pointer_arithmetic = "int f(void){int x=0;int *p=&x;p=p+1;*p=7;return x;}"
    assert not cg.data_flow(pointer_arithmetic)[0]["memory_complete"]
    assert not cg.call_summaries(pointer_arithmetic)[0]["complete"]
    condition_arithmetic = "int f(int x){int a=0,b=0;int *p=(x+1)?&a:&b;*p=x;return a;}"
    assert cg.data_flow(condition_arithmetic)[0]["memory_complete"]
    assert cg.call_summaries(condition_arithmetic)[0]["complete"]
    escaped = cg.data_flow("int f(void){int x=1;(void)&x;return 0;}")[0]
    assert any(item["kind"] == "address_taken" for item in escaped["definitions"])
    assert not escaped["uses"] and not escaped["edges"]
    assert not escaped["dead_stores"] and not escaped["unused_bindings"]
    maybe_uninitialized = "int f(int x){int y=0;int *p;if(x)p=&y;*p=x;return y;}"
    assert not cg.data_flow(maybe_uninitialized)[0]["memory_complete"]
    assert not cg.call_summaries(maybe_uninitialized)[0]["complete"]
    definitely_initialized = (
        "int f(int x){int y=0;int *p;if(x)p=&y;else p=&y;*p=x;return y;}"
    )
    assert cg.data_flow(definitely_initialized)[0]["memory_complete"]
    assert cg.call_summaries(definitely_initialized)[0]["complete"]
    discarded_assignment = "int f(int x){int y=0;int *p;(p=&y,0);*p=x;return y;}"
    assert cg.data_flow(discarded_assignment)[0]["memory_complete"]
    assert cg.call_summaries(discarded_assignment)[0]["complete"]
    discarded_arithmetic = (
        "int f(int x){int y=0;int *q=&y;int *p;((p=q+1,1),0);*p=x;return y;}"
    )
    assert not cg.data_flow(discarded_arithmetic)[0]["memory_complete"]
    assert not cg.call_summaries(discarded_arithmetic)[0]["complete"]
    second_order = "int f(int x){int a=0,b=0;int *p=&a;int **q=&p;*q=&b;*p=x;return b;}"
    assert cg.data_flow(second_order)[0]["memory_complete"]
    second_order_summary = cg.call_summaries(second_order)[0]
    assert second_order_summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return"
        for item in second_order_summary["flows"]
    )
    second_order_load = (
        "int f(int x){int a=0,b=0;int *p=&a;int **q=&p;*q=&b;int *r=*q;*r=x;return b;}"
    )
    second_order_load_summary = cg.call_summaries(second_order_load)[0]
    assert second_order_load_summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return"
        for item in second_order_load_summary["flows"]
    )
    known_pointer_load = (
        "int f(int x){int a=0;int *p=&a;int **q=&p;int *r=*q;*r=x;return a;}"
    )
    known_pointer_load_summary = cg.call_summaries(known_pointer_load)[0]
    assert known_pointer_load_summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return"
        for item in known_pointer_load_summary["flows"]
    )
    integer_pointer = "int f(int x){int a=0;int *p=x?&a:(int*)1;*p=x;return a;}"
    assert not cg.data_flow(integer_pointer)[0]["memory_complete"]
    assert not cg.call_summaries(integer_pointer)[0]["complete"]
    mixed_scalar_pointer = "int f(int x){int a=0;int *p=(int*)(x?&a:x);*p=x;return a;}"
    assert not cg.data_flow(mixed_scalar_pointer)[0]["memory_complete"]
    assert not cg.call_summaries(mixed_scalar_pointer)[0]["complete"]
    assert cg.reaches("int f(int x){return 0;", "f", 0, "sink") == "unknown"
    graphs = cg.export_graphs(code, repr="pdg", format="json")
    assert graphs[0][0] == "f"
    assert json.loads(graphs[0][1])["directed"]
    # Cover recent semantic fixes in the artifact, not merely editable imports.
    unevaluated = "int sink(int x){return x;} int f(int x){return sizeof(sink(x));}"
    assert cg.reaches(unevaluated, "f", 0, "sink") == "no"
    comma = "int f(int x){return (x,0);}"
    assert not cg.call_summaries(comma)[0]["flows"]
    global_read = "int global; int f(void){return global;}"
    assert not cg.call_summaries(global_read)[0]["complete"]
    duplicate = "int f(int x){return x;} int f(void){return 0;}"
    assert cg.reaches(duplicate, "f", 0, "f") == "unknown"
    try:
        cg.backward_slice(duplicate, "f", 0)
    except ValueError:
        pass
    else:
        raise AssertionError("ambiguous slice selected a body")
    report = cg.analyze(duplicate)
    try:
        cg.compare(report, report)
    except ValueError:
        pass
    else:
        raise AssertionError("comparison discarded duplicate definitions")
    # Parse XML rather than just accepting that a writer returned a string.
    for _, document in cg.export_graphs(code, repr="pdg", format="graphml"):
        ElementTree.fromstring(document)
    print(f"Installed cindergraph {distribution.version}: {cg._native.__file__}")


if __name__ == "__main__":
    main()
