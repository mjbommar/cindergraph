import json
import random

import pytest

import cindergraph as cg


@pytest.mark.parametrize(
    "setup", ["int *p=&y;", "int *q=&y;int *p=q;", "int *p; p=&y;"]
)
def test_local_pointer_store_reaches_return_in_all_dependence_consumers(setup):
    code = "int f(int x){int y=0;" + setup + "*p=x;return y;}"
    cfg = cg.control_flow_graphs(code)[0]["cfg"]
    raw = code.encode()
    store = next(n["id"] for n in cfg["nodes"] if b"*p=x" in raw[n["start"] : n["end"]])
    ret = next(n["id"] for n in cfg["nodes"] if n["kind"] == "return")
    assert store in cg.backward_slice(code, "f", ret)
    graph = json.loads(cg.export_graphs(code, repr="pdg", format="json")[0][1])
    assert any(e["source"] == store and e["target"] == ret for e in graph["edges"])
    assert any(
        f["parameter"] == 0 and f["sink"] == "return"
        for f in cg.call_summaries(code)[0]["flows"]
    )


def test_direct_overwrite_kills_indirect_store_before_return():
    code = "int f(int x){int y=0;int *p=&y;*p=x;y=0;return y;}"
    assert cg.call_summaries(code)[0]["flows"] == []


def test_pointer_load_reads_pointed_to_local_value():
    code = "int f(int x){int y=x;int *p=&y;return *p;}"
    assert any(
        f["parameter"] == 0 and f["sink"] == "return"
        for f in cg.call_summaries(code)[0]["flows"]
    )


def test_taking_address_does_not_kill_the_stored_value():
    code = "int f(int x){int y=x;int *p=&y;return y;}"
    assert any(
        f["parameter"] == 0 and f["sink"] == "return"
        for f in cg.call_summaries(code)[0]["flows"]
    )


@pytest.mark.parametrize("seed", range(40))
def test_random_local_pointer_chains_preserve_target_identity(seed):
    rng = random.Random(seed)
    count = rng.randrange(2, 8)
    target, returned = rng.randrange(count), rng.randrange(count)
    code = "int f(int x){" + "".join(f"int v{i}=0;" for i in range(count))
    code += f"int *p0=&v{target};"
    for i in range(1, count):
        code += f"int *p{i}=p{i - 1};"
    code += f"*p{count - 1}=x;return v{returned};}}"
    summary = cg.call_summaries(code)[0]
    assert any(
        f["parameter"] == 0 and f["sink"] == "return" for f in summary["flows"]
    ) == (target == returned)


@pytest.mark.parametrize(
    "code",
    [
        "int f(int *p){return *p;}",
        "int f(int *p,int x){*p=x;return 0;}",
        "int f(int *p){return p[0];}",
    ],
)
def test_unresolved_memory_access_is_not_reported_complete(code):
    assert not cg.data_flow(code)[0]["memory_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


def test_known_local_target_does_not_hide_an_unknown_pointer_alternative():
    code = "int f(int *q,int x){int y=0;int *p=&y;if(x)p=q;*p=x;return y;}"
    assert not cg.data_flow(code)[0]["memory_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


@pytest.mark.parametrize("operation", ["(*p)++", "++*p", "p++"])
def test_unmodeled_pointer_updates_are_incomplete(operation):
    code = "int f(int x){int y=x;int *p=&y;" + operation + ";return *p;}"
    assert not cg.data_flow(code)[0]["memory_complete"]
