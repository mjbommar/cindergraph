"""Outer parameter identity must not leak from nested declarator syntax."""

import cindergraph as cg
import pytest


@pytest.mark.parametrize(
    ("declaration", "expected_names"),
    [
        ("int (*cb)(const char *, size_t), int x", ["cb", "x"]),
        ("void (*cb)(int values[4]), int x", ["cb", "x"]),
        ("int (*cb)(int, int), int x", ["cb", "x"]),
        ("int callback(int), int x", ["callback", "x"]),
        ("int (*)(size_t), int x", ["x"]),
        ("struct point, int x", ["x"]),
        ("union value, int x", ["x"]),
        ("enum mode, int x", ["x"]),
    ],
)
def test_nested_signature_names_are_not_outer_bindings(
    declaration: str, expected_names: list[str]
) -> None:
    code = f"int f({declaration}){{return x;}}"
    function = cg.data_flow(code)[0]
    assert [binding["name"] for binding in function["bindings"]] == expected_names
    assert not function["unresolved_uses"]


def test_multidimensional_parameter_records_each_array_suffix() -> None:
    function = cg.data_flow("int f(int matrix[4][8], int x){return matrix[x][0];}")[0]
    matrix = next(
        binding for binding in function["bindings"] if binding["name"] == "matrix"
    )
    assert (matrix["pointer_depth"], matrix["array_rank"], matrix["type"]) == (
        0,
        2,
        "int[][]",
    )


def test_visible_file_typedef_disambiguates_an_unnamed_parameter() -> None:
    function = cg.data_flow(
        "typedef unsigned long size_t; int f(const size_t, int x){return x;}"
    )[0]
    assert [binding["name"] for binding in function["bindings"]] == ["x"]


def test_explicit_parameter_name_can_shadow_a_visible_typedef() -> None:
    for declaration in ("int size_t", "size_t size_t"):
        function = cg.data_flow(
            f"typedef unsigned long size_t; int f({declaration}){{return size_t;}}"
        )[0]
        assert [binding["name"] for binding in function["bindings"]] == ["size_t"]
        assert not function["unresolved_uses"]


def test_later_typedef_does_not_retroactively_change_parameter() -> None:
    function = cg.data_flow(
        "int f(size_t, int x){return x;} typedef unsigned long size_t;"
    )[0]
    assert [binding["name"] for binding in function["bindings"]] == ["size_t", "x"]


def test_typedef_only_affects_later_functions() -> None:
    functions = cg.data_flow(
        "int before(size_t, int x){return x;}"
        "typedef unsigned long size_t;"
        "int after(size_t, int x){return x;}"
    )
    assert [binding["name"] for binding in functions[0]["bindings"]] == ["size_t", "x"]
    assert [binding["name"] for binding in functions[1]["bindings"]] == ["x"]


def test_multi_declarator_typedef_names_are_visible() -> None:
    function = cg.data_flow(
        "typedef unsigned long size_t, count_t;int f(size_t, count_t, int x){return x;}"
    )[0]
    assert [binding["name"] for binding in function["bindings"]] == ["x"]


@pytest.mark.parametrize(
    "declaration", ["struct point value", "union cell value", "enum mode value"]
)
def test_named_aggregate_parameter_binds_its_declarator(declaration: str) -> None:
    function = cg.data_flow(f"int f({declaration}){{return value != 0;}}")[0]
    assert [binding["name"] for binding in function["bindings"]] == ["value"]
    assert not function["unresolved_uses"]


def test_block_scope_typedef_is_type_identity_not_value_identity() -> None:
    function = cg.data_flow(
        "int f(void){typedef unsigned long word_t;word_t x=1;return sizeof(word_t)+x;}"
    )[0]
    assert [binding["name"] for binding in function["bindings"]] == ["x"]
    assert [use["name"] for use in function["uses"]] == ["x"]
    assert not function["unresolved_uses"]


def test_block_scope_typedef_visibility_ends_with_scope() -> None:
    function = cg.data_flow(
        "int f(void){{typedef unsigned long word_t;}return word_t;}"
    )[0]
    assert [
        (binding["name"], binding["is_unresolved"]) for binding in function["bindings"]
    ] == [("word_t", True)]
    assert len(function["unresolved_uses"]) == 1


def test_visible_file_typedef_is_a_type_in_sizeof() -> None:
    function = cg.data_flow(
        "typedef unsigned long word_t;int f(void){return sizeof(word_t);}"
    )[0]
    assert not function["bindings"]
    assert not function["uses"]
    assert not function["unresolved_uses"]


