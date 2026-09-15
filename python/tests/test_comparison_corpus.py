"""Comparison invariants over real fixtures and their truncated versions."""

from pathlib import Path

import cindergraph as cg
import pytest


ROOT = Path(__file__).resolve().parents[2] / "crates/cindergraph/tests"
FILES = sorted((ROOT / "decompiler_fixtures/src").glob("*.c")) + sorted(
    (ROOT / "decbench_corpus/src").glob("*.c")
)


def test_comparison_corpus_is_present() -> None:
    assert len(FILES) == 210


@pytest.mark.parametrize("path", FILES, ids=lambda path: path.name)
def test_comparison_reversal_and_matched_population(path: Path) -> None:
    code = path.read_text(encoding="utf-8")
    before = cg.analyze(code)
    # Include a recovered partial final definition; this tests report algebra,
    # not whether truncation preserves C semantics.
    after = cg.analyze(code[: 3 * len(code) // 4])
    left = {f.name: f for f in before.functions}
    right = {f.name: f for f in after.functions}
    assert len(left) == len(before.functions), path
    assert len(right) == len(after.functions), path
    forward = cg.compare(before, after)
    reverse = cg.compare(after, before)
    assert forward["added"] == reverse["removed"] == sorted(right.keys() - left.keys())
    assert forward["removed"] == reverse["added"] == sorted(left.keys() - right.keys())
    shared = left.keys() & right.keys()
    assert {row["name"] for row in forward["matched"]} == shared
    assert [row["name"] for row in forward["matched"]] == [
        row["name"] for row in reverse["matched"]
    ]
    for a, b in zip(forward["matched"], reverse["matched"]):
        assert a["before"] == b["after"]
        assert a["after"] == b["before"]
        for metric in cg.COMPARED_METRICS:
            assert a["deltas"][metric] == -b["deltas"][metric]
            assert a["deltas"][metric] == a["after"][metric] - a["before"][metric]
    for metric in cg.COMPARED_METRICS:
        a = forward["totals"][metric]
        b = reverse["totals"][metric]
        assert (
            a["before"]
            == b["after"]
            == sum(getattr(left[name], metric) for name in shared)
        )
        assert (
            a["after"]
            == b["before"]
            == sum(getattr(right[name], metric) for name in shared)
        )
        assert a["delta"] == -b["delta"] == a["after"] - a["before"]
