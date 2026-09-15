"""Opaque value expressions must not produce conclusive negative answers."""

import cindergraph as cg
import pytest


@pytest.mark.parametrize(
    "expression",
    [
        "_Generic(x, int: id(x), default: 0)",
        "__builtin_choose_expr(1, id(x), 0)",
        "__builtin_va_arg(x, int)",
    ],
)
def test_opaque_value_builtin_fails_closed(expression: str) -> None:
    code = f"int id(int x){{return x;}} int f(int x){{return {expression};}}"
    flow = next(item for item in cg.data_flow(code) if item["name"] == "f")
    summary = next(item for item in cg.call_summaries(code) if item["name"] == "f")
    assert not flow["effects_complete"]
    assert not summary["complete"]


@pytest.mark.parametrize(
    "expression",
    [
        "__builtin_types_compatible_p(int, long)",
        "__builtin_offsetof(struct s, field)",
    ],
)
def test_type_only_builtin_does_not_taint_effect_completeness(expression: str) -> None:
    flow = cg.data_flow(f"int f(int x){{return x + {expression};}}")[0]
    assert flow["effects_complete"]


@pytest.mark.parametrize(
    "expression",
    [
        "__builtin_expect(x,1)",
        "__builtin_expect_with_probability(x,1,0.9)",
        "__builtin_bswap32(x)",
        "__builtin_popcount(x)",
        "__builtin_clz(x)",
    ],
)
def test_pure_value_builtin_has_complete_parameter_flow(expression: str) -> None:
    summary = cg.call_summaries(f"int f(unsigned x){{return {expression};}}")[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


@pytest.mark.parametrize(
    "expression",
    [
        "__builtin_not_a_real_intrinsic(x)",
        "__builtin_add_overflow(x,1,&out)",
        "__builtin_expect(x)",
        "__builtin_expect(x,1,2)",
    ],
)
def test_unknown_or_mutating_builtin_remains_incomplete(expression: str) -> None:
    code = f"int f(unsigned x){{unsigned out=0;return {expression};}}"
    assert not cg.call_summaries(code)[0]["complete"]


@pytest.mark.parametrize("operand", ["x", "x++", "opaque(x)"])
def test_builtin_constant_p_operand_is_unevaluated(operand: str) -> None:
    code = f"int f(int x){{return __builtin_constant_p({operand});}}"
    flow = cg.data_flow(code)[0]
    assert not any(use["name"] == "x" for use in flow["uses"])
    assert [item["kind"] for item in flow["definitions"] if item["name"] == "x"] == [
        "parameter"
    ]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == []


def test_sizeof_vla_type_evaluates_bound_but_alignof_does_not() -> None:
    sizeof = cg.data_flow("int f(int n){int z=sizeof(int[n++]);return n+z*0;}")[0]
    assert [item["kind"] for item in sizeof["definitions"] if item["name"] == "n"] == [
        "parameter",
        "inc_dec",
    ]
    assert sum(use["name"] == "n" for use in sizeof["uses"]) == 2

    alignof = cg.data_flow("int f(int n){int z=_Alignof(int[n++]);return n+z*0;}")[0]
    assert [item["kind"] for item in alignof["definitions"] if item["name"] == "n"] == [
        "parameter"
    ]
    assert sum(use["name"] == "n" for use in alignof["uses"]) == 1

    pointer = cg.data_flow("int f(int n){int z=sizeof(int (*)[n++]);return n+z*0;}")[0]
    assert any(
        item["name"] == "n" and item["kind"] == "inc_dec"
        for item in pointer["definitions"]
    )
    assert not pointer["vla_complete"]

    for expression in ["sizeof(int[sizeof(n++)])", "sizeof(int[_Alignof(int[n++])])"]:
        flow = cg.data_flow(f"int f(int n){{int z={expression};return n+z*0;}}")[0]
        assert [
            item["kind"] for item in flow["definitions"] if item["name"] == "n"
        ] == ["parameter"]
        assert sum(use["name"] == "n" for use in flow["uses"]) == 1


@pytest.mark.parametrize("bound", ["s.n", "p->n"])
@pytest.mark.parametrize("context", ["int values[BOUND];", "int z=sizeof(int[BOUND]);"])
def test_field_name_in_vla_bound_is_not_local_value(bound: str, context: str) -> None:
    statement = context.replace("BOUND", bound)
    code = (
        "struct S{int n;};int f(int n,struct S s,struct S *p){"
        f"{statement}return n;}}"
    )
    flow = cg.data_flow(code)[0]
    assert sum(use["name"] == "n" for use in flow["uses"]) == 1
    assert any(
        use["name"] == ("s" if bound.startswith("s") else "p") for use in flow["uses"]
    )
    assert not flow["vla_complete"]


@pytest.mark.parametrize(
    ("parameters", "bound"),
    [
        ("int n,int (*size)(int)", "size(n)"),
        ("int n,int *p", "p[n]"),
        ("int n,int *p", "*p+n"),
    ],
)
def test_opaque_vla_bound_effects_fail_closed(parameters: str, bound: str) -> None:
    code = f"int f({parameters}){{int values[{bound}];return n;}}"
    flow = cg.data_flow(code)[0]
    assert not flow["vla_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


@pytest.mark.parametrize("bound", ["n+1", "n*2", "(n+3)/2"])
def test_scalar_vla_bound_arithmetic_remains_complete(bound: str) -> None:
    flow = cg.data_flow(f"int f(int n){{int values[{bound}];return n;}}")[0]
    assert flow["vla_complete"]


@pytest.mark.parametrize(
    "code",
    [
        "typedef int T;int f(int n){int values[(T)(n)];return n;}",
        "int f(int n){typedef int T;int values[(T)n];return n;}",
    ],
)
def test_typedef_cast_in_vla_bound_is_type_syntax(code: str) -> None:
    flow = cg.data_flow(code)[0]
    assert flow["vla_complete"]
    assert sum(use["name"] == "n" for use in flow["uses"]) == 2
    assert not any(binding["name"] == "T" for binding in flow["bindings"])


@pytest.mark.parametrize(
    "code",
    [
        "enum { N=4 };int f(void){int values[N];return N;}",
        "int f(void){enum { N=4,M=N+1 };int values[M];return N;}",
    ],
)
def test_enumerator_is_not_a_runtime_value_dependency(code: str) -> None:
    flow = cg.data_flow(code)[0]
    assert flow["vla_complete"]
    assert not flow["unresolved_uses"]
    assert not any(binding["is_unresolved"] for binding in flow["bindings"])
    assert not any(use["name"] in {"N", "M"} for use in flow["uses"])


def test_parameter_type_enumerator_is_visible_to_later_bounds_and_body() -> None:
    code = "int f(enum {N=2} x,int a[N]){return N+a[0]+x;}"
    flow = cg.data_flow(code)[0]
    assert flow["vla_complete"]
    assert not flow["unresolved_uses"]
    assert not any(use["name"] == "N" for use in flow["uses"])


def test_typeof_variably_modified_type_evaluates_bound() -> None:
    code = "int f(int n){typeof(int[n++]) *p=0;return n+(p!=0);}"
    flow = cg.data_flow(code)[0]
    assert not flow["vla_complete"]
    assert [
        definition["kind"]
        for definition in flow["definitions"]
        if definition["name"] == "n"
    ] == ["parameter", "inc_dec"]
    assert sum(use["name"] == "n" for use in flow["uses"]) == 2


def test_typeof_captured_vla_preserves_bound_provenance() -> None:
    code = "int f(int n){int a[n];typeof(a) b;return sizeof(b);}"
    flow = cg.data_flow(code)[0]
    assert flow["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]
    assert not flow["semantic_issues"]
    assert flow["function_id"] == 0
    assert flow["analysis_revision"] == 5
    assert flow["source_id"].endswith(f"-{len(code.encode())}")


def test_typeof_vla_capture_uses_the_value_at_type_formation() -> None:
    before = "int f(int n){int a[n];typeof(a) b;n=1;return sizeof(b);}"
    after = "int f(int n){n=1;int a[n];typeof(a) b;return sizeof(b);}"
    expected = {"parameter": 0, "sink": "return", "sink_parameter": None}
    before_summary = cg.call_summaries(before)[0]
    after_summary = cg.call_summaries(after)[0]
    assert before_summary["complete"] and expected in before_summary["flows"]
    assert after_summary["complete"] and expected not in after_summary["flows"]


@pytest.mark.parametrize(
    "statement",
    [
        '__asm__("mov %1, %0" : "=r"(y) : "r"(x));',
        'asm volatile("" : : "r"(x) : "memory");',
        '__asm__("nop");',
    ],
)
def test_inline_assembly_fails_effect_completeness(statement: str) -> None:
    code = f"int f(int x){{int y=0;{statement}return y;}}"
    flow = cg.data_flow(code)[0]
    summary = cg.call_summaries(code)[0]
    assert not flow["effects_complete"]
    assert not summary["complete"]
    issues = [
        issue
        for issue in flow["semantic_issues"]
        if issue["kind"] == "unmodeled_effect"
    ]
    assert len(issues) == 1
    assert issues[0]["start"] is not None and issues[0]["end"] is not None
    affected = code.encode()[issues[0]["start"] : issues[0]["end"]].decode()
    assert "asm" in affected


@pytest.mark.parametrize(
    "operand",
    [
        "+_Generic(x, int: x, default: 0)",
        "+__builtin_choose_expr(1, x, 0)",
        "+__builtin_va_arg(x, int)",
        '+({ __asm__("nop"); x; })',
    ],
)
def test_opaque_effect_inside_sizeof_is_unevaluated(operand: str) -> None:
    flow = cg.data_flow(f"int f(int x){{return sizeof({operand});}}")[0]
    assert flow["effects_complete"]
    assert not any(
        issue["kind"] == "unmodeled_effect" for issue in flow["semantic_issues"]
    )


@pytest.mark.parametrize("spelling", ["cleanup", "__cleanup__"])
def test_cleanup_attribute_fails_closed_while_inert_attribute_does_not(
    spelling: str,
) -> None:
    cleanup = (
        "void wipe(int *);"
        f"int f(int x){{int y __attribute__(({spelling}(wipe)))=x;return 0;}}"
    )
    flow = cg.data_flow(cleanup)[0]
    assert not flow["effects_complete"]
    assert not cg.call_summaries(cleanup)[0]["complete"]

    inert = "int f(int x){int y __attribute__((unused))=x;return y;}"
    assert cg.data_flow(inert)[0]["effects_complete"]


@pytest.mark.parametrize(
    ("expression", "flows"),
    [
        ("0 ? x : 2", False),
        ("1 ? 2 : x", False),
        ("0 && x", False),
        ("1 || x", False),
        ("1 ? x : 2", True),
        ("0 ? 2 : x", True),
        ("1 && x", True),
        ("0 || x", True),
    ],
)
def test_literal_control_excludes_statically_unevaluated_arms(
    expression: str, flows: bool
) -> None:
    summary = cg.call_summaries(f"int f(int x){{return {expression};}}")[0]
    assert summary["complete"]
    assert bool(summary["flows"]) is flows


@pytest.mark.parametrize(
    "body",
    [
        "if(0) opaque(x); return x;",
        "if(1) return x; else opaque(x);",
        "while(0) opaque(x); return x;",
        "for(;0;) opaque(x); return x;",
    ],
)
def test_literal_dead_statements_do_not_add_phantom_calls(body: str) -> None:
    summary = cg.call_summaries(f"int f(int x){{{body}}}")[0]
    assert summary["complete"]
    assert summary["flows"]


def test_false_for_condition_skips_step_but_not_initializer() -> None:
    dead_step = "int f(int x){for(;0;opaque(x)){}return x;}"
    summary = cg.call_summaries(dead_step)[0]
    assert summary["complete"]
    assert summary["flows"]

    live_initializer = "int f(int x){for(opaque(x);0;){}return x;}"
    assert not cg.call_summaries(live_initializer)[0]["complete"]


@pytest.mark.parametrize(
    "body",
    [
        "return x; opaque(x);",
        "goto done; opaque(x); done: return x;",
        "if(x)return x;else return 0;opaque(x);",
    ],
)
def test_structurally_unreachable_calls_do_not_taint_summary(body: str) -> None:
    summary = cg.call_summaries(f"int f(int x){{{body}}}")[0]
    assert summary["complete"]
    assert summary["flows"]


@pytest.mark.parametrize(
    "nested",
    [
        "if(opaque()){x=1;live:x=2;}",
        "if(opaque()){earlier:x=1;live:x=2;}",
    ],
)
def test_goto_into_nested_statement_only_revives_suffix_from_label(
    nested: str,
) -> None:
    code = f"int f(void){{int x=0;goto live;{nested}return x;}}"
    flow = cg.data_flow(code)[0]
    assert [definition["kind"] for definition in flow["definitions"]] == [
        "declaration",
        "assignment",
    ]
    assert cg.call_summaries(code)[0]["complete"]


def test_structurally_unreachable_assembly_does_not_taint_effects() -> None:
    code = 'int f(void){return 0;__asm__("nop");}'
    flow = cg.data_flow(code)[0]
    assert flow["effects_complete"]
    assert cg.call_summaries(code)[0]["complete"]


def test_unreachable_bare_vla_and_cleanup_declarations_add_no_effects() -> None:
    vla = cg.data_flow("int f(int x){return 0;int dead[x];}")[0]
    assert not any(use["name"] == "x" for use in vla["uses"])
    assert vla["vla_complete"]

    cleanup = (
        "void wipe(int *);int f(void){return 0;"
        "int dead __attribute__((cleanup(wipe)));}"
    )
    assert cg.data_flow(cleanup)[0]["effects_complete"]


@pytest.mark.parametrize("prefix", ["int dead[n];", "{int dead[n];}"])
def test_switch_dispatch_skips_bare_vla_before_first_case(prefix: str) -> None:
    code = f"int f(int n){{switch(n){{{prefix}case 1:return n;}}return 0;}}"
    flow = cg.data_flow(code)[0]
    assert sum(use["name"] == "n" for use in flow["uses"]) == 2
    assert flow["vla_complete"]


def test_switch_dispatch_skips_cleanup_before_first_case() -> None:
    code = (
        "void wipe(int*);int f(int n){switch(n){"
        "int dead __attribute__((cleanup(wipe)));default:return n;}}"
    )
    assert cg.data_flow(code)[0]["effects_complete"]


@pytest.mark.parametrize(
    ("prefix", "expected_uses"),
    [("", 2), ("if(0)goto live;", 2), ("if(n)goto live;", 4)],
)
def test_only_reachable_goto_enters_ordinary_label_before_case(
    prefix: str, expected_uses: int
) -> None:
    code = (
        f"int f(int n){{{prefix}switch(n){{int a[n];live:int b[n];"
        "case 1:return n;}return 0;}"
    )
    flow = cg.data_flow(code)[0]
    assert sum(use["name"] == "n" for use in flow["uses"]) == expected_uses


def test_unreachable_switch_prefix_goto_cannot_revive_its_own_label() -> None:
    code = (
        "int f(int n){switch(n){goto live;int a[n];live:int b[n];"
        "case 1:return n;}return 0;}"
    )
    flow = cg.data_flow(code)[0]
    assert sum(use["name"] == "n" for use in flow["uses"]) == 2


def test_preprocessor_alternative_after_return_remains_possible() -> None:
    code = "int f(int x){\n#if FIRST\nreturn x;\n#else\nopaque(x);return x;\n#endif\n}"
    assert not cg.call_summaries(code)[0]["complete"]

    configured_conditions = (
        "int f(int x){\n#if FIRST\nif(0)opaque(x);\n"
        "#else\nif(1)opaque(x);\n#endif\nreturn x;\n}"
    )
    flow = cg.data_flow(configured_conditions)[0]
    assert sum(use["name"] == "x" for use in flow["uses"]) == 3


@pytest.mark.parametrize(
    "loop",
    [
        "while(1){}",
        "for(;;){}",
        "do{}while(1);",
        "while(1){if(0)break;}",
        "for(;;){if(0)break;}",
        "do{if(0)break;}while(1);",
        "while(1){return 0;break;}",
        "for(;;){return 0;break;}",
        "do{return 0;break;}while(1);",
    ],
)
def test_statements_after_non_exiting_loop_do_not_taint_summary(loop: str) -> None:
    code = f"int f(int x){{{loop}opaque(x);return x;}}"
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == []


def test_reachable_break_keeps_loop_tail_executable() -> None:
    code = "int f(int x){while(1){if(1)break;}opaque(x);return x;}"
    assert not cg.call_summaries(code)[0]["complete"]


@pytest.mark.parametrize(
    "body",
    [
        "while(1){}done:opaque(x);return x;",
        "while(1){if(0)goto done;}done:opaque(x);return x;",
    ],
)
def test_unreachable_label_does_not_revive_non_fallthrough_tail(body: str) -> None:
    assert cg.call_summaries(f"int f(int x){{{body}}}")[0]["complete"]


def test_executable_goto_revives_label_after_non_fallthrough_loop() -> None:
    code = "int f(int x){if(x)goto done;while(1){}done:opaque(x);return x;}"
    assert not cg.call_summaries(code)[0]["complete"]
