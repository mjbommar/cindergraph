"""The documented export schema, as a consumer reads it through the wheel.

Pins the 2026-09-16 additions (operator, type, declarator and parameter
fields, line/column, ``expr_internal``), that spans are byte offsets, and the
ordering contract in docs/reference/source-python.md.
"""

import json

import cindergraph as cg

SOURCE = (
    "/* an em dash — shifts every later byte offset by two */\n"
    "typedef unsigned long size_t;\n"
    "int f(unsigned char *dst, size_t n, unsigned short s) {\n"
    "    unsigned int table[16];\n"
    "    int idx = n < 0 ? -n : n;\n"
    "    if (idx <= 16 && n != 0) { table[idx] = (s + 1) << 2u; }\n"
    "    return dst /* x */ + idx;\n"
    "}\n"
)
RAW = SOURCE.encode("utf-8")


def _graph(repr: str) -> dict:
    (name, body), *_ = cg.export_graphs(SOURCE, repr=repr, format="json")
    assert name == "f"
    return json.loads(body)


def _text(node: dict) -> str:
    lo, hi = map(int, node["span"].split(":"))
    return RAW[lo:hi].decode("utf-8")


def _by_text(graph: dict, tag: str, text: str) -> dict:
    for node in graph["nodes"]:
        if node.get("tag") == tag and _text(node) == text:
            return node
    raise AssertionError(f"no {tag} node covering {text!r}")


def test_spans_are_byte_offsets_and_line_column_agree_with_them() -> None:
    ast = _graph("ast")
    plus = _by_text(ast, "binary_expr", "dst /* x */ + idx")
    lo = int(plus["span"].split(":")[0])
    assert int(plus["line"]) == RAW.count(b"\n", 0, lo) + 1 == 7
    assert int(plus["column"]) == lo - RAW.rfind(b"\n", 0, lo) == 12
    # Indexing the decoded string with the same offsets is off by the em
    # dash's extra bytes, which is exactly the bug the byte contract prevents.
    assert SOURCE[lo:] != RAW[lo:].decode("utf-8")


def test_operator_type_declarator_and_parameter_attributes_are_present() -> None:
    ast = _graph("ast")
    plus = _by_text(ast, "binary_expr", "dst /* x */ + idx")
    assert plus["op"] == "+"
    assert plus["type"] == "unsigned char *"
    less = _by_text(ast, "binary_expr", "n < 0")
    assert (less["op"], less["type"], less["operand_type"]) == (
        "<",
        "int",
        "unsigned long",
    )
    assert _by_text(ast, "cond_expr", "n < 0 ? -n : n")["op"] == "?:"
    assert _by_text(ast, "unary_expr", "-n")["op"] == "-"
    shift = _by_text(ast, "binary_expr", "(s + 1) << 2u")
    assert (shift["op"], shift["type"], shift["operand_type"]) == ("<<", "int", "int")
    assert _by_text(ast, "literal", "2u")["type"] == "unsigned int"
    table = _by_text(ast, "declarator", "table[16]")
    assert (table["name"], table["element_type"], table["count"]) == (
        "table",
        "unsigned int",
        "16",
    )
    dst = _by_text(ast, "param_decl", "unsigned char *dst")
    assert (dst["name"], dst["type"], dst["pointer_depth"]) == (
        "dst",
        "unsigned char *",
        "1",
    )
    assert _by_text(ast, "param_decl", "size_t n")["type"] == "unsigned long"
    assert _by_text(ast, "binary_expr", "idx <= 16 && n != 0")["type"] == "int"
    assert "operand_type" not in _by_text(ast, "binary_expr", "idx <= 16 && n != 0")


def test_unknown_is_spelled_out_rather_than_guessed() -> None:
    (_, body), *_ = cg.export_graphs(
        "int g(uint32_t x) { return h(x) + x; }", repr="ast", format="json"
    )
    ast = json.loads(body)
    types = {n["label"].split("\n")[-1]: n["type"] for n in ast["nodes"] if "type" in n}
    assert types["x"] == "unknown"


def test_expression_internal_cfg_nodes_are_marked() -> None:
    cfg = _graph("cfg")
    flagged = {_text(n): n["expr_internal"] for n in cfg["nodes"]}
    assert flagged["idx <= 16"] == "true"
    assert flagged["n != 0"] == "true"
    assert flagged["idx <= 16 && n != 0"] == "false"
    assert flagged["n < 0"] == "true"
    assert flagged["-n"] == "true"
    assert flagged["int idx = n < 0 ? -n : n;"] == "false"
    assert all("expr_internal" in n for n in cfg["nodes"])
    for repr in ("cdg", "pdg"):
        other = _graph(repr)
        assert [n["expr_internal"] for n in other["nodes"]] == [
            n["expr_internal"] for n in cfg["nodes"]
        ]


def test_ast_edges_are_grouped_by_parent_with_children_in_source_order() -> None:
    ast = _graph("ast")
    nodes = {n["id"]: n for n in ast["nodes"]}
    ids = [n["id"] for n in ast["nodes"]]
    assert ids == sorted(ids) == list(range(len(ids)))
    starts = [int(n["span"].split(":")[0]) for n in ast["nodes"]]
    assert starts == sorted(starts)
    sources = [e["source"] for e in ast["edges"]]
    assert sources == sorted(sources)
    children: dict[int, list[int]] = {}
    for e in ast["edges"]:
        assert e["source"] < e["target"]
        children.setdefault(e["source"], []).append(e["target"])
    for parent, kids in children.items():
        spans = [tuple(map(int, nodes[k]["span"].split(":"))) for k in kids]
        assert spans == sorted(spans), (parent, spans)
        assert all(a[1] <= b[0] for a, b in zip(spans, spans[1:])), (parent, spans)
    chain = _by_text(ast, "binary_expr", "dst /* x */ + idx")
    assert [_text(nodes[k]) for k in children[chain["id"]]] == ["dst", "idx"]
