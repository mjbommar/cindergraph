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


def test_projected_memory_regions_preserve_identity_and_precision():
    code = (
        "struct S{int x;int a[4];};"
        "int f(int i,int v){struct S s;struct S *p=&s;"
        "s.x=v;p->x=v;s.a[i]=v;return s.x+s.a[0];}"
    )
    flow = cg.data_flow(code)[0]
    x_region = next(
        region
        for region in flow["memory_regions"]
        if region["kind"] == "field" and region["member"] == "x"
    )
    x_accesses = [
        access
        for access in flow["memory_accesses"]
        if access["region"] == x_region["id"]
    ]
    assert len(x_accesses) == 3
    assert {access["kind"] for access in x_accesses} == {"read", "write"}
    assert {access["precision"] for access in x_accesses} == {
        "exact",
        "may_alias",
    }

    element_region = next(
        region for region in flow["memory_regions"] if region["kind"] == "elements"
    )
    element_accesses = [
        access
        for access in flow["memory_accesses"]
        if access["region"] == element_region["id"]
    ]
    assert len(element_accesses) == 2
    assert all(access["precision"] == "may_alias" for access in element_accesses)
    assert any(
        overlap["kind"] == "containment" and overlap["right"] == element_region["id"]
        for overlap in flow["memory_overlaps"]
    )
    field_use = next(
        index
        for index, use in enumerate(flow["memory_uses"])
        if use["region"] == x_region["id"]
    )
    assert sum(edge["use"] == field_use for edge in flow["memory_edges"]) == 2
    element_use = next(
        index
        for index, use in enumerate(flow["memory_uses"])
        if use["region"] == element_region["id"]
    )
    assert sum(edge["use"] == element_use for edge in flow["memory_edges"]) == 1
    assert flow["memory_complete"]


def test_exact_projected_writes_kill_and_branch_writes_join():
    linear = cg.data_flow(
        "struct S{int x;};int f(int a,int b){struct S s;s.x=a;s.x=b;return s.x;}"
    )[0]
    edge = linear["memory_edges"][0]
    assert edge == {
        "definition": 1,
        "use": 0,
        "definition_region": linear["memory_definitions"][1]["region"],
        "use_region": linear["memory_uses"][0]["region"],
        "overlap": None,
    }

    branch = cg.data_flow(
        "struct S{int x;};"
        "int f(int c,int a,int b){struct S s;"
        "if(c)s.x=a;else s.x=b;return s.x;}"
    )[0]
    assert {edge["definition"] for edge in branch["memory_edges"]} == {0, 1}


def test_union_members_have_explicit_overlap_and_cross_member_flow():
    union = cg.data_flow(
        "union U{int a;int b;};int f(int x,int y){union U u;u.a=x;u.b=y;return u.a;}"
    )[0]
    assert any(
        overlap["kind"] == "union_members" for overlap in union["memory_overlaps"]
    )
    assert len(union["memory_edges"]) == 1
    edge = union["memory_edges"][0]
    assert edge["definition"] == 1
    assert edge["overlap"] == "union_members"
    assert union["memory_regions"][edge["definition_region"]]["member"] == "b"
    assert union["memory_regions"][edge["use_region"]]["member"] == "a"

    structure = cg.data_flow(
        "struct S{int a;int b;};int f(int x){struct S s;s.a=x;return s.b;}"
    )[0]
    assert not any(
        overlap["kind"] == "union_members" for overlap in structure["memory_overlaps"]
    )
    assert structure["memory_edges"] == []

    nested = cg.data_flow(
        "struct A{int x;};struct B{int y;};"
        "union U{struct A a;struct B b;};"
        "int f(int x){union U u;u.a.x=x;return u.b.y;}"
    )[0]
    edge = nested["memory_edges"][0]
    assert edge["overlap"] == "union_members"
    assert nested["memory_regions"][edge["definition_region"]]["member"] == "x"
    assert nested["memory_regions"][edge["use_region"]]["member"] == "y"


