"""Value and call-site identity regressions from independent small programs."""

import random

import pytest

import cindergraph as cg


@pytest.mark.parametrize(
    "body, expected",
    [
        ("return x;", True),
        ("x=0;return x;", False),
        ("if(x){} return 0;", False),
        ("drop(x);return x;", True),
        ("return drop(x);", False),
        ("return keep(x);", True),
        ("int y=keep(x);int z=y;return z;", True),
        ("int y=drop(x);return y;", False),
        ("return drop(keep(x));", False),
        ("return keep(drop(x));", False),
        ("return keep(x+1);", True),
        ("return x ? 1 : 0;", True),
        ("if(x)return 1;return 0;", True),
        ("int y=0;if(x)y=1;return y;", True),
        ("int y=x?1:0;return y;", True),
        ("int y=0;if(x)y=1;y=0;return y;", False),
        ("if(drop(x))return 1;return 0;", False),
        ("if(keep(x))return 1;return 0;", True),
    ],
)
def test_return_provenance(body, expected):
    code = (
        "int keep(int v){return v;}int drop(int v){return 0;}int f(int x){" + body + "}"
    )
    summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
    assert (
        any(f["parameter"] == 0 and f["sink"] == "return" for f in summary["flows"])
        == expected
    )


@pytest.mark.parametrize(
    "body, expected",
    [
        ("return x;", "no"),
        ("sink(x);return 0;", "yes"),
        ("x=0;sink(x);return 0;", "no"),
        ("sink(x+1);return 0;", "yes"),
        ("int y=x;x=0;sink(y);return 0;", "yes"),
        ("sink(0);return x;", "no"),
    ],
)
def test_reachability_uses_actual_argument_values(body, expected):
    code = "int sink(int v){return 0;}int f(int x){" + body + "}"
    assert cg.reaches(code, "f", 0, "sink") == expected


def test_call_argument_positions_change_across_functions():
    code = (
        "int sink(int a){return 0;}"
        "int middle(int a,int b){sink(b);return 0;}"
        "int f(int x,int y){middle(y,x);return 0;}"
    )
    assert cg.reaches(code, "f", 0, "sink") == "yes"
    assert cg.reaches(code, "f", 1, "sink") == "no"


def test_function_pointer_shadow_does_not_resolve_to_same_named_function():
    code = "int keep(int x){return x;}int f(int (*keep)(int),int y){return keep(y);}"
    summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
    assert not summary["complete"]
    assert summary["flows"] == []
    assert cg.reaches(code, "f", 1, "keep") == "unknown"


@pytest.mark.parametrize("seed", range(40))
def test_random_scalar_assignments_match_symbolic_origin_sets(seed):
    rng = random.Random(seed)
    origins = {"x": {0}, "y": {1}}
    statements = []
    for _ in range(20):
        target = rng.choice(["x", "y"])
        value = rng.choice(["x", "y", "0", "x+y"])
        statements.append(f"{target}={value};")
        origins[target] = (
            origins["x"] | origins["y"]
            if value == "x+y"
            else set(origins.get(value, set()))
        )
    code = "int f(int x,int y){" + "".join(statements) + "return x;}"
    summary = cg.call_summaries(code)[0]
    actual = {f["parameter"] for f in summary["flows"] if f["sink"] == "return"}
    assert actual == origins["x"], (seed, code)


@pytest.mark.parametrize("seed", range(40))
def test_seeded_argument_graph_matches_independent_reachability(seed):
    rng = random.Random(seed)
    names = [f"f{i}" for i in range(6)]
    edges = {}
    bodies = []
    for name in names:
        calls = []
        for _ in range(rng.randrange(4)):
            callee = rng.choice(names)
            arguments = [rng.choice([0, 1, None]) for _ in range(2)]
            calls.append(
                callee
                + "("
                + ",".join("0" if a is None else ("x", "y")[a] for a in arguments)
                + ");"
            )
            for position, argument in enumerate(arguments):
                if argument is not None:
                    edges.setdefault((name, argument), set()).add((callee, position))
        bodies.append(f"int {name}(int x,int y){{" + "".join(calls) + "return 0;}")
    rng.shuffle(bodies)
    code = "".join(bodies)
    for _ in range(12):
        source, sink, parameter = rng.choice(names), rng.choice(names), rng.randrange(2)
        seen, pending = set(), [(source, parameter)]
        while pending:
            state = pending.pop()
            if state not in seen:
                seen.add(state)
                pending.extend(edges.get(state, ()))
        expected = "yes" if any(name == sink for name, _ in seen) else "no"
        assert cg.reaches(code, source, parameter, sink) == expected, (seed, code)
