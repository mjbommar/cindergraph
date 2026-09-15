"""Declarators in one declaration have separate initializer effects."""

import cindergraph as cg
import pytest
import random


def semantic_summary(summary: dict) -> dict:
    """Payload equality deliberately excludes snapshot-local identity."""
    return {
        key: value
        for key, value in summary.items()
        if key not in {"source_id", "function_id"}
    }


@pytest.mark.parametrize(
    "body",
    ["int a=x,b=a;return b;", "int a=x,b=a,c=b;return c;", "int a=0,b=x;return a;"],
)
def test_declarator_sequence_matches_separate_declarations(body: str) -> None:
    combined = "int f(int x){" + body + "}"
    separated = combined.replace(",b=", ";int b=").replace(",c=", ";int c=")
    combined_summary = cg.call_summaries(combined)[0]
    separated_summary = cg.call_summaries(separated)[0]
    assert combined_summary["source_id"] != separated_summary["source_id"]
    assert semantic_summary(combined_summary) == semantic_summary(separated_summary)
    assert not cg.data_flow(combined)[0]["unresolved_uses"]


@pytest.mark.parametrize("seed", range(128))
def test_seeded_initializer_chains_match_value_oracle(seed: int) -> None:
    rng = random.Random(seed)
    dependencies = {"x": True}
    declarators = []
    for index in range(24):
        source = rng.choice(list(dependencies))
        mode = rng.randrange(4)
        expression = [source, "0", f"identity({source})", f"drop({source})"][mode]
        name = f"v{index}"
        declarators.append(f"{name}={expression}")
        dependencies[name] = dependencies[source] if mode in (0, 2) else False
    returned = rng.choice(list(dependencies))
    expected = (
        [{"parameter": 0, "sink": "return", "sink_parameter": None}]
        if dependencies[returned]
        else []
    )
    helpers = "int identity(int a){return a;} int drop(int a){return 0;}"
    summaries = []
    for separator in (",", ";int "):
        code = (
            helpers
            + "int f(int x){int "
            + separator.join(declarators)
            + f";return {returned};}}"
        )
        flow = next(f for f in cg.data_flow(code) if f["name"] == "f")
        assert not flow["unresolved_uses"], (seed, separator)
        summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
        assert summary["complete"], (seed, separator)
        assert summary["flows"] == expected, (seed, separator, code)
        summaries.append(summary)
    assert summaries[0]["source_id"] != summaries[1]["source_id"]
    assert semantic_summary(summaries[0]) == semantic_summary(summaries[1])


@pytest.mark.parametrize(
    ("declaration", "expected"),
    [
        ("int a=x*2,b=x", {"a": (0, 0), "b": (0, 0)}),
        ("int a=p[0],b=x", {"a": (0, 0), "b": (0, 0)}),
        ("int a=*p,*b=p", {"a": (0, 0), "b": (1, 0)}),
        ("int a[4]=p[0],*b=p", {"a": (0, 1), "b": (1, 0)}),
    ],
)
def test_initializer_tokens_do_not_contaminate_declared_types(
    declaration: str, expected: dict[str, tuple[int, int]]
) -> None:
    code = f"int f(int x,int *p){{{declaration};return x;}}"
    bindings = {item["name"]: item for item in cg.data_flow(code)[0]["bindings"]}
    actual = {
        name: (bindings[name]["pointer_depth"], bindings[name]["array_rank"])
        for name in expected
    }
    assert actual == expected


@pytest.mark.parametrize("seed", range(64))
def test_seeded_declarator_shapes_ignore_neighbour_initializers(seed: int) -> None:
    rng = random.Random(seed)
    declarations = []
    expected: dict[str, tuple[int, int]] = {}
    scalar_initializers = ["x*2", "p[0]", "*p", "identity(p[0])", "x?*p:p[1]"]
    for index in range(20):
        name = f"v{index}"
        shape = rng.randrange(3)
        if shape == 0:
            declarations.append(f"{name}={rng.choice(scalar_initializers)}")
            expected[name] = (0, 0)
        elif shape == 1:
            declarations.append(f"*{name}=p")
            expected[name] = (1, 0)
        else:
            declarations.append(f"{name}[{rng.randrange(1, 9)}]={{0}}")
            expected[name] = (0, 1)
    code = (
        "int identity(int x){return x;}"
        f"int f(int x,int *p){{int {','.join(declarations)};return x;}}"
    )
    function = next(item for item in cg.data_flow(code) if item["name"] == "f")
    bindings = {item["name"]: item for item in function["bindings"]}
    actual = {
        name: (bindings[name]["pointer_depth"], bindings[name]["array_rank"])
        for name in expected
    }
    assert actual == expected, (seed, code)