def test_incoming_aggregate_parameter_field_reaches_return_until_overwritten():
    source = "struct S{int x;};int f(struct S s){return s.x;}"
    flow = cg.data_flow(source)[0]
    assert flow["memory_definitions"][0]["kind"] == "incoming_parameter"
    assert flow["memory_edges"][0]["definition"] == 0
    assert all(use["name"] != "s" for use in flow["uses"])
    summary = cg.call_summaries(source)[0]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]
    assert summary["complete"]

    overwritten = "struct S{int x;};int f(struct S s){s.x=0;return s.x;}"
    flow = cg.data_flow(overwritten)[0]
    assert (
        flow["memory_definitions"][flow["memory_edges"][0]["definition"]]["kind"]
        == "store"
    )
    assert all(use["name"] != "s" for use in flow["uses"])
    assert cg.call_summaries(overwritten)[0]["flows"] == []


def test_pointer_call_arguments_add_weak_clobbers_without_poisoning_value_calls():
    projected = (
        "struct S{int x;};void touch(struct S *);"
        "int f(int v){struct S s;s.x=v;touch(&s);return s.x;}"
    )
    flow = cg.data_flow(projected)[0]
    assert len(flow["memory_edges"]) == 2
    assert any(
        definition["kind"] == "call_clobber"
        for definition in flow["memory_definitions"]
    )
    assert not flow["memory_complete"]

    value_only = cg.data_flow("int consume(int);int f(int x){return consume(x);}")[0]
    assert value_only["memory_complete"]
    assert all(
        definition["kind"] != "memory_write" for definition in value_only["definitions"]
    )

    array = cg.data_flow(
        "void touch(int *);int f(void){int a[4];a[0]=1;touch(a);return a[0];}"
    )[0]
    assert len(array["memory_edges"]) == 2
    assert any(
        definition["kind"] == "call_clobber"
        for definition in array["memory_definitions"]
    )
    assert all(use["name"] != "a" for use in array["uses"])


def test_projected_memory_flow_reaches_pdg_export_and_backward_slice():
    code = "struct S{int x;};int f(int v){struct S s;s.x=v;return s.x;}"
    cfg = cg.control_flow_graphs(code)[0]["cfg"]
    raw = code.encode()
    store = next(
        node["id"]
        for node in cfg["nodes"]
        if b"s.x=v" in raw[node["start"] : node["end"]]
    )
    ret = next(node["id"] for node in cfg["nodes"] if node["kind"] == "return")
    assert store in cg.backward_slice(code, "f", ret)

    pdg = json.loads(cg.export_graphs(code, repr="pdg", format="json")[0][1])
    assert any(
        edge["source"] == store
        and edge["target"] == ret
        and edge.get("memory") == "s.x"
        for edge in pdg["edges"]
    )


@pytest.mark.parametrize("target", ["(*p)", "(((*p)))"])
def test_parenthesized_pointer_store_reaches_return(target):
    code = f"int f(int x){{int y=0;int *p=&y;{target}=x;return y;}}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return"
        for item in cg.call_summaries(code)[0]["flows"]
    )


def test_taking_address_does_not_kill_the_stored_value():
    code = "int f(int x){int y=x;int *p=&y;return y;}"
    assert any(
        f["parameter"] == 0 and f["sink"] == "return"
        for f in cg.call_summaries(code)[0]["flows"]
    )


def test_taking_address_is_an_escape_event_not_a_value_read_or_write():
    flow = cg.data_flow("int f(void){int x=1;(void)&x;return 0;}")[0]
    x = next(
        index
        for index, binding in enumerate(flow["bindings"])
        if binding["name"] == "x"
    )
    assert any(
        definition["binding"] == x and definition["kind"] == "address_taken"
        for definition in flow["definitions"]
    )
    assert not any(use["binding"] == x for use in flow["uses"])
    assert not flow["edges"]
    assert not flow["dead_stores"]
    assert x not in flow["unused_bindings"]


@pytest.mark.parametrize(
    ("setup", "complete"),
    [
        ("int *p;int *q;p=q;q=&y;", False),
        ("int *p;if(x)p=&y;", False),
        ("int *p;while(x)p=&y;", False),
        ("int *p;p=&y;", True),
        ("int *p;if(x)p=&y;else p=&y;", True),
        ("int *p;do p=&y;while(x);", True),
    ],
)
def test_pointer_completeness_requires_initialization_on_every_path(setup, complete):
    code = f"int f(int x){{int y=0;{setup}*p=x;return y;}}"
    assert cg.data_flow(code)[0]["memory_complete"] is complete
    assert cg.call_summaries(code)[0]["complete"] is complete


