"""The defect conformance corpus: what a semantic consumer must be able to see.

Item 15 of docs/improvement-list-2026-09-16.md. ``tests/fixtures/defects/``
holds the twelve annotated defect/fix samples Axeyum's solver front end
consumes (copied verbatim) plus one loop sample written for this corpus; the
README there says where they came from and why the loop sample exists. For
every function in every sample this asserts, over one ``AnalysisSession``'s
``ast``, ``cfg`` and ``ops`` exports, the facts that consumer reads instead
of re-deriving from source bytes: operators, resolved types, statement-level
conditions and loop bounds. A regression in any of them is invisible to the
metrics tests and visible to the consumer only as a wrong verdict.

The samples include ``<stddef.h>``/``<stdint.h>``; the typedefs are prepended
here (LP64) and the text is analysed with the ``ordinary`` dialect, because
the ``preprocessed`` dialect keeps only marker-attributed project lines and
drops marker-less text entirely (see the README).
"""

import json
import re
from pathlib import Path

import cindergraph as cg
import pytest

ROOT = Path(__file__).resolve().parents[2] / "tests/fixtures/defects"
FILES = sorted(ROOT.glob("*.c"))
PREFIX = "typedef unsigned long size_t;\ntypedef unsigned int uint32_t;\n"
EXPECT = re.compile(r"^// expect: (\w+) (finding|clean)$", re.MULTILINE)
LOOP_KIND = re.compile(r"/\* bound_kind: (\w+)")

# What each loop-bearing sample's headers must say, keyed by file prefix and
# header text: (bound_kind, induction, step, bound_value). 09 and 10 are the
# consumer's own loops; 13's are the comments beside them.
LOOP_HEADERS = {
    "09": {
        "i <= n": ("parameter", "i", "+1", None),
        "i < n": ("parameter", "i", "+1", None),
    },
    "10": {"i < 8": ("constant", "i", "+1", "8")},
    "13": {
        "i < 16": ("constant", "i", "+1", "16"),
        "u > 0": ("runtime", "u", "-1", None),
        "i <= n": ("parameter", "i", "+2", None),
        "i < limit": ("runtime", "i", "+1", None),
    },
}
LOOPS_PER_FILE = {"09": 2, "10": 2, "13": 5}


def _span(node: dict) -> tuple[int, int]:
    lo, hi = node["span"].split(":")
    return int(lo), int(hi)


class _Ast:
    def __init__(self, raw: bytes, graph: dict) -> None:
        self.raw = raw
        self.nodes = {node["id"]: node for node in graph["nodes"]}
        self.children: dict[int, list[int]] = {}
        for edge in graph["edges"]:
            self.children.setdefault(edge["source"], []).append(edge["target"])

    def text(self, node: dict) -> str:
        lo, hi = _span(node)
        return self.raw[lo:hi].decode("utf-8")

    def tagged(self, tag: str) -> list[dict]:
        return [node for node in self.nodes.values() if node["tag"] == tag]

    def kids(self, node: dict) -> list[dict]:
        return [self.nodes[i] for i in self.children.get(node["id"], [])]


def _header_spans(ast: _Ast, loop: dict) -> set[tuple[int, int]]:
    """The spans a CFG `loop_header` for `loop` may carry."""
    spans = {_span(loop)}
    for child in ast.kids(loop):
        if child["tag"] == "for_cond":
            spans.update(_span(grandchild) for grandchild in ast.kids(child))
        elif loop["tag"] != "for_stmt" and not child["tag"].endswith("_stmt"):
            # A `while`/`do` condition: the one child that is not the body.
            spans.add(_span(child))
    return spans


def test_corpus_is_present_and_annotated() -> None:
    assert [path.name[:2] for path in FILES] == [f"{i:02d}" for i in range(1, 14)]
    for path in FILES:
        assert EXPECT.search(path.read_text()), path.name


def _functions(path: Path) -> list[str]:
    return [name for name, _ in EXPECT.findall(path.read_text())]


