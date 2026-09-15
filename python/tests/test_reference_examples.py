"""Execute trusted, checked-in reference examples against the installed API."""

from pathlib import Path
import re

import pytest


ROOT = Path(__file__).resolve().parents[2]
REFERENCE = ROOT / "docs" / "reference"


@pytest.mark.parametrize(
    "path",
    [
        ROOT / "README.md",
        REFERENCE / "source-python.md",
        REFERENCE / "source-metrics.md",
    ],
    ids=lambda path: str(path.relative_to(ROOT)),
)
def test_reference_examples(path: Path) -> None:
    """Run each page in order, retaining imports and values between examples."""
    text = path.read_text(encoding="utf-8")
    examples = list(re.finditer(r"^```python\n(.*?)^```", text, re.M | re.S))
    assert examples, f"No executable Python examples in {path}"
    namespace: dict[str, object] = {"__name__": "__reference_example__"}
    for example in examples:
        # Preserve Markdown line numbers so failures point to the actual page.
        padding = "\n" * text.count("\n", 0, example.start(1))
        exec(compile(padding + example.group(1), str(path), "exec"), namespace)
