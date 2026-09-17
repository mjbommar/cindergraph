"""One parse per source (item 13 of docs/improvement-list-2026-09-16.md).

``AnalysisSession`` parses once and serves diagnostics and every export from
that parse; the module-level functions each parse again. Measured 2026-09-17
with a trace on the parser entry point over a 190-line file: ``analyze`` +
``export_graphs`` for ``ast``, ``cfg`` and ``ops`` entered the parser four
times; a session doing the same work plus ``ddg``, ``cdg`` and ``pdg`` entered
it once. This pins the half of that which a consumer can rely on: choosing
the session changes how many times the text is parsed and nothing else.
"""

import cindergraph as cg

SOURCE = (
    "/* — */\n"
    "typedef unsigned long size_t;\n"
    "int f(unsigned char *dst, size_t n, unsigned short s) {\n"
    "    unsigned int table[16];\n"
    "    int i, acc = 0;\n"
    "    for (i = 0; i < 16; i++) table[i] = s + i;\n"
    "    while (n > 0) { acc += table[n & 15u]; n--; }\n"
    "    return dst[0] ? acc : -acc;\n"
    "}\n"
    "int g(int k) { return k * 2 /* stray */ + 1; }\n"
)


def test_session_exports_are_byte_identical_to_the_free_functions() -> None:
    session = cg.AnalysisSession(SOURCE)
    for repr in cg.EXPORT_REPRS:
        for format in cg.EXPORT_FORMATS:
            free = cg.export_graphs(SOURCE, repr=repr, format=format)
            held = session.export_graphs(repr=repr, format=format)
            assert held == free, (repr, format)
            assert [name for name, _ in held] == ["f", "g"], (repr, format)


def test_session_diagnostics_match_analyze() -> None:
    session = cg.AnalysisSession(SOURCE)
    report = cg.analyze(SOURCE)
    assert [str(d) for d in session.diagnostics] == [str(d) for d in report.diagnostics]
    assert session.source == SOURCE


def test_a_session_keeps_serving_the_same_bytes_after_other_queries() -> None:
    # Dataflow, summaries and slicing are lazily cached on the same unit; the
    # export bytes must not depend on which of them ran first.
    session = cg.AnalysisSession(SOURCE)
    before = session.export_graphs(repr="pdg", format="json")
    session.data_flow()
    session.call_summaries()
    session.control_flow_graphs()
    after = session.export_graphs(repr="pdg", format="json")
    assert after == before == cg.export_graphs(SOURCE, repr="pdg", format="json")
