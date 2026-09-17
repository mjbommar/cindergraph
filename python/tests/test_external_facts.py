"""External facts: the consumer contract of item 7.

``docs/design/external-facts-2026-09-17.md`` decides the form: one grammar,
two front doors (a ``// @cindergraph`` comment above the function, with the
consumer's ``// axeyum:`` marker as an alias, or a ``facts=`` argument), one
landing place (``facts``/``facts_source`` on ``param_decl`` and ``func_def``
in the ``ast`` export and on the parameter's loads and stores in ``ops``), and
a diagnostic for every fact that cannot be attached. This file pins each of
those, and walks every vendored defect sample asserting that every
``// axeyum:`` line it carries lands on the node it names.
"""

import json
import re
from pathlib import Path

import cindergraph as cg
import pytest

DEFECTS = Path(__file__).resolve().parents[2] / "tests/fixtures/defects"
SAMPLES = sorted(DEFECTS.glob("*.c"))
AXEYUM_LINE = re.compile(r"^// axeyum: (\w+)(?:\((\w+)\))? = (\w+)$", re.MULTILINE)

SOURCE = """\
#include <string.h>
// @cindergraph capacity(dst) = dst_len
// axeyum: capacity(src) = 256
// @cindergraph: strlen(src) = n
// @cindergraph unroll = 4
int copy(char *dst, unsigned long dst_len, const char *src, unsigned long n) {
    unsigned long i;
    for (i = 0; i < n; i++) dst[i] = src[i];
    return 0;
}
"""


def _nodes(session: cg.AnalysisSession, repr: str, function: str) -> list[dict]:
    exported = dict(session.export_graphs(repr=repr, format="json"))
    return json.loads(exported[function])["nodes"]


def _facts(nodes: list[dict]) -> list[tuple[str, str | None, str, str]]:
    """``(tag or kind, name, facts, facts_source)`` of every node with facts."""
    return [
        (
            node.get("tag") or node["kind"],
            node.get("name"),
            node["facts"],
            node["facts_source"],
        )
        for node in nodes
        if "facts" in node
    ]


def test_both_markers_all_three_kinds_and_both_value_forms_land_on_the_ast():
    session = cg.AnalysisSession(SOURCE)
    assert session.diagnostics == ()
    assert _facts(_nodes(session, "ast", "copy")) == [
        ("func_def", None, "unroll=4", "comment"),
        ("param_decl", "dst", "capacity=dst_len", "comment"),
        ("param_decl", "src", "capacity=256,strlen=n", "comment,comment"),
    ]


def test_ops_loads_of_a_parameter_with_facts_carry_them_and_nothing_else_does():
    session = cg.AnalysisSession(SOURCE)
    nodes = _nodes(session, "ops", "copy")
    with_facts = _facts(nodes)
    assert with_facts, "no ops node carries facts"
    assert all(kind == "load" for kind, _, _, _ in with_facts)
    assert {name for _, name, _, _ in with_facts} == {"dst", "src"}
    assert {(name, facts) for _, name, facts, _ in with_facts} == {
        ("dst", "capacity=dst_len"),
        ("src", "capacity=256,strlen=n"),
    }
    for node in nodes:
        if node["kind"] in {"load", "store"} and node.get("name") in {"i", "n"}:
            assert "facts" not in node, node
    # The element store `dst[i] = ...` is a projection over the loaded
    # pointer: its inputs lead to the load that carries the obligation.
    (store,) = [n for n in nodes if n["kind"] == "store" and n["access"] == "element"]
    dst_loads = {
        n["id"] for n in nodes if n["kind"] == "load" and n.get("name") == "dst"
    }
    assert dst_loads & {int(i) for i in store["inputs"].split(",")}


def test_the_export_without_facts_has_no_facts_attributes():
    plain = "int f(char *p, unsigned long n) { return p[n - 1]; }\n"
    for repr in cg.EXPORT_REPRS:
        for _, body in cg.export_graphs(plain, repr=repr, format="json"):
            for node in json.loads(body)["nodes"]:
                assert "facts" not in node and "facts_source" not in node


def test_the_api_form_lands_the_same_way_and_wins_over_a_comment():
    facts = {"copy": {"capacity": {"dst": "n"}, "strlen": {"dst": 4}, "unroll": "9"}}
    session = cg.AnalysisSession(SOURCE, facts=facts)
    assert session.diagnostics == ()
    assert _facts(_nodes(session, "ast", "copy")) == [
        ("func_def", None, "unroll=9", "api"),
        ("param_decl", "dst", "capacity=n,strlen=4", "api,api"),
        ("param_decl", "src", "capacity=256,strlen=n", "comment,comment"),
    ]
    by_name = {
        name: facts for _, name, facts, _ in _facts(_nodes(session, "ops", "copy"))
    }
    assert by_name == {"dst": "capacity=n,strlen=4", "src": "capacity=256,strlen=n"}


