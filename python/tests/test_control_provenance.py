"""Finite execution witnesses must be covered by may-dependence summaries."""

import itertools
import random

import pytest

import cindergraph as cg


@pytest.mark.parametrize("seed", range(40))
def test_generated_branches_cover_observed_input_influence(seed):
    rng = random.Random(seed)
    operations = [
        (rng.choice(("x", "y")), rng.choice(("a", "b")), rng.randrange(-3, 4))
        for _ in range(8)
    ]
    code = (
        "int f(int x,int y){int a=0;int b=0;"
        + "".join(
            f"if({condition}){target}={value};"
            for condition, target, value in operations
        )
        + "return a+b;}"
    )

    def execute(x, y):
        state = dict(x=x, y=y, a=0, b=0)
        for condition, target, value in operations:
            if state[condition]:
                state[target] = value
        return state["a"] + state["b"]

    observed = set()
    inputs = list(itertools.product((-1, 0, 1), repeat=2))
    for first, second in itertools.combinations(inputs, 2):
        differences = [i for i in range(2) if first[i] != second[i]]
        if len(differences) == 1 and execute(*first) != execute(*second):
            observed.add(differences[0])
    actual = {
        flow["parameter"]
        for flow in cg.call_summaries(code)[0]["flows"]
        if flow["sink"] == "return"
    }
    assert observed <= actual, (seed, code, observed, actual)
