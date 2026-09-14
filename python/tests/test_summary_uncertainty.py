"""Completeness has an independent graph-reachability oracle."""

import random

import pytest

import cindergraph as cg


@pytest.mark.parametrize("seed", range(40))
def test_unknown_callee_propagates_through_seeded_call_graph(seed):
    rng = random.Random(seed)
    names = [f"f{i}" for i in range(8)]
    edges = {name: rng.sample(names + ["external"], rng.randrange(4)) for name in names}
    bodies = [
        f"int {name}(int x){{"
        + "".join(f"{callee}(x);" for callee in edges[name])
        + "return 0;}"
        for name in names
    ]
    rng.shuffle(bodies)
    actual = {s["name"]: s["complete"] for s in cg.call_summaries("".join(bodies))}
    for name in names:
        visited = set()
        pending = [name]
        while pending:
            current = pending.pop()
            if current not in visited:
                visited.add(current)
                pending.extend(edges.get(current, []))
        assert actual[name] == ("external" not in visited), (seed, name, edges)


def test_ambiguous_definition_has_no_merged_positive_flow():
    code = "int f(int x){return x;} int f(int x,int y){return y;}"
    summary = cg.call_summaries(code)[0]
    assert not summary["complete"]
    assert summary["flows"] == []


def test_ambiguous_callee_taints_caller_completeness():
    code = "int f(int x){return x;} int f(int x){return 0;}int g(int x){return f(x);}"
    summaries = {s["name"]: s for s in cg.call_summaries(code)}
    assert not summaries["g"]["complete"]
    assert summaries["g"]["flows"] == []
