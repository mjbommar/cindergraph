"""Directory discovery must not silently discard unreadable selected paths."""

from pathlib import Path

import cindergraph as cg
import pytest


@pytest.mark.parametrize("suffix", [".c", ".h"])
def test_dangling_source_symlink_raises(tmp_path: Path, suffix: str) -> None:
    link = tmp_path / ("missing" + suffix)
    try:
        link.symlink_to(tmp_path / "absent-target")
    except (OSError, NotImplementedError) as error:
        pytest.skip(f"symlink creation unavailable: {error}")
    with pytest.raises(FileNotFoundError):
        cg.parse_source(tmp_path, no_cfg=True)


def test_source_named_directory_is_not_read_as_file(tmp_path: Path) -> None:
    (tmp_path / "directory.c").mkdir()
    assert cg.parse_source(tmp_path, no_cfg=True) == {}