@pytest.mark.parametrize(
    ("body", "complete"),
    [
        ("(p=&y,0);*p=x;", True),
        ("x?(p=&y):(p=&y);*p=x;", True),
        ("(0,*p=x,p=&y);", False),
        ("int *q=&y;((p=q+1,1),0);*p=x;", False),
    ],
)
def test_discarded_values_preserve_pointer_assignment_side_effects(body, complete):
    code = f"int f(int x){{int y=0;int *p;{body}return y;}}"
    assert cg.data_flow(code)[0]["memory_complete"] is complete
    assert cg.call_summaries(code)[0]["complete"] is complete


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
        "int f(int *p,int x){((*p))=x;return 0;}",
        "int f(int *p){return p[0];}",
    ],
)
def test_formal_pointee_memory_access_is_reported_as_an_abstract_effect(code):
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert any(
        region["kind"] == "parameter_pointee" for region in flow["memory_regions"]
    )
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["memory_effects_complete"]
    assert summary["memory_effects"]


def test_known_local_and_formal_pointee_alternatives_are_both_retained():
    code = "int f(int *q,int x){int y=0;int *p=&y;if(x)p=q;*p=x;return y;}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert {region["kind"] for region in flow["memory_regions"]} >= {
        "binding",
        "parameter_pointee",
    }
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert any(
        effect["parameter"] == 0 and effect["kind"] == "write"
        for effect in summary["memory_effects"]
    )


@pytest.mark.parametrize(
    "pointer_update",
    ["p=p+1;", "int *q=p+x;p=q;", "int *q=p++;p=q;"],
)
def test_pointer_arithmetic_invalidates_a_complete_local_target(pointer_update):
    code = f"int f(int x){{int y=0;int *p=&y;{pointer_update}*p=x;return y;}}"
    assert not cg.data_flow(code)[0]["memory_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


@pytest.mark.parametrize(
    "initializer",
    ["(x + 1) ? &a : &b", "(x + 1, &a)", "(x ? &a : (x + 1, &b))"],
)
def test_non_result_arithmetic_does_not_taint_a_known_pointer(initializer):
    code = f"int f(int x){{int a=0,b=0;int *p={initializer};*p=x;return a;}}"
    assert cg.data_flow(code)[0]["memory_complete"]
    assert cg.call_summaries(code)[0]["complete"]


def test_pointer_replaced_through_a_double_pointer_retains_the_new_target_flow():
    code = "int f(int x){int a=0,b=0;int *p=&a;int **q=&p;*q=&b;*p=x;return b;}"
    flow = cg.data_flow(code)[0]
    summary = cg.call_summaries(code)[0]
    assert flow["memory_complete"]
    assert summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return" for item in summary["flows"]
    )


def test_pointer_loaded_after_second_order_replacement_retains_the_new_target_flow():
    code = (
        "int f(int x){int a=0,b=0;int *p=&a;int **q=&p;*q=&b;int *r=*q;*r=x;return b;}"
    )
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return" for item in summary["flows"]
    )


@pytest.mark.parametrize("seed", range(100))
def test_pointer_loaded_through_a_known_double_pointer_is_complete(seed):
    rng = random.Random(seed)
    names = [f"v{index}" for index in range(rng.randrange(2, 9))]
    target = rng.randrange(len(names))
    code = "int f(int x){" + "".join(f"int {name}=0;" for name in names)
    code += f"int *p=&{names[target]};int **q=&p;"
    previous = "*q"
    for index in range(rng.randrange(1, 7)):
        code += f"int *r{index}={previous};"
        previous = f"r{index}"
    code += f"*{previous}=x;return {names[target]};}}"
    flow = cg.data_flow(code)[0]
    summary = cg.call_summaries(code)[0]
    assert flow["memory_complete"]
    assert summary["complete"]
    assert any(
        item["parameter"] == 0 and item["sink"] == "return" for item in summary["flows"]
    )


