import random

import pytest

import cindergraph as cg


@pytest.mark.parametrize("name", ["global", "external_state", "counter"])
def test_unresolved_identity_is_dense_and_different_names_do_not_alias(name):
    code = f"int f(void){{{name}=1;return other;}}"
    flow = cg.data_flow(code)[0]
    for event in flow["definitions"] + flow["uses"]:
        binding = flow["bindings"][event["binding"]]
        assert binding["name"] == event["name"]
        assert binding["is_unresolved"]
        assert binding["type"] is None
    assert flow["edges"] == []
    assert flow["unresolved_uses"] == [0]
    assert flow["dead_stores"] == []
    assert flow["unused_bindings"] == []


def test_repeated_global_writes_keep_only_the_last_reaching_write():
    flow = cg.data_flow("int f(void){global=1;global=2;return global;}")[0]
    assert [e["definition"] for e in flow["edges"]] == [1]
    assert len(flow["bindings"]) == 1


def test_local_shadow_and_unresolved_name_keep_separate_identities():
    flow = cg.data_flow("int f(void){g=1;{int g=2;use(g);}return g;}")[0]
    ids = {d["binding"] for d in flow["definitions"] if d["name"] == "g"}
    assert len(ids) == 2
    assert sum(b["is_unresolved"] for b in flow["bindings"] if b["name"] == "g") == 1


@pytest.mark.parametrize("seed", range(40))
def test_random_global_writes_match_last_write_per_name(seed):
    rng = random.Random(seed)
    names = [f"global_{i}" for i in range(8)]
    writes = [rng.choice(names) for _ in range(20)]
    returned = rng.choice(names)
    code = (
        "int f(void){"
        + "".join(f"{name}={i};" for i, name in enumerate(writes))
        + f"return {returned};}}"
    )
    flow = cg.data_flow(code)[0]
    expected = (
        [max(i for i, name in enumerate(writes) if name == returned)]
        if returned in writes
        else []
    )
    assert [e["definition"] for e in flow["edges"]] == expected
    assert flow["dead_stores"] == []
    for event in flow["definitions"] + flow["uses"]:
        assert flow["bindings"][event["binding"]]["name"] == event["name"]
