"""Hotspot limits are counts, not Python slice end indices."""

from pathlib import Path

import cindergraph as cg
import pytest


FIXTURE = Path(__file__).resolve().parents[2] / "tests/fixtures/sizeof_local.c"


@pytest.mark.parametrize("limit", [-1, -10])
@pytest.mark.parametrize("empty", [False, True])
def test_negative_hotspot_limit_is_rejected(limit: int, empty: bool) -> None:
    report = cg.analyze("" if empty else FIXTURE.read_text())
    with pytest.raises(ValueError, match="limit.*non-negative"):
        report.hotspots(limit=limit)


@pytest.mark.parametrize("limit", [0, 1, 3, 100, None])
def test_hotspot_limits_are_prefixes_of_full_ranking(limit: int | None) -> None:
    report = cg.analyze(FIXTURE.read_text())
    full = report.hotspots(limit=None)
    assert len(full) == len(report.functions)
    assert report.hotspots(limit=limit) == (full if limit is None else full[:limit])


def test_empty_summary_does_not_invent_distributions() -> None:
    report = cg.analyze("")
    assert report.hotspots(limit=None) == ()
    assert report.summary()["functions"] == 0
    assert report.summary()["distributions"] == {}
