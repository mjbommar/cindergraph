"""Path-taking adapters must name the mistake when handed C source text."""

from pathlib import Path

import cindergraph as cg
import pytest
from cindergraph.compat import pyjoern

SOURCE = "int f(int a) {\n    return a + 1;\n}\n"
ONE_LINE = "int " + "x" * 5000 + "(void) { return 0; }"


@pytest.mark.parametrize(
    ("call", "parameter"),
    [
        (pyjoern.parse_source, "source_path"),
        (pyjoern.fast_cfgs_from_source, "filepath"),
        (pyjoern.parse_callgraph, "source_path"),
    ],
)
def test_source_text_with_newlines_raises_a_value_error_naming_the_argument(
    call, parameter: str
) -> None:
    with pytest.raises(ValueError, match=rf"{parameter} to be a path") as info:
        call(SOURCE)
    assert "C source text" in str(info.value)
    assert not isinstance(info.value, OSError)


def test_one_line_source_text_too_long_for_a_name_is_reported_the_same_way() -> None:
    # No newline to recognize it by; the filesystem's ENAMETOOLONG is what
    # arrives, and it is translated rather than leaked as OSError.
    with pytest.raises(ValueError, match="source_path to be a path"):
        cg.parse_source(ONE_LINE)


def test_a_real_path_still_works_and_a_missing_one_is_still_an_os_error(
    tmp_path: Path,
) -> None:
    path = tmp_path / "f.c"
    path.write_text(SOURCE)
    assert set(cg.parse_source(path, no_cfg=True)) == {"f"}
    assert set(cg.parse_source(str(path), no_cfg=True)) == {"f"}
    with pytest.raises(FileNotFoundError):
        cg.parse_source(tmp_path / "absent.c", no_cfg=True)
