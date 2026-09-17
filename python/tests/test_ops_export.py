"""The typed-operations export, as a consumer reads it through the wheel.

Pins the ``repr="ops"`` schema in docs/reference/source-python.md: the
operation kinds, the explicit conversions, the value inputs, the CFG join,
the ``unknown`` contract and determinism.
"""

import json

import pytest

import cindergraph as cg

SOURCE = (
    "unsigned int total_len(unsigned int hdr, unsigned int body, unsigned int dst_len) {\n"
    "    unsigned int total = hdr + body;\n"
    "    if (total > dst_len) { return 1; }\n"
    "    return 0;\n"
    "}\n"
    "int checks(int len, unsigned long n, unsigned char bits, unsigned short s) {\n"
    "    s += 2;\n"
    "    if (len < n) { return 1u << bits; }\n"
    "    return n < 0 ? -len : len;\n"
    "}\n"
    "int casts(int n) { return (unsigned char) n; }\n"
)


def _ops(name: str) -> tuple[list[dict], list[dict]]:
    for function, body in cg.export_graphs(SOURCE, repr="ops", format="json"):
        if function == name:
            graph = json.loads(body)
            return graph["nodes"], graph["edges"]
    raise AssertionError(f"no function {name!r}")


def _shape(nodes: list[dict]) -> list[tuple[str, str, str, str]]:
    return [(n["kind"], n["op"], n["type"], n["inputs"]) for n in nodes]


@pytest.mark.core
def test_ops_is_an_export_representation() -> None:
    assert "ops" in cg.EXPORT_REPRS
    session = cg.AnalysisSession(SOURCE)
    names = [name for name, _ in session.export_graphs(repr="ops", format="json")]
    assert names == ["total_len", "checks", "casts"]


@pytest.mark.core
def test_the_consumer_length_check_reads_as_typed_operations() -> None:
    nodes, edges = _ops("total_len")
    assert _shape(nodes)[:8] == [
        ("load", "hdr", "unsigned int", ""),
        ("load", "body", "unsigned int", ""),
        ("binary", "+", "unsigned int", "0,1"),
        ("store", "=", "unsigned int", "2"),
        ("load", "total", "unsigned int", ""),
        ("load", "dst_len", "unsigned int", ""),
        ("compare", ">", "int", "4,5"),
        ("branch", "if", "void", "6"),
    ]
    assert nodes[6]["operand_type"] == "unsigned int"
    assert nodes[3]["name"] == "total" and nodes[3]["access"] == "scalar"
    # `return 1` converts the int literal to the unsigned int return type.
    assert _shape(nodes)[8:11] == [
        ("const", "1", "int", ""),
        ("convert", "assignment", "unsigned int", "8"),
        ("return", "return", "void", "9"),
    ]
    # Edges are the inputs relation, producer to consumer, in operand order.
    assert [(e["source"], e["target"], e["index"]) for e in edges][:4] == [
        (0, 2, "0"),
        (1, 2, "1"),
        (2, 3, "0"),
        (4, 6, "0"),
    ]


@pytest.mark.core
def test_every_conversion_kind_is_explicit() -> None:
    nodes, _ = _ops("checks")
    conversions = [
        (n["op"], n["from"], n["to"]) for n in nodes if n["kind"] == "convert"
    ]
    assert ("promotion", "unsigned short", "int") in conversions  # s += 2
    assert ("assignment", "int", "unsigned short") in conversions  # back into s
    assert ("usual_arithmetic", "int", "unsigned long") in conversions  # len < n
    assert ("promotion", "unsigned char", "int") in conversions  # shift count
    assert ("usual_arithmetic", "int", "unsigned long") in conversions  # n < 0
    add = next(n for n in nodes if n["kind"] == "binary" and n["op"] == "+")
    assert add["type"] == "int" and add["operand_type"] == "int"
    shift = next(n for n in nodes if n["op"] == "<<")
    assert shift["type"] == "unsigned int"


@pytest.mark.core
def test_conditional_arms_are_guarded_and_placed_in_their_own_blocks() -> None:
    nodes, _ = _ops("checks")
    select = next(n for n in nodes if n["kind"] == "select")
    condition, when_true, when_false = map(int, select["inputs"].split(","))
    assert nodes[condition]["kind"] == "compare"
    assert nodes[when_true]["guarded_by"] == f"{condition}:true"
    assert nodes[when_false]["guarded_by"] == f"{condition}:false"
    assert select["guarded_by"] == ""
    assert nodes[when_true]["block"] != nodes[when_false]["block"]
    # `block` joins onto the CFG export's node ids.
    cfg_body = dict(cg.export_graphs(SOURCE, repr="cfg", format="json"))["checks"]
    cfg_nodes = {n["id"]: n for n in json.loads(cfg_body)["nodes"]}
    for op in (select, nodes[when_true], nodes[when_false]):
        assert int(op["block"]) in cfg_nodes
    # The arms are expression-internal CFG nodes; the select is after them.
    assert cfg_nodes[int(nodes[when_true]["block"])]["expr_internal"] == "true"
    assert int(select["block"]) not in (
        int(nodes[when_true]["block"]),
        int(nodes[when_false]["block"]),
    )


@pytest.mark.core
def test_a_declined_root_is_an_unknown_with_its_reason_not_a_gap() -> None:
    nodes, _ = _ops("casts")
    assert _shape(nodes) == [("unknown", "return", "unknown", "")]
    assert nodes[0]["reason"] == "unsupported_form"
    lo, hi = map(int, nodes[0]["span"].split(":"))
    assert SOURCE.encode()[lo:hi] == b"(unsigned char) n"


@pytest.mark.core
def test_the_export_is_deterministic() -> None:
    first = cg.export_graphs(SOURCE, repr="ops", format="json")
    second = cg.export_graphs(SOURCE, repr="ops", format="json")
    assert first == second
    for format in cg.EXPORT_FORMATS:
        assert cg.export_graphs(SOURCE, repr="ops", format=format)
