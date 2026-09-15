"""Independent affine-coefficient oracle for straight-line scalar provenance.

Only copies, positive additions, overwrites and two known call effects are
generated. Coefficients
stay below 2**32, avoiding cancellation in unsigned arithmetic. This checks
the supported scalar fragment, not arbitrary C or compiler equivalence.
"""

import random

import cindergraph as cg
import pytest


@pytest.mark.parametrize(
    ("code", "expected"),
    [
        (
            "int f(void){int x=0;int *p=&x;*p=1;x=2;return x;}",
            ["assignment"],
        ),
        (
            "int f(void){int x=0;int *p=&x;x=2;*p=1;return x;}",
            ["assignment", "memory_write"],
        ),
    ],
)
def test_weak_and_strong_writes_preserve_source_order(
    code: str, expected: list[str]
) -> None:
    flow = cg.data_flow(code)[0]
    return_use = next(
        index
        for index, use in enumerate(flow["uses"])
        if use["start"] == code.rfind("x")
    )
    kinds = [
        flow["definitions"][edge["definition"]]["kind"]
        for edge in flow["edges"]
        if edge["use"] == return_use
    ]
    assert kinds == expected


@pytest.mark.parametrize("seed", range(200))
def test_mixed_pointer_and_direct_writes_retain_the_concrete_last_write(
    seed: int,
) -> None:
    rng = random.Random(seed)
    operations = [
        (rng.choice([False, True]), value) for value in range(1, rng.randrange(2, 15))
    ]
    body = "".join(
        f"{'*p' if indirect else 'x'}={value};" for indirect, value in operations
    )
    code = f"int f(void){{int x=0;int *p=&x;{body}return x;}}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    return_use = next(
        index
        for index, use in enumerate(flow["uses"])
        if use["start"] == code.rfind("x")
    )
    reaching = [
        flow["definitions"][edge["definition"]]
        for edge in flow["edges"]
        if edge["use"] == return_use
    ]
    indirect, value = operations[-1]
    expression = f"{'*p' if indirect else 'x'}={value}"
    expected_kind = "memory_write" if indirect else "assignment"
    expected_start = code.rfind(expression)
    assert any(
        definition["kind"] == expected_kind and definition["start"] == expected_start
        for definition in reaching
    ), code


@pytest.mark.parametrize("seed", range(250))
def test_pointer_copy_and_reassignment_retain_each_concrete_target(seed: int) -> None:
    rng = random.Random(seed)
    pointers = {"p": "x", "q": "y"}
    writes: dict[str, tuple[str, int]] = {}
    code = "int f(void){int x=0,y=0;int *p=&x,*q=&y;"
    for value in range(1, rng.randrange(5, 25)):
        operation = rng.randrange(8)
        if operation == 0:
            statement = "p=&x;"
            pointers["p"] = "x"
        elif operation == 1:
            statement = "p=&y;"
            pointers["p"] = "y"
        elif operation == 2:
            statement = "q=p;"
            pointers["q"] = pointers["p"]
        elif operation == 3:
            statement = "p=q;"
            pointers["p"] = pointers["q"]
        elif operation == 4:
            statement = f"*p={value};"
            writes[pointers["p"]] = ("memory_write", len(code))
        elif operation == 5:
            statement = f"*q={value};"
            writes[pointers["q"]] = ("memory_write", len(code))
        elif operation == 6:
            statement = f"x={value};"
            writes["x"] = ("assignment", len(code))
        else:
            statement = f"y={value};"
            writes["y"] = ("assignment", len(code))
        code += statement
    if not writes:
        writes["x"] = ("assignment", len(code))
        code += "x=99;"
    returned = rng.choice(list(writes))
    code += f"return {returned};}}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    return_use = next(
        index
        for index, use in enumerate(flow["uses"])
        if use["start"] == code.rfind(returned)
    )
    reaching = [
        flow["definitions"][edge["definition"]]
        for edge in flow["edges"]
        if edge["use"] == return_use
    ]
    expected_kind, expected_start = writes[returned]
    assert any(
        definition["kind"] == expected_kind and definition["start"] == expected_start
        for definition in reaching
    ), code


@pytest.mark.parametrize("seed", range(200))
@pytest.mark.parametrize("with_calls", [False, True])
def test_affine_assignment_provenance(seed: int, with_calls: bool) -> None:
    rng = random.Random(seed)
    coefficients = {
        "a": (1, 0, 0),
        "b": (0, 1, 0),
        "c": (0, 0, 1),
        "x": (0, 0, 0),
        "y": (0, 0, 0),
        "z": (0, 0, 0),
    }
    lines = ["unsigned f(unsigned a,unsigned b,unsigned c){unsigned x=0,y=0,z=0;"]
    operations = ["copy", "constant", "add", "increment"]
    if with_calls:
        operations += ["call_copy", "call_drop"]
        lines.insert(0, "unsigned identity(unsigned);unsigned discard(unsigned);")
    for _ in range(25):
        target = rng.choice(["x", "y", "z"])
        source = rng.choice(list(coefficients))
        operation = rng.choice(operations)
        if operation == "copy":
            lines.append(f"{target}={source};")
            coefficients[target] = coefficients[source]
        elif operation == "constant":
            lines.append(f"{target}={rng.randrange(10)};")
            coefficients[target] = (0, 0, 0)
        elif operation == "add":
            lines.append(f"{target}+={source};")
            coefficients[target] = tuple(
                left + right
                for left, right in zip(coefficients[target], coefficients[source])
            )
        elif operation == "call_copy":
            lines.append(f"{target}=identity({source});")
            coefficients[target] = coefficients[source]
        elif operation == "call_drop":
            lines.append(f"{target}=discard({source});")
            coefficients[target] = (0, 0, 0)
        else:
            lines.append(f"{target}++;")
    result = rng.choice(["x", "y", "z"])
    lines.append(f"return {result};}}")
    if with_calls:
        lines.append("unsigned identity(unsigned v){return v;}")
        lines.append("unsigned discard(unsigned v){return 0;}")
    code = "".join(lines)
    expected = {i for i, coefficient in enumerate(coefficients[result]) if coefficient}
    assert max(coefficients[result]) < 2**32
    summary = next(s for s in cg.call_summaries(code) if s["name"] == "f")
    assert summary["complete"], code
    actual = {
        flow["parameter"] for flow in summary["flows"] if flow["sink"] == "return"
    }
    assert actual == expected, code
