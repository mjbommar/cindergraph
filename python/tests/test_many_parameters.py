"""Parameter identity and position survive wide function signatures."""

import random

import cindergraph as cg
import pytest


@pytest.mark.parametrize("count", [1, 32, 128, 512])
def test_selected_parameter_is_the_only_return_source(count: int) -> None:
    selected = random.Random(718 + count).randrange(count)
    signature = ",".join(f"int p{i}" for i in range(count))
    code = f"int f({signature}){{return p{selected};}}"
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["parameters"] == count
    assert summary["flows"] == [
        {"parameter": selected, "sink": "return", "sink_parameter": None}
    ]