def test_the_free_functions_accept_facts_and_match_the_session():
    facts = {"copy": {"capacity": {"dst": 64}}}
    session = cg.AnalysisSession(SOURCE, facts=facts)
    for repr in ("ast", "ops"):
        assert cg.export_graphs(SOURCE, repr=repr, format="json", facts=facts) == (
            session.export_graphs(repr=repr, format="json")
        )
    (graph,) = cg.native_graphs(SOURCE, repr="ast", facts=facts)
    (dst,) = [
        node["attributes"]
        for node in graph.nodes()
        if node["attributes"].get("name") == "dst"
        and node["attributes"]["tag"] == "param_decl"
    ]
    assert (dst["facts"], dst["facts_source"]) == ("capacity=64", "api")


def test_export_path_accepts_facts(tmp_path):
    path = tmp_path / "copy.c"
    path.write_text(SOURCE, encoding="utf-8")
    facts = {"copy": {"unroll": 2}}
    assert cg.export_path(path, repr="ast", format="json", facts=facts) == (
        cg.export_graphs(SOURCE, repr="ast", format="json", facts=facts)
    )


UNATTACHABLE = """\
// @cindergraph capacity(dsx) = n
// @cindergraph capacity(dst) = m
// @cindergraph capacity(n) = 4
// @cindergraph capacity(dst) = src
// @cindergraph unroll = n
// @cindergraph size(dst) = 4
// @cindergraph strlen(src) = 1
// @cindergraph strlen(src) = 2
int f(char *dst, const char *src, unsigned long n) { return 0; }
// @cindergraph unroll = 2
int g(void);
"""


def test_every_fact_that_cannot_be_attached_is_a_diagnostic_on_the_session():
    session = cg.AnalysisSession(UNATTACHABLE)
    messages = [d.message for d in session.diagnostics]
    expected = [
        "capacity(dsx): `dsx` is not a parameter of `f`",
        "capacity(dst): `m` is neither a parameter of `f` nor a decimal literal",
        "capacity(n): `n` is not a pointer parameter of `f`",
        "capacity(dst): `src` is a pointer parameter, not a scalar",
        "unroll: the bound must be a decimal literal, found `n`",
        'malformed fact comment: unknown fact kind "size"',
        "duplicate fact `strlen(src)` for `f`; the first one stays",
        "fact comment is not followed by a function definition",
    ]
    for needle in expected:
        assert any(needle in m for m in messages), (needle, messages)
    assert len(messages) == len(expected)
    # The diagnostic points at the comment line.
    dsx = next(d for d in session.diagnostics if "dsx" in d.message)
    assert session.source[dsx.start : dsx.end] == "// @cindergraph capacity(dsx) = n"
    assert dsx.severity == "error"
    # Only the valid fact survives.
    assert _facts(_nodes(session, "ast", "f")) == [
        ("param_decl", "src", "strlen=1", "comment"),
    ]


def test_an_api_fact_that_cannot_be_attached_is_a_diagnostic_on_a_session():
    source = "int f(char *p, unsigned long n) { return p[0]; }\n"
    session = cg.AnalysisSession(
        source, facts={"f": {"capacity": {"q": "n"}}, "h": {"unroll": 1}}
    )
    messages = [d.message for d in session.diagnostics]
    assert any("capacity(q): `q` is not a parameter of `f`" in m for m in messages)
    assert any(
        "facts name function `h`, which the source does not define" in m
        for m in messages
    )
    assert len(messages) == 2
    # The diagnostic for a function-level problem points at the name.
    q = next(d for d in session.diagnostics if "`q`" in d.message)
    assert session.source[q.start : q.end] == "f"
    assert _facts(_nodes(session, "ast", "f")) == []


def test_an_api_fact_that_cannot_be_attached_raises_from_a_free_function():
    source = "int f(char *p, unsigned long n) { return p[0]; }\n"
    with pytest.raises(ValueError, match="`q` is not a parameter of `f`"):
        cg.export_graphs(
            source, repr="ast", format="json", facts={"f": {"capacity": {"q": 1}}}
        )
    with pytest.raises(ValueError, match="does not define"):
        cg.native_graphs(source, repr="ast", facts={"h": {"unroll": 1}})


def test_a_duplicate_definition_refuses_api_facts_by_name():
    source = "int f(char *p) { return 0; }\nint f(char *p) { return 1; }\n"
    session = cg.AnalysisSession(source, facts={"f": {"capacity": {"p": 8}}})
    assert any("defines 2 times" in d.message for d in session.diagnostics)
    for _, body in session.export_graphs(repr="ast", format="json"):
        assert not _facts(json.loads(body)["nodes"])


@pytest.mark.parametrize(
    "facts, match",
    [
        ("x", "must be a dict"),
        ({"f": "x"}, "must be a dict"),
        ({"f": {"size": 1}}, "unknown fact kind"),
        ({"f": {"capacity": 1}}, "must be a dict of"),
        ({"f": {"capacity": {"p": 1.5}}}, "parameter name .* or an integer"),
        ({"f": {"unroll": -1}}, "non-negative integer"),
        ({"f": {"unroll": None}}, "parameter name .* or an integer"),
    ],
)
def test_a_malformed_facts_argument_is_a_value_error(facts, match):
    source = "int f(char *p) { return 0; }\n"
    with pytest.raises(ValueError, match=match):
        cg.AnalysisSession(source, facts=facts)


