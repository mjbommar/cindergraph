"""Lexical damage must not erase structurally separate later functions."""

import cindergraph as cg


def test_unterminated_comment_resynchronizes_at_later_function() -> None:
    code = """
int left(void) { return 1; }
int damaged(void) { return 2 + /* unclosed
static int right(void) { return 3; }
"""
    session = cg.AnalysisSession(code)
    assert [graph["name"] for graph in session.control_flow_graphs()] == [
        "left",
        "damaged",
        "right",
    ]
    assert any(
        diagnostic.message == "unterminated block comment"
        for diagnostic in session.diagnostics
    )
    assert all(not flow["recovery_free"] for flow in session.data_flow())


def test_unterminated_comment_does_not_restart_at_control_statement() -> None:
    code = """
int damaged(int x) { /* unclosed
if (x) { return 2; }
"""
    session = cg.AnalysisSession(code)
    assert [graph["name"] for graph in session.control_flow_graphs()] == ["damaged"]
    assert any(
        diagnostic.message == "unterminated block comment"
        for diagnostic in session.diagnostics
    )