def test_direct_vla_extent_reaches_its_size() -> None:
    code = "int f(int n){int values[n];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    uses = [use for use in function["uses"] if use["name"] == "n"]
    assert len(uses) == 2
    assert uses[0]["start"] < uses[1]["start"]
    assert function["vla_complete"]
    assert cg.call_summaries(code)[0]["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_vla_typedef_preserves_captured_bound_provenance() -> None:
    code = "int f(int n){typedef int values_t[n];return sizeof(values_t);}"
    function = cg.data_flow(code)[0]
    uses = [use for use in function["uses"] if use["name"] == "n"]
    assert len(uses) == 2
    assert uses[0]["start"] < uses[1]["start"]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_unresolved_array_extent_prevents_complete_negative() -> None:
    code = "int f(void){int values[EXTERNAL_BOUND];return sizeof(values);}"
    assert not cg.data_flow(code)[0]["vla_complete"]
    assert not cg.call_summaries(code)[0]["complete"]


def test_bound_call_keeps_argument_flow_but_reports_unknown_semantics() -> None:
    code = "int opaque(int); int f(int n){int values[opaque(n)];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert not function["vla_complete"]
    assert not function["effects_complete"]
    assert {issue["kind"] for issue in function["semantic_issues"]} >= {
        "unmodeled_type_value",
        "unmodeled_effect",
    }
    summary = cg.call_summaries(code)[0]
    assert not summary["complete"]
    assert {"parameter": 0, "sink": "return", "sink_parameter": None} in summary[
        "flows"
    ]


def test_nested_bound_call_keeps_flow_and_unknown_semantics() -> None:
    code = (
        "int opaque(int);int f(int n){int values[opaque(n + 1) * 2];"
        "return sizeof(values);}"
    )
    function = cg.data_flow(code)[0]
    assert not function["vla_complete"]
    assert not function["effects_complete"]
    summary = cg.call_summaries(code)[0]
    assert not summary["complete"]
    assert {"parameter": 0, "sink": "return", "sink_parameter": None} in summary[
        "flows"
    ]


def test_binary_bound_preserves_both_argument_flows() -> None:
    code = "int f(int n,int m){int values[n + m];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    assert not function["semantic_issues"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None},
        {"parameter": 1, "sink": "return", "sink_parameter": None},
    ]


def test_nested_binary_bound_preserves_leaf_flows() -> None:
    code = (
        "int f(int n,int m){int values[((n + 3) * (m - 1)) / 2];return sizeof(values);}"
    )
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None},
        {"parameter": 1, "sink": "return", "sink_parameter": None},
    ]


def test_unary_bound_preserves_nested_operand_flow() -> None:
    code = "int f(int n){int values[+(n + 1)];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_pure_short_circuit_bound_preserves_possible_flows() -> None:
    code = "int f(int n,int m){int values[n && (m + 1)];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None},
        {"parameter": 1, "sink": "return", "sink_parameter": None},
    ]


@pytest.mark.parametrize(
    "bound", ["sizeof(n)", "sizeof(n + 1)", "_Alignof(n)", "sizeof n"]
)
def test_unevaluated_array_extent_operand_does_not_read_parameter(bound: str) -> None:
    code = f"int f(int n){{int values[{bound}];return sizeof(values);}}"
    function = cg.data_flow(code)[0]
    assert not [use_ for use_ in function["uses"] if use_["name"] == "n"]
    assert function["vla_complete"]
    assert not cg.call_summaries(code)[0]["flows"]


def test_sizeof_vla_type_in_an_array_extent_evaluates_its_bound() -> None:
    code = "int f(int n){int values[sizeof(int[n])];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert [use_["name"] for use_ in function["uses"]].count("n") == 2
    assert function["vla_complete"]


@pytest.mark.parametrize(
    "bound",
    ["sizeof(int[n])", "sizeof(const int[n])", "sizeof(n) + n", "sizeof n + n"],
)
def test_array_extent_records_formation_and_captured_consumption(bound: str) -> None:
    code = f"int f(int n){{int values[{bound}];return sizeof(values);}}"
    function = cg.data_flow(code)[0]
    assert [use_["name"] for use_ in function["uses"]].count("n") == 2


def test_alignof_vla_type_does_not_evaluate_its_bound() -> None:
    code = "int f(int n){int values[_Alignof(int[n])];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert not [use_ for use_ in function["uses"] if use_["name"] == "n"]


@pytest.mark.parametrize(
    "bound",
    [
        "_Generic((n), int: 4, default: 8)",
        "_Generic((n), int: n, default: 8)",
        "_Generic((n), int: 4, default: n)",
        "_Generic((n), int: n, default: n + 1)",
    ],
)
def test_generic_array_extent_fails_closed_without_definite_reads(bound: str) -> None:
    code = f"int f(int n){{int values[{bound}];return sizeof(values);}}"
    function = cg.data_flow(code)[0]
    assert not [use_ for use_ in function["uses"] if use_["name"] == "n"]
    assert not function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert not summary["complete"] and not summary["flows"]
    assert cg.reaches(code, "f", 0, "return") == "unknown"


