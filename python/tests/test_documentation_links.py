"""Keep the maintained documentation routes resolvable."""

from pathlib import Path
import re

import pytest


ROOT = Path(__file__).resolve().parents[2]
INDEXES = (
    ROOT / "README.md",
    ROOT / "CHANGELOG.md",
    ROOT / "docs" / "README.md",
    ROOT / "docs" / "install.md",
    ROOT / "docs" / "releasing.md",
    ROOT / "docs" / "support-and-evidence.md",
    ROOT / "docs" / "reference" / "source-python.md",
    ROOT / "docs" / "reference" / "source-metrics.md",
    ROOT / "review" / "README.md",
    ROOT / "review" / "qa-iteration-49.md",
    ROOT / "review" / "qa-iteration-50.md",
)
LINK = re.compile(r"(?<!!)\[[^]]+\]\(([^)]+)\)")


@pytest.mark.parametrize(
    "document", INDEXES, ids=lambda path: str(path.relative_to(ROOT))
)
def test_relative_documentation_links_resolve(document: Path) -> None:
    """A reader can follow every local link in maintained routing pages."""
    assert document.is_file()
    for match in LINK.finditer(document.read_text(encoding="utf-8")):
        target = match.group(1).split("#", 1)[0]
        if not target or "://" in target or target.startswith("mailto:"):
            continue
        resolved = (document.parent / target).resolve()
        assert resolved.is_file(), f"{document.relative_to(ROOT)} -> {target}"


def test_support_page_is_linked_from_public_entrypoint() -> None:
    """Release qualifications stay discoverable from the package README."""
    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    assert "docs/support-and-evidence.md" in readme


def test_review_index_points_to_latest_iteration() -> None:
    """The chronology does not silently stop before the latest QA record."""
    records = sorted(
        (int(path.stem.rsplit("-", 1)[1]), path.name)
        for path in (ROOT / "review").glob("qa-iteration-*.md")
    )
    latest = records[-1][1]
    index = (ROOT / "review" / "README.md").read_text(encoding="utf-8")
    assert latest in index