@pytest.mark.parametrize("path", FILES, ids=lambda path: path.name)
def test_every_function_exports_what_a_semantic_consumer_needs(path: Path) -> None:
    text = PREFIX + path.read_text()
    raw = text.encode("utf-8")
    session = cg.AnalysisSession(text)
    assert not session.diagnostics, [str(d) for d in session.diagnostics]
    asts = dict(session.export_graphs(repr="ast", format="json"))
    cfgs = dict(session.export_graphs(repr="cfg", format="json"))
    opss = dict(session.export_graphs(repr="ops", format="json"))
    expected = _functions(path)
    assert sorted(asts) == sorted(expected) == sorted(cfgs) == sorted(opss)

    seen = {"op": 0, "typed_ref": 0, "if": 0, "loop": 0, "param": 0, "ops": 0}
    for name in expected:
        ast = _Ast(raw, json.loads(asts[name]))
        cfg = json.loads(cfgs[name])["nodes"]
        ops = json.loads(opss[name])["nodes"]

        # Every operator-bearing node says which operator it is.
        for tag in ("binary_expr", "assign_expr", "unary_expr"):
            for node in ast.tagged(tag):
                assert node.get("op"), (name, tag, ast.text(node))
                seen["op"] += 1

        # Every parameter is structured, not a label to regex.
        declared: set[str] = set()
        for node in ast.tagged("param_decl"):
            assert {"type", "pointer_depth", "name"} <= node.keys(), (name, node)
            assert "unknown" not in node["type"], (name, node)
            declared.add(node["name"])
            seen["param"] += 1
        for node in ast.tagged("declarator"):
            if "name" in node and node["name"] != name:
                declared.add(node["name"])

        # Every reference to a parameter or local has a resolved type, which
        # is the whole point of prepending the typedefs the file includes.
        for node in ast.tagged("name_ref"):
            if ast.text(node) in declared:
                assert "unknown" not in node["type"], (name, ast.text(node), node)
                seen["typed_ref"] += 1

        # Every `if` condition is a statement-level CFG `cond` node.
        by_span = {}
        for node in cfg:
            by_span.setdefault(_span(node), []).append(node)
        for node in ast.tagged("if_stmt"):
            cond = ast.kids(node)[0]
            assert cond["tag"] not in ("compound_stmt", "expr_stmt"), (name, node)
            matches = [
                n
                for n in by_span.get(_span(cond), [])
                if n["kind"] == "cond" and n["expr_internal"] == "false"
            ]
            assert matches, (name, ast.text(cond))
            seen["if"] += 1

        # Every loop has a classified header, on the AST and the CFG alike.
        headers = [n for n in cfg if n["kind"] == "loop_header"]
        loops = [
            n
            for tag in ("for_stmt", "while_stmt", "do_while_stmt")
            for n in ast.tagged(tag)
        ]
        assert len(headers) == len(loops), (name, headers, loops)
        for header in headers:
            assert header["bound_kind"] in {"constant", "parameter", "runtime", "none"}
            assert header["loop_kind"] in {"for", "while", "do_while"}
            # The header's span is the loop's condition (or the whole loop
            # when it has none); that is how a consumer joins the two.
            twins = [n for n in loops if _span(header) in _header_spans(ast, n)]
            assert len(twins) == 1, (name, header, loops)
            keys = (
                "loop_kind",
                "bound_kind",
                "bound_expr",
                "induction",
                "step",
                "bound_value",
            )
            assert {k: header.get(k) for k in keys} == {
                k: twins[0].get(k) for k in keys
            }
            seen["loop"] += 1
            if header["bound_kind"] == "none":
                assert "bound_expr" not in header and "induction" not in header
                continue
            promised = LOOP_HEADERS[path.name[:2]][header["bound_expr"]]
            kind, induction, step, value = promised
            assert header["bound_kind"] == kind, header
            assert header["induction"] == induction, header
            assert header["step"] == step, header
            assert header.get("bound_value") == value, header

        # Every operation is typed, and an `unknown` names a documented reason.
        for op in ops:
            assert op["type"], (name, op)
            if op["kind"] == "unknown":
                assert op["reason"] in {
                    "unsupported_form",
                    "unsequenced_effects",
                    "braced_initializer",
                    "unplaced",
                }, (name, op)
            seen["ops"] += 1

    # The assertions above are only evidence if they ran over something.
    assert seen["op"] and seen["typed_ref"] and seen["param"] and seen["ops"], seen
    assert seen["if"], seen
    assert seen["loop"] == LOOPS_PER_FILE.get(path.name[:2], 0), seen


def test_the_loop_sample_comments_and_the_export_agree() -> None:
    # The `/* bound_kind: ... */` comments in 13 are documentation; this
    # keeps them honest against the export, in source order.
    path = ROOT / "13_loop_bounds.c"
    text = PREFIX + path.read_text()
    promised = LOOP_KIND.findall(text)
    session = cg.AnalysisSession(text)
    exported = []
    for _, body in session.export_graphs(repr="cfg", format="json"):
        for node in json.loads(body)["nodes"]:
            if node["kind"] == "loop_header":
                exported.append((int(node["line"]), node["bound_kind"]))
    assert (
        [kind for _, kind in sorted(exported)]
        == promised
        == [
            "constant",
            "runtime",
            "parameter",
            "runtime",
            "none",
        ]
    )
