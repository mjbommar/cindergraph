"""Line buckets and coordinates obey the documented LF-based convention."""

from pathlib import Path

import cindergraph as cg
import pytest


ROOT = Path(__file__).resolve().parents[2] / "crates/cindergraph/tests"
FILES = sorted((ROOT / "decompiler_fixtures/src").glob("*.c")) + sorted(
    (ROOT / "decbench_corpus/src").glob("*.c")
)


def test_line_corpus_is_present() -> None:
    assert len(FILES) == 210


@pytest.mark.parametrize("text,lines", [("", 1), ("\n", 2), ("\r", 1), ("\r\n", 2)])
def test_empty_and_whitespace_line_convention(text: str, lines: int) -> None:
    report = cg.analyze(text)
    assert (
        report.lines,
        report.code_lines,
        report.blank_lines,
        report.other_lines,
    ) == (lines, 0, lines, 0)


@pytest.mark.parametrize("path", FILES, ids=lambda path: path.name)
def test_line_partition_and_newline_transformations(path: Path) -> None:
    code = path.read_bytes().decode("utf-8")
    before = cg.analyze(code)
    prefixed = cg.analyze("\n" + code)
    crlf = cg.analyze(code.replace("\r\n", "\n").replace("\n", "\r\n"))
    for text, report in ((code, before), ("\n" + code, prefixed)):
        assert report.lines == text.count("\n") + 1
        assert (
            report.lines == report.code_lines + report.blank_lines + report.other_lines
        )
        assert min(report.code_lines, report.blank_lines, report.other_lines) >= 0
    assert prefixed.lines == before.lines + 1
    assert prefixed.blank_lines == before.blank_lines + 1
    assert prefixed.code_lines == before.code_lines
    assert prefixed.other_lines == before.other_lines
    assert (crlf.lines, crlf.code_lines, crlf.blank_lines, crlf.other_lines) == (
        before.lines,
        before.code_lines,
        before.blank_lines,
        before.other_lines,
    )
    assert len(prefixed.functions) == len(before.functions)
    for old, new in zip(before.functions, prefixed.functions):
        assert new.name == old.name
        assert new.first_line == old.first_line + 1
        assert new.last_line == old.last_line + 1
        assert new.lines == old.lines