def test_assignment_in_array_extent_kills_prior_parameter_value() -> None:
    code = "int f(int n){int values[n = 4];return n + sizeof(values) * 0;}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    assert [
        definition["kind"]
        for definition in function["definitions"]
        if definition["name"] == "n"
    ] == [
        "parameter",
        "assignment",
    ]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"] and not summary["flows"]


@pytest.mark.parametrize("bound", ["n++", "++n"])
def test_read_modify_write_array_extent_retains_parameter_flow(bound: str) -> None:
    code = f"int f(int n){{int values[{bound}];return n + sizeof(values) * 0;}}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    assert (
        len(
            [
                definition
                for definition in function["definitions"]
                if definition["name"] == "n"
            ]
        )
        == 2
    )
    assert cg.call_summaries(code)[0]["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_compound_assignment_array_extent_has_read_before_write_semantics() -> None:
    code = "int f(int n){int values[n += 1];return n + sizeof(values) * 0;}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None}
    ]


def test_comma_array_extent_separates_effects_from_captured_value() -> None:
    code = "int f(int n,int m){int values[(n++,m)];return sizeof(values);}"
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    assert any(
        definition["name"] == "n" and definition["kind"] == "inc_dec"
        for definition in function["definitions"]
    )
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 1, "sink": "return", "sink_parameter": None}
    ]

    constant = "int f(int n){int values[(n++,4)];return sizeof(values);}"
    function = cg.data_flow(constant)[0]
    assert function["vla_complete"]
    assert any(
        definition["name"] == "n" and definition["kind"] == "inc_dec"
        for definition in function["definitions"]
    )
    summary = cg.call_summaries(constant)[0]
    assert summary["complete"] and not summary["flows"]


def test_conditional_array_extent_exposes_condition_and_possible_values() -> None:
    code = (
        "int f(int condition,int left,int right){"
        "int values[condition?left:right];return sizeof(values);}"
    )
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None},
        {"parameter": 1, "sink": "return", "sink_parameter": None},
        {"parameter": 2, "sink": "return", "sink_parameter": None},
    ]


def test_conditional_expression_arms_preserve_nested_flows() -> None:
    code = (
        "int f(int c,int n,int m){int values[c ? (n + 1) : +(m * 2)];"
        "return sizeof(values);}"
    )
    function = cg.data_flow(code)[0]
    assert function["vla_complete"]
    summary = cg.call_summaries(code)[0]
    assert summary["complete"]
    assert summary["flows"] == [
        {"parameter": 0, "sink": "return", "sink_parameter": None},
        {"parameter": 1, "sink": "return", "sink_parameter": None},
        {"parameter": 2, "sink": "return", "sink_parameter": None},
    ]


def test_assignment_from_other_parameter_in_array_extent_transfers_flow() -> None:
    code = "int f(int n,int m){int values[(n = m)];return n + sizeof(values) * 0;}"
    summary = cg.call_summaries(code)[0]
    assert summary["flows"] == [
        {"parameter": 1, "sink": "return", "sink_parameter": None}
    ]


@pytest.mark.parametrize(
    "declaration",
    [
        "int values[n]",
        "int values[static n]",
        "int (*values)[n]",
        "int (values[n])",
        "int ((values[n]))",
        "int (*values[n])(void)",
    ],
)
def test_parameter_vla_extent_reads_prior_parameter(declaration: str) -> None:
    function = cg.data_flow(f"int f(int n,{declaration}){{return values != 0;}}")[0]
    assert [use["name"] for use in function["uses"]].count("n") == 1
    assert function["vla_complete"]


def test_nested_function_pointer_array_extent_is_not_an_outer_use() -> None:
    function = cg.data_flow(
        "int f(int n,void (*callback)(int nested[n])){callback(0);return n;}"
    )[0]
    assert [use["name"] for use in function["uses"]].count("n") == 1


def test_parameter_vla_bounds_preserve_rank_parentheses_and_source_order() -> None:
    function = cg.data_flow(
        "int f(int n,int m,int values[(n)][m]){return values != 0;}"
    )[0]
    assert [use["name"] for use in function["uses"]].count("n") == 1
    assert [use["name"] for use in function["uses"]].count("m") == 1
    assert function["vla_complete"]

    later = cg.data_flow("int f(int values[n],int n){return values != 0;}")[0]
    assert not later["vla_complete"]
    assert not [use_ for use_ in later["uses"] if use_["name"] == "n"]