def test_a_marked_block_comment_and_an_unmarked_line_comment_are_prose():
    source = """\
/* @cindergraph capacity(p) = n */
// see @cindergraph for the grammar
// axeyum capacity(p) = n
int f(char *p, unsigned long n) { return p[0]; }
"""
    session = cg.AnalysisSession(source)
    assert session.diagnostics == ()
    assert _facts(_nodes(session, "ast", "f")) == []


def test_a_comment_reaches_over_other_comments_but_not_over_a_declaration():
    source = """\
// @cindergraph capacity(p) = n
int g(char *p, unsigned long n);
// expect: f clean

/* prose */
int f(char *p, unsigned long n) { return p[0]; }
"""
    session = cg.AnalysisSession(source)
    assert [d.message for d in session.diagnostics] == [
        "fact comment is not followed by a function definition, so it attaches to nothing"
    ]
    assert _facts(_nodes(session, "ast", "f")) == []
    reached = source.replace("int g(char *p, unsigned long n);\n", "")
    session = cg.AnalysisSession(reached)
    assert session.diagnostics == ()
    assert _facts(_nodes(session, "ast", "f")) == [
        ("param_decl", "p", "capacity=n", "comment"),
    ]


def test_corpus_carries_the_annotations_this_contract_was_written_for():
    """The positive control for the per-sample walk below.

    The corpus has every kind and both value forms, and at least one sample
    (``01``) lowers a load of an annotated parameter, so the per-sample
    assertion that every such load carries the fact cannot pass vacuously.
    """
    lines = [m for path in SAMPLES for m in AXEYUM_LINE.finditer(path.read_text())]
    kinds = {m.group(1) for m in lines}
    assert kinds == {"capacity", "strlen", "unroll"}
    assert any(m.group(3).isdigit() for m in lines) and any(
        not m.group(3).isdigit() for m in lines
    )
    session = cg.AnalysisSession((DEFECTS / "01_length_overflow.c").read_text())
    loads = [
        node
        for node in _nodes(session, "ops", "assemble_bug")
        if node["kind"] == "load" and node.get("name") == "dst"
    ]
    assert loads and all(node["facts"] == "capacity=dst_len" for node in loads)


@pytest.mark.parametrize("path", SAMPLES, ids=lambda path: path.stem)
def test_every_axeyum_line_in_the_sample_is_a_fact_on_the_right_node(path: Path):
    """The consumer's own comments, read by Cindergraph, land where it reads them.

    The sample is analysed exactly as the consumer hands it over: no typedef
    prefix, so ``size_t`` parameters are of ``unknown`` type and are accepted
    as values. The "right node" is decided from the text: a comment attaches
    to the next function definition, which for these samples is always the
    function the consumer attaches it to (the one whose header follows; file
    ``09``'s ``unroll`` sits above ``zero_bug``).
    """
    text = path.read_text(encoding="utf-8")
    session = cg.AnalysisSession(text)
    assert session.diagnostics == (), [d.message for d in session.diagnostics]
    ast = dict(session.export_graphs(repr="ast", format="json"))
    definitions = []  # (byte offset of the definition, function name)
    for name, body in ast.items():
        (root,) = [n for n in json.loads(body)["nodes"] if n["tag"] == "func_def"]
        definitions.append((int(root["span"].split(":")[0]), name))
    definitions.sort()
    expected: set[tuple[str, str, str | None, str]] = set()
    for match in AXEYUM_LINE.finditer(text):
        offset = len(text[: match.start()].encode("utf-8"))
        function = next(name for start, name in definitions if start > offset)
        kind, parameter, value = match.groups()
        expected.add((function, kind, parameter, value))
    found: set[tuple[str, str, str | None, str]] = set()
    for name, body in ast.items():
        for node in json.loads(body)["nodes"]:
            if "facts" not in node:
                continue
            assert node["tag"] in {"func_def", "param_decl"}
            sources = node["facts_source"].split(",")
            pairs = node["facts"].split(",")
            assert len(sources) == len(pairs) and set(sources) == {"comment"}
            for pair in pairs:
                kind, value = pair.split("=")
                found.add((name, kind, node.get("name"), value))
    assert found == expected
    # And the consumer sees the fact beside every load of the parameter the
    # lowering expressed (a root the lowering declined --- `pack_bug`'s
    # `memcpy` with a cast in it --- is one `unknown` and has no loads).
    ops = dict(session.export_graphs(repr="ops", format="json"))
    for function, kind, parameter, value in expected:
        if parameter is None:
            continue
        for node in json.loads(ops[function])["nodes"]:
            if node["kind"] in {"load", "store"} and node.get("name") == parameter:
                assert f"{kind}={value}" in node["facts"].split(","), node