@pytest.mark.parametrize(
    "initializer",
    [
        "x ? &a : (int *)1",
        "x ? &a : (int *)0",
        "(int *)x",
        "(int *)(x ? &a : x)",
    ],
)
def test_integer_to_pointer_cast_invalidates_local_memory_completeness(initializer):
    code = f"int f(int x){{int a=0;int *p={initializer};*p=x;return a;}}"
    assert not cg.data_flow(code)[0]["memory_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


@pytest.mark.parametrize(
    "initializer",
    ["(int *)(long)&a", "(int *)(unsigned long)(void *)&a"],
)
def test_pointer_integer_pointer_round_trip_is_not_a_certified_local_alias(initializer):
    code = f"int f(int x){{int a=0;int *p={initializer};*p=x;return a;}}"
    assert not cg.data_flow(code)[0]["memory_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


def test_address_of_dereference_reads_pointer_without_loading_pointee():
    code = "int f(int x){int y=x;int *p=&y;return &*p != 0;}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert any(use["name"] == "p" for use in flow["uses"])
    assert not any(use["name"] == "y" for use in flow["uses"])
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == []


def test_address_of_dereference_preserves_pointer_copy_target():
    code = "int f(int x){int y=0;int *p=&y;int *q=&*p;*q=x;return y;}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert any(item["parameter"] == 0 for item in summary["flows"])


@pytest.mark.parametrize(
    ("body", "address_events"),
    [
        ("int y=x;return *&y;", 0),
        ("int y=0;*&y=x;return y;", 0),
        ("int y=x;int *p=&y;return *&*p;", 1),
        ("int y=0;int *p=&y;*&*p=x;return y;", 1),
    ],
)
def test_dereference_of_address_is_a_direct_object_access(body, address_events):
    code = f"int f(int x){{{body}}}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert (
        sum(item["kind"] == "address_taken" for item in flow["definitions"])
        == address_events
    )
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert any(item["parameter"] == 0 for item in summary["flows"])


@pytest.mark.parametrize("operation", ["(*p)++", "++*p", "(*p)--", "--*p"])
def test_increment_through_known_pointer_is_complete(operation):
    code = "int f(int x){int y=x;int *p=&y;" + operation + ";return *p;}"
    flow = cg.data_flow(code)[0]
    assert flow["memory_complete"]
    assert any(item["kind"] == "memory_write" for item in flow["definitions"])
    assert cg.call_summaries(code)[0]["complete"]


def test_increment_of_pointer_itself_remains_incomplete():
    code = "int f(int x){int y=x;int *p=&y;p++;return *p;}"
    assert not cg.data_flow(code)[0]["memory_complete"]


def test_formal_pointee_regions_and_summary_effects_are_public():
    source = (
        "struct Pair { int left; int values[4]; }; "
        "int inspect(struct Pair *p, int i) { "
        "p->left = p->values[i]; return p->left; }"
    )
    flow = cg.data_flow(source)[0]
    roots = [
        region
        for region in flow["memory_regions"]
        if region["kind"] == "parameter_pointee"
    ]
    assert len(roots) == 1
    assert roots[0]["parameter"] == 0

    summary = cg.call_summaries(source)[0]
    assert summary["memory_effects_complete"]
    assert {
        (effect["parameter"], effect["kind"], tuple(effect["path"]))
        for effect in summary["memory_effects"]
    } >= {
        (0, "write", (".left",)),
        (0, "read", (".values", "[*]")),
    }


def test_known_callee_effects_are_instantiated_on_caller_regions():
    source = (
        "struct S { int x; }; "
        "void touch(struct S *p) { p->x = 1; } "
        "int caller(void) { struct S s; s.x = 0; touch(&s); return s.x; }"
    )
    caller = next(flow for flow in cg.data_flow(source) if flow["name"] == "caller")
    call_argument = caller["call_memory_arguments"][0]
    assert caller["memory_complete"]
    assert call_argument == {
        "cfg_node": call_argument["cfg_node"],
        "argument": 0,
        "targets": [0],
        "parameter_origins": [],
        "complete": True,
        "start": source.index("touch(&s)"),
        "end": source.index("touch(&s)") + len("touch(&s)"),
    }
    assert any(
        definition["kind"] == "call_effect"
        for definition in caller["memory_definitions"]
    )
    assert not any(
        definition["kind"] == "call_clobber"
        for definition in caller["memory_definitions"]
    )
