//! Tests for the reaching-definitions analysis.
//!
//! Each of the corpus assertions at the end is a bound that a real
//! over-report tripped during development, kept so the same class of mistake
//! fails here rather than shipping as a plausible-looking number.

use super::*;

fn one(text: &str) -> DataFlow {
    analyze(text).into_parts().0.into_iter().next().unwrap()
}

#[test]
fn only_an_untransformed_call_result_is_marked_directly_returned() {
    let direct = one("int g(int); int f(int x) { return g(x); }");
    assert_eq!(direct.calls.len(), 1);
    assert!(direct.calls[0].result_is_returned);

    let parenthesized = one("int g(int); int f(int x) { return ((g(x))); }");
    assert_eq!(parenthesized.calls.len(), 1);
    assert!(parenthesized.calls[0].result_is_returned);

    for source in [
        "int g(int); int f(int x) { return g(x) + 1; }",
        "int g(int); int f(int x) { return +g(x); }",
        "int g(int); int f(int x) { return (int)g(x); }",
        "int g(int); int f(int x) { return (g(x), 0); }",
        "int *g(int); int f(int x) { return g(x)[0]; }",
    ] {
        let flow = one(source);
        assert_eq!(flow.calls.len(), 1, "{source}");
        assert!(!flow.calls[0].result_is_returned, "{source}");
    }

    let binary = one("int g(int); int h(int); int f(int x) { return g(x) + h(x); }");
    assert_eq!(binary.calls.len(), 2);
    assert!(binary.calls.iter().all(|call| !call.result_is_returned));
}

#[test]
fn every_call_suffix_is_recorded_and_an_outer_indirect_call_is_incomplete() {
    let source = "int factory(void); int f(void) { return factory()(); }";
    let flow = one(source);
    assert_eq!(flow.calls.len(), 2, "{:?}", flow.calls);
    assert_eq!(flow.calls[0].callee.as_deref(), Some("factory"));
    assert!(!flow.calls[0].result_is_returned);
    assert_eq!(flow.calls[1].callee, None);
    assert!(flow.calls[1].result_is_returned);

    let summaries = summarize(&analyze(source).into_parts().0);
    assert!(!summaries.get("f").expect("f").complete);
}

#[test]
fn redundant_parentheses_preserve_a_known_direct_callee() {
    let source = "int id(int x) { return x; } int f(int y) { return ((id))(y); }";
    let flows = analyze(source).into_parts().0;
    let f = flows.iter().find(|flow| flow.name == "f").expect("f");
    assert_eq!(f.calls.len(), 1);
    assert_eq!(f.calls[0].callee.as_deref(), Some("id"));
    assert!(f.calls[0].result_is_returned);

    let summaries = summarize(&flows);
    let f = summaries.get("f").expect("f summary");
    assert!(f.complete);
    assert!(f.flows_to(0, Sink::Return));
}

#[test]
fn a_prototyped_parenthesized_callee_is_direct_but_a_function_pointer_is_not() {
    for prototype in [
        "extern int id(int);",
        "extern int *id(int);",
        "extern int (*id(int))(int);",
    ] {
        let declared = one(&format!("{prototype} int f(int y) {{ return ((id))(y); }}"));
        assert_eq!(declared.calls.len(), 1, "{prototype}");
        assert_eq!(
            declared.calls[0].callee.as_deref(),
            Some("id"),
            "{prototype}"
        );
        assert!(declared.calls[0].result_is_returned, "{prototype}");
        let summaries = summarize(&[declared]);
        assert_eq!(
            interproc::reaches(&summaries, "f", 0, "id"),
            Flow::Yes,
            "{prototype}"
        );
    }

    for declaration in [
        "extern int (*callback)(int);",
        "extern int (*callback[2])(int);",
    ] {
        let pointer = one(&format!(
            "{declaration} int f(int y) {{ return (callback)(y); }}"
        ));
        assert_eq!(pointer.calls.len(), 1, "{declaration}");
        assert_eq!(pointer.calls[0].callee, None, "{declaration}");
    }
}

#[test]
fn address_and_dereference_of_a_known_function_preserve_direct_call_identity() {
    for callee in ["(&id)", "(*id)", "(*&id)", "(&*id)"] {
        let source = format!("extern int id(int); int f(int y) {{ return {callee}(y); }}");
        let flow = one(&source);
        assert_eq!(flow.calls.len(), 1, "{callee}");
        assert_eq!(flow.calls[0].callee.as_deref(), Some("id"), "{callee}");
        assert!(flow.calls[0].result_is_returned, "{callee}");
        assert_eq!(
            interproc::reaches(&summarize(&[flow]), "f", 0, "id"),
            Flow::Yes,
            "{callee}"
        );
    }

    let source = concat!(
        "int id(int x) { return x; }",
        "int f(int (*id)(int), int y) { return (*id)(y); }",
    );
    let flows = analyze(source).into_parts().0;
    let shadowed = flows.iter().find(|flow| flow.name == "f").expect("f");
    assert_eq!(shadowed.calls.len(), 1);
    assert_eq!(shadowed.calls[0].callee, None);
}

#[test]
fn function_type_aliases_declare_direct_callees_but_pointer_aliases_do_not() {
    for declarations in [
        "typedef int Unary(int); extern Unary id;",
        "typedef int Unary(int); extern Unary (id);",
        "typedef int Unary(int); typedef Unary Alias; extern Alias id;",
    ] {
        let source = format!("{declarations} int f(int y) {{ return (id)(y); }}");
        let direct = one(&source);
        assert_eq!(direct.calls.len(), 1, "{declarations}");
        assert_eq!(
            direct.calls[0].callee.as_deref(),
            Some("id"),
            "{declarations}"
        );
        assert_eq!(
            interproc::reaches(&summarize(&[direct]), "f", 0, "id"),
            Flow::Yes,
            "{declarations}"
        );
    }

    for declarations in [
        "typedef int (*UnaryPtr)(int); extern UnaryPtr callback;",
        "typedef int Unary(int); extern Unary *callback;",
        "typedef int Unary(int); typedef Unary *UnaryPtr; extern UnaryPtr callback;",
    ] {
        let source = format!("{declarations} int f(int y) {{ return (callback)(y); }}");
        let pointer = one(&source);
        assert_eq!(pointer.calls.len(), 1, "{declarations}");
        assert_eq!(pointer.calls[0].callee, None, "{declarations}");
    }
}

#[test]
fn an_opaque_parenthesized_name_remains_conservatively_indirect() {
    let source = "int f(int x) { return (opaque)(x); }";
    let flow = one(source);
    assert_eq!(flow.calls.len(), 1);
    assert_eq!(flow.calls[0].callee, None);
    assert!(!summarize(&[flow]).get("f").expect("f").complete);
}

#[test]
fn a_local_pointer_shadowing_a_defined_function_remains_indirect() {
    let source = concat!(
        "int helper(int x) { return x; }",
        "int f(int (*helper)(int), int x) { return ((helper))(x); }",
    );
    let flows = analyze(source).into_parts().0;
    let f = flows.iter().find(|flow| flow.name == "f").expect("f");
    assert_eq!(f.calls.len(), 1);
    assert_eq!(f.calls[0].callee, None);
    assert!(!summarize(&flows).get("f").expect("f summary").complete);
}

#[test]
fn opaque_value_builtins_cannot_certify_a_negative_summary() {
    for expression in [
        "_Generic(x, int: x, default: 0)",
        "__builtin_choose_expr(1, x, 0)",
        "__builtin_va_arg(x, int)",
    ] {
        let source = format!("int f(int x) {{ return {expression}; }}");
        let flow = one(&source);
        assert!(!flow.effects_complete, "{expression}");
        let issue = flow
            .semantic_issues
            .iter()
            .find(|issue| issue.kind == SemanticIssueKind::UnmodeledEffect)
            .expect("localized effect issue");
        let span = issue.span.expect("known builtin span");
        assert!(source[span.lo as usize..span.hi as usize].contains(expression));
        assert!(
            !summarize(&[flow]).get("f").expect("f").complete,
            "{expression}"
        );
    }
}

#[test]
fn type_only_opaque_builtins_preserve_effect_completeness() {
    for expression in [
        "__builtin_types_compatible_p(int, long)",
        "__builtin_offsetof(struct s, field)",
    ] {
        let source = format!("int f(int x) {{ return x + {expression}; }}");
        assert!(one(&source).effects_complete, "{expression}");
    }
}

#[test]
fn builtin_constant_p_does_not_evaluate_its_operand() {
    use crate::csource::eval::TypeValueOp;
    use crate::csource::semantic::AnalysisUnit;

    for operand in ["x", "x++", "opaque(x)"] {
        let source = format!("int f(int x) {{ return __builtin_constant_p({operand}); }}");
        let flow = one(&source);
        assert!(
            !flow.uses.iter().any(|use_| use_.name == "x"),
            "{operand}: {:?}",
            flow.uses
        );
        assert_eq!(
            flow.definitions
                .iter()
                .filter(|definition| definition.name == "x")
                .map(|definition| definition.kind)
                .collect::<Vec<_>>(),
            vec![DefKind::Parameter],
            "{operand}: {:?}",
            flow.definitions
        );
        assert_eq!(flow.calls.len(), 1, "{operand}: {:?}", flow.calls);
        let summary = summarize(&[flow]);
        let f = summary.get("f").expect("f");
        assert!(f.complete, "{operand}: {f:?}");
        assert!(f.flows.is_empty(), "{operand}: {f:?}");
    }

    let shadowed = one(concat!(
        "int f(int x, int (*__builtin_constant_p)(int)) {",
        "return __builtin_constant_p(x); }",
    ));
    assert!(shadowed.uses.iter().any(|use_| use_.name == "x"));
    assert_eq!(shadowed.calls[0].callee, None);
    assert!(!summarize(&[shadowed]).get("f").expect("f").complete);

    let shadowed = AnalysisUnit::new(concat!(
        "int f(int x, int (*__builtin_constant_p)(int)) {",
        "return __builtin_constant_p(x); }",
    ));
    assert!(shadowed.evaluations()[0]
        .operations()
        .iter()
        .all(|operation| !matches!(operation.kind, TypeValueOp::CallScalar { .. })));
}

#[test]
fn inline_assembly_cannot_certify_absent_value_or_memory_effects() {
    for source in [
        r#"int f(int x) { int y; __asm__("mov %1, %0" : "=r"(y) : "r"(x)); return y; }"#,
        r#"int f(int x) { asm volatile("" : : "r"(x) : "memory"); return 0; }"#,
        r#"int f(void) { __asm__("nop"); return 0; }"#,
    ] {
        let flow = one(source);
        assert!(!flow.effects_complete, "{source}");
        assert!(!summarize(&[flow]).get("f").expect("f").complete);
    }
}

#[test]
fn opaque_effects_inside_fixed_sizeof_operands_do_not_execute() {
    for operand in [
        "+_Generic(x, int: x, default: 0)",
        "+__builtin_choose_expr(1, x, 0)",
        "+__builtin_va_arg(x, int)",
        r#"+({ __asm__("nop"); x; })"#,
    ] {
        let source = format!("int f(int x) {{ return sizeof({operand}); }}");
        let flow = one(&source);
        assert!(flow.effects_complete, "{operand}: {flow:#?}");
    }
}

#[test]
fn literal_short_circuit_and_conditional_arms_do_not_create_value_flows() {
    for expression in [
        "0 ? x : 2",
        "1 ? 2 : x",
        "0 && x",
        "1 || x",
        "00u && x",
        "0x1L || x",
        "!1 && x",
        "!0 || x",
        "(x, 0) && x",
    ] {
        let flow = one(&format!("int f(int x) {{ return {expression}; }}"));
        let summary = summarize(&[flow]);
        let f = summary.get("f").expect("f");
        assert!(f.complete, "{expression}");
        assert!(!f.flows_to(0, Sink::Return), "{expression}");
    }

    for expression in [
        "1 ? x : 2",
        "0 ? 2 : x",
        "1 && x",
        "0 || x",
        "!0 && x",
        "!1 || x",
    ] {
        let flow = one(&format!("int f(int x) {{ return {expression}; }}"));
        assert!(
            summarize(&[flow])
                .get("f")
                .expect("f")
                .flows_to(0, Sink::Return),
            "{expression}"
        );
    }
}

#[test]
fn calls_and_writes_in_literal_dead_arms_do_not_execute() {
    let source = concat!(
        "int id(int x) { return x; }",
        "int f(int x) { int y = 0; 0 && (y = x); return 0 ? id(x) : y; }",
    );
    let flows = analyze(source).into_parts().0;
    let f = flows.iter().find(|flow| flow.name == "f").expect("f");
    assert!(f.calls.is_empty(), "{:?}", f.calls);
    assert!(!f
        .definitions
        .iter()
        .any(|definition| { definition.name == "y" && definition.kind == DefKind::Assignment }));
    assert!(!f.uses.iter().any(|use_| use_.name == "x"));
    assert!(summarize(&flows).get("f").expect("f").complete);

    for expression in [
        r#"0 && ({ __asm__("nop"); 1; })"#,
        "1 || _Generic(x, int: x, default: 0)",
    ] {
        let flow = one(&format!("int f(int x) {{ return {expression}; }}"));
        assert!(flow.effects_complete, "{expression}: {flow:#?}");
        assert!(summarize(&[flow]).get("f").expect("f").complete);
    }
}

#[test]
fn literal_dead_statement_branches_do_not_add_calls_or_uncertainty() {
    for body in [
        "if (0) opaque(x); return x;",
        "if (1) return x; else opaque(x);",
        "while (0) opaque(x); return x;",
        "for (; 0;) opaque(x); return x;",
    ] {
        let source = format!("int f(int x) {{ {body} }}");
        let flow = one(&source);
        assert!(flow.calls.is_empty(), "{body}: {:?}", flow.calls);
        let summary = summarize(&[flow]);
        let f = summary.get("f").expect("f");
        assert!(f.complete, "{body}");
        assert!(f.flows_to(0, Sink::Return), "{body}");
    }

    let dead_effect = one(r#"int f(void) { if (0) __asm__("nop"); return 0; }"#);
    assert!(dead_effect.effects_complete, "{dead_effect:#?}");

    let dead_step = one("int f(int x) { int y=0; for (;0; y=x) {} return y; }");
    assert!(!dead_step.uses.iter().any(|use_| use_.name == "x"));
    assert!(!summarize(&[dead_step])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));

    let dead_step_call = one("int f(int x) { for (;0; opaque(x)) {} return x; }");
    assert!(
        dead_step_call.calls.is_empty(),
        "{:?}",
        dead_step_call.calls
    );
    assert!(summarize(&[dead_step_call]).get("f").expect("f").complete);

    let live_init_call = one("int f(int x) { for (opaque(x);0;) {} return x; }");
    assert_eq!(live_init_call.calls.len(), 1);
    assert!(!summarize(&[live_init_call]).get("f").expect("f").complete);
}

#[test]
fn a_label_keeps_a_literal_dead_branch_conservatively_executable() {
    let source = "int f(int x) { goto live; if (0) { live: opaque(x); } return x; }";
    let flow = one(source);
    assert_eq!(flow.calls.len(), 1, "{:?}", flow.calls);
    assert!(!summarize(&[flow]).get("f").expect("f").complete);

    let case_entry = one(concat!(
        "int f(int x) { switch (x) {",
        "while (0) { case 1: opaque(x); }",
        "} return x; }",
    ));
    assert_eq!(case_entry.calls.len(), 1, "{:?}", case_entry.calls);
    assert!(!summarize(&[case_entry]).get("f").expect("f").complete);
}

#[test]
fn goto_into_nested_statement_only_revives_the_suffix_from_the_label() {
    for nested in [
        "if (opaque()) { x = 1; live: x = 2; }",
        "if (opaque()) { earlier: x = 1; live: x = 2; }",
    ] {
        let flow = one(&format!(
            "int f(void) {{ int x = 0; goto live; {nested} return x; }}"
        ));
        assert!(flow.calls.is_empty(), "{nested}: {:?}", flow.calls);
        assert_eq!(
            flow.definitions
                .iter()
                .filter(|definition| definition.name == "x")
                .count(),
            2,
            "{nested}: {:#?}",
            flow.definitions
        );
        assert!(summarize(&[flow]).get("f").expect("f").complete);
    }
}

#[test]
fn structurally_unreachable_statements_do_not_add_events_or_uncertainty() {
    for body in [
        "return x; opaque(x);",
        "goto done; opaque(x); done: return x;",
        "if (x) return x; else return 0; opaque(x);",
    ] {
        let source = format!("int f(int x) {{ {body} }}");
        let flow = one(&source);
        assert!(flow.calls.is_empty(), "{body}: {:?}", flow.calls);
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "x").count(),
            if body.starts_with("if") { 2 } else { 1 },
            "{body}: {:?}",
            flow.uses
        );
        let summary = summarize(&[flow]);
        let f = summary.get("f").expect("f");
        assert!(f.complete, "{body}");
        assert!(f.flows_to(0, Sink::Return), "{body}");
    }

    let dead_opaque = one(concat!(
        "int f(int x) { return x;",
        "__asm__(\"nop\"); int *p=(int*)x; p[0]=x;",
        "}",
    ));
    assert!(dead_opaque.effects_complete, "{dead_opaque:#?}");
    assert!(dead_opaque.memory_complete, "{dead_opaque:#?}");

    for body in [
        "return 0; int dead[x];",
        "goto done; int dead[x]; done: return 0;",
    ] {
        let flow = one(&format!("int f(int x) {{ {body} }}"));
        assert!(!flow.uses.iter().any(|use_| use_.name == "x"), "{body}");
        assert!(flow.vla_complete, "{body}");
    }

    let dead_cleanup = one(concat!(
        "void wipe(int *); int f(void) { return 0;",
        "int dead __attribute__((cleanup(wipe)));",
        "}",
    ));
    assert!(dead_cleanup.effects_complete, "{dead_cleanup:#?}");
}

#[test]
fn switch_dispatch_skips_declarations_before_the_first_entry_label() {
    for prefix in ["int dead[n];", "{ int dead[n]; }"] {
        let flow = one(&format!(
            "int f(int n) {{ switch (n) {{ {prefix} case 1: return n; }} return 0; }}"
        ));
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "n").count(),
            2,
            "{prefix}: {:#?}",
            flow.uses
        );
        assert!(flow.vla_complete, "{prefix}: {flow:#?}");
    }

    let cleanup = one(concat!(
        "void wipe(int *); int f(int n) { switch (n) {",
        "int dead __attribute__((cleanup(wipe)));",
        "default: return n; } }",
    ));
    assert!(cleanup.effects_complete, "{cleanup:#?}");
}

#[test]
fn only_a_reachable_goto_makes_an_ordinary_switch_label_an_entry() {
    for (prefix, expected_uses) in [("", 2), ("if (0) goto live;", 2), ("if (n) goto live;", 4)] {
        let flow = one(&format!(
            "int f(int n) {{ {prefix} switch (n) {{ int a[n]; live: int b[n]; case 1: return n; }} return 0; }}"
        ));
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "n").count(),
            expected_uses,
            "{prefix}: {:#?}",
            flow.uses
        );
    }

    let self_justifying = one(concat!(
        "int f(int n) { switch (n) { goto live; int a[n];",
        "live: int b[n]; case 1: return n; } return 0; }",
    ));
    assert_eq!(
        self_justifying
            .uses
            .iter()
            .filter(|use_| use_.name == "n")
            .count(),
        2,
        "{:#?}",
        self_justifying.uses
    );
}

#[test]
fn preprocessor_alternatives_are_not_mistaken_for_post_return_dead_code() {
    let source = concat!(
        "int f(int x) {\n",
        "#if FIRST\nreturn x;\n",
        "#else\nopaque(x); return x;\n",
        "#endif\n}",
    );
    let flow = one(source);
    assert_eq!(flow.calls.len(), 1, "{:?}", flow.calls);
    assert!(!summarize(&[flow]).get("f").expect("f").complete);

    let configured_conditions = one(concat!(
        "int f(int x) {\n",
        "#if FIRST\nif (0) opaque(x);\n",
        "#else\nif (1) opaque(x);\n",
        "#endif\nreturn x;\n}",
    ));
    assert_eq!(
        configured_conditions.calls.len(),
        2,
        "preprocessing, not this layer, selects an arm: {:?}",
        configured_conditions.calls
    );

    let configured_asm = one(concat!(
        "int f(void) {\n",
        "#if FIRST\nreturn 0;\n",
        "#else\n__asm__(\"nop\"); return 0;\n",
        "#endif\n}",
    ));
    assert!(!configured_asm.effects_complete);
}

#[test]
fn statements_after_a_provably_non_exiting_loop_do_not_execute() {
    for loop_ in [
        "while (1) {}",
        "for (;;) {}",
        "do {} while (1);",
        "while (1) { if (0) break; }",
        "for (;;) { if (0) break; }",
        "do { if (0) break; } while (1);",
        "while (1) { return 0; break; }",
        "for (;;) { return 0; break; }",
        "do { return 0; break; } while (1);",
    ] {
        let source = format!("int f(int x) {{ {loop_} opaque(x); return x; }}");
        let flow = one(&source);
        assert!(flow.calls.is_empty(), "{loop_}: {:?}", flow.calls);
        assert!(!flow.uses.iter().any(|use_| use_.name == "x"), "{loop_}");
        let summary = summarize(&[flow]);
        let f = summary.get("f").expect("f");
        assert!(f.complete, "{loop_}");
        assert!(!f.flows_to(0, Sink::Return), "{loop_}");
    }
}

#[test]
fn a_possible_loop_exit_keeps_following_statements_executable() {
    for loop_ in [
        "while (1) { if (x) break; }",
        "while (1) { if (1) break; }",
        "for (;;) { if (x) goto done; }",
    ] {
        let source = format!("int f(int x) {{ {loop_} done: opaque(x); return x; }}");
        let flow = one(&source);
        assert_eq!(flow.calls.len(), 1, "{loop_}: {:?}", flow.calls);
        assert!(!summarize(&[flow]).get("f").expect("f").complete);
    }
}

#[test]
fn only_an_executable_goto_revives_a_label_after_a_non_fallthrough_loop() {
    for prefix in ["", "while (1) { if (0) goto done; }"] {
        let source = if prefix.is_empty() {
            "int f(int x) { while (1) {} done: opaque(x); return x; }".to_owned()
        } else {
            format!("int f(int x) {{ {prefix} done: opaque(x); return x; }}")
        };
        let flow = one(&source);
        assert!(flow.calls.is_empty(), "{prefix}: {:?}", flow.calls);
        assert!(summarize(&[flow]).get("f").expect("f").complete);
    }

    let live = one(concat!(
        "int f(int x) { if (x) goto done; while (1) {}",
        "done: opaque(x); return x; }",
    ));
    assert_eq!(live.calls.len(), 1, "{:?}", live.calls);
    assert!(!summarize(&[live]).get("f").expect("f").complete);
}

#[test]
fn cleanup_attributes_invalidate_effect_completeness_but_inert_attributes_do_not() {
    for spelling in ["cleanup", "__cleanup__"] {
        let source = format!(
            "void wipe(int *); int f(int x) {{ int y __attribute__(({spelling}(wipe))) = x; return 0; }}"
        );
        let cleanup = one(&source);
        assert!(!cleanup.effects_complete, "{spelling}");
        assert!(!summarize(&[cleanup]).get("f").expect("f").complete);
    }

    for attribute in ["unused", "aligned(16)", "deprecated"] {
        let source =
            format!("int f(int x) {{ int y __attribute__(({attribute})) = x; return y; }}");
        assert!(one(&source).effects_complete, "{attribute}");
    }
}

fn edge_names(flow: &DataFlow) -> Vec<String> {
    let mut names: Vec<String> = flow.edges.iter().map(|e| e.name.clone()).collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn a_definition_reaches_the_use_after_it() {
    let flow = one("int f(void) { int x = 1; return x; }");
    assert_eq!(edge_names(&flow), vec!["x"]);
    assert_eq!(flow.edges.len(), 1);
    let definition = &flow.definitions[flow.edges[0].def as usize];
    assert_eq!(definition.kind, DefKind::Declaration);
}

#[test]
fn reaching_definitions_cross_machine_word_boundaries() {
    let mut code = String::from("int f(int x){");
    for index in 0..130 {
        code.push_str(&format!("int v{index}=x+{index};"));
    }
    code.push_str("return v0+v63+v64+v127+v128+v129;}");
    let flow = one(&code);
    for name in ["v0", "v63", "v64", "v127", "v128", "v129"] {
        let use_index = flow
            .uses
            .iter()
            .rposition(|use_| use_.name == name)
            .expect("return use") as u32;
        assert_eq!(
            flow.definitions_reaching(use_index)
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            vec![name],
            "{name}"
        );
    }
}

#[test]
fn a_later_assignment_kills_an_earlier_one() {
    let flow = one("int f(void) { int x = 1; x = 2; return x; }");
    // Only the second write reaches the return.
    assert_eq!(flow.edges.len(), 1, "{:?}", flow.edges);
    let definition = &flow.definitions[flow.edges[0].def as usize];
    assert_eq!(definition.kind, DefKind::Assignment);
}

#[test]
fn both_arms_of_a_branch_reach_the_join() {
    let flow = one("int f(int c) { int x = 0; if (c) { x = 1; } else { x = 2; } return x; }");
    let reaching: Vec<&Definition> = flow
        .definitions_reaching(
            flow.uses
                .iter()
                .position(|u| u.name == "x")
                .and_then(|_| {
                    flow.uses
                        .iter()
                        .enumerate()
                        .filter(|(_, u)| u.name == "x")
                        .map(|(i, _)| i as u32)
                        .next_back()
                })
                .expect("a use of x"),
        )
        .collect();
    // The two arms reach; the initializer does not, both arms kill it.
    assert_eq!(reaching.len(), 2, "{reaching:?}");
    assert!(reaching.iter().all(|d| d.kind == DefKind::Assignment));
}

#[test]
fn a_loop_carries_a_definition_backwards() {
    // `sum` is read on an iteration that a later write reaches, which only
    // a fixpoint over the back edge finds.
    let flow = one(
        "int f(int n) { int sum = 0; for (int i = 0; i < n; i++) { sum = sum + i; } return sum; }",
    );
    let sum_uses: Vec<u32> = flow
        .uses
        .iter()
        .enumerate()
        .filter(|(_, u)| u.name == "sum")
        .map(|(i, _)| i as u32)
        .collect();
    assert!(!sum_uses.is_empty());
    // The read inside the loop sees both the initializer and the loop's
    // own write.
    let inside = sum_uses[0];
    let kinds: Vec<DefKind> = flow.definitions_reaching(inside).map(|d| d.kind).collect();
    assert!(kinds.contains(&DefKind::Declaration), "{kinds:?}");
    assert!(kinds.contains(&DefKind::Assignment), "{kinds:?}");
}

#[test]
fn a_parameter_is_a_definition_at_the_entry() {
    let flow = one("int f(int a) { return a; }");
    assert_eq!(flow.edges.len(), 1);
    let definition = &flow.definitions[flow.edges[0].def as usize];
    assert_eq!(definition.kind, DefKind::Parameter);
    assert_eq!(definition.name, "a");
}

#[test]
fn a_shadowed_declaration_is_a_different_variable() {
    // The inner `x` must not be confused with the outer one: the return
    // sees the outer write, and the inner write reaches nothing.
    let flow = one("int f(void) { int x = 1; { int x = 2; (void)x; } return x; }");
    let outer_return = flow
        .uses
        .iter()
        .enumerate()
        .filter(|(_, u)| u.name == "x")
        .map(|(i, _)| i as u32)
        .next_back()
        .expect("a use of x");
    let reaching: Vec<&Definition> = flow.definitions_reaching(outer_return).collect();
    assert_eq!(reaching.len(), 1, "{reaching:?}");
    // The one that reaches is the OUTER declaration, at the lower offset.
    assert!(reaching[0].span.lo < 30, "{:?}", reaching[0]);
}

#[test]
fn a_compound_assignment_both_reads_and_writes() {
    let flow = one("int f(int a) { int x = 1; x += a; return x; }");
    let kinds: Vec<DefKind> = flow.definitions.iter().map(|d| d.kind).collect();
    assert!(kinds.contains(&DefKind::CompoundAssignment), "{kinds:?}");
    // `x` is still read by the `+=` itself.
    assert!(flow.uses.iter().filter(|u| u.name == "x").count() >= 2);
}

#[test]
fn an_increment_both_reads_and_writes() {
    let flow = one("int f(void) { int i = 0; i++; return i; }");
    let kinds: Vec<DefKind> = flow.definitions.iter().map(|d| d.kind).collect();
    assert!(kinds.contains(&DefKind::IncDec), "{kinds:?}");
}

#[test]
fn a_read_before_any_write_is_reported_unresolved() {
    // `g` is a global: nothing in this function defines it.
    let flow = one("int f(void) { return g; }");
    assert_eq!(flow.unresolved_uses.len(), 1, "{:?}", flow.uses);
    assert_eq!(flow.uses[flow.unresolved_uses[0] as usize].name, "g");
}

#[test]
fn a_store_through_a_pointer_does_not_kill() {
    // Nothing here knows what `p` points at, so the write to `x` before it
    // must still reach the read after it. Over-approximating is the safe
    // direction and this pins it.
    let flow = one("int f(int *p) { int x = 1; *p = 2; return x; }");
    let read = flow
        .uses
        .iter()
        .enumerate()
        .filter(|(_, u)| u.name == "x")
        .map(|(i, _)| i as u32)
        .next_back()
        .expect("a use of x");
    assert_eq!(flow.definitions_reaching(read).count(), 1);
}

#[test]
fn taking_an_address_is_not_a_value_definition() {
    let code = "int f(void){int x=1;int *p=&x;return x;}";
    let flow = one(code);
    assert!(flow
        .definitions
        .iter()
        .any(|definition| definition.kind == DefKind::AddressTaken));
    assert_eq!(
        flow.definitions
            .iter()
            .filter(|definition| definition.kind == DefKind::AddressTaken)
            .count(),
        1,
        "the operation owner must replace, not duplicate, syntax promotion"
    );
    let return_use = flow
        .uses
        .iter()
        .rposition(|use_| use_.name == "x")
        .expect("return use") as u32;
    assert_eq!(
        flow.definitions_reaching(return_use)
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        vec![DefKind::Declaration]
    );

    let escaped_value = one("int f(void){int x=1;(void)&x;return 0;}");
    assert!(!escaped_value.uses.iter().any(|use_| use_.name == "x"));
    assert!(escaped_value.dead_stores.is_empty());
    assert!(escaped_value.unused_bindings().is_empty());

    // This has an escape event but no value definition at all. It exercises
    // the zero-word reaching lattice used after escape-only events are removed
    // from its compact internal index.
    let uninitialized_escape = one("int f(void){int x;(void)&x;return 0;}");
    assert_eq!(uninitialized_escape.definitions.len(), 1);
    assert_eq!(
        uninitialized_escape.definitions[0].kind,
        DefKind::AddressTaken
    );
    assert!(uninitialized_escape.edges.is_empty());
    assert!(uninitialized_escape.dead_stores.is_empty());
    assert!(uninitialized_escape.unused_bindings().is_empty());
}

#[test]
fn pointer_completeness_requires_initialization_on_every_path() {
    for source in [
        "int f(int x){int y=0;int *p;int *q;p=q;q=&y;*p=x;return y;}",
        "int f(int x){int y=0;int *p;if(x)p=&y;*p=x;return y;}",
        "int f(int x){int y=0;int *p;while(x)p=&y;*p=x;return y;}",
    ] {
        let flow = one(source);
        assert!(!flow.memory_complete, "{source}");
        assert!(!summarize(&[flow]).get("f").unwrap().complete, "{source}");
    }

    for source in [
        "int f(int x){int y=0;int *p;p=&y;*p=x;return y;}",
        "int f(int x){int y=0;int *p;if(x)p=&y;else p=&y;*p=x;return y;}",
        "int f(int x){int y=0;int *p;do p=&y;while(x);*p=x;return y;}",
    ] {
        let flow = one(source);
        assert!(flow.memory_complete, "{source}");
        assert!(summarize(&[flow]).get("f").unwrap().complete, "{source}");
    }
}

#[test]
fn discarded_values_preserve_pointer_assignment_side_effects() {
    for source in [
        "int f(int x){int y=0;int *p;(p=&y,0);*p=x;return y;}",
        "int f(int x){int y=0;int *p;x?(p=&y):(p=&y);*p=x;return y;}",
    ] {
        let flow = one(source);
        assert!(flow.memory_complete, "{source}");
        assert!(summarize(&[flow]).get("f").unwrap().complete, "{source}");
    }

    // The assignment after the access cannot retroactively initialize it.
    let source = "int f(int x){int y=0;int *p;(0,*p=x,p=&y);return y;}";
    let flow = one(source);
    assert!(!flow.memory_complete, "{source}");
    assert!(!summarize(&[flow]).get("f").unwrap().complete, "{source}");

    // Discarding the assignment's result must not hide unsupported pointer
    // arithmetic used to compute the value that was stored.
    let source = "int f(int x){int y=0;int *q=&y;int *p;((p=q+1,1),0);*p=x;return y;}";
    assert!(!one(source).memory_complete, "{source}");
}

#[test]
fn weak_and_strong_writes_on_separate_nodes_preserve_source_order() {
    for (code, expected) in [
        (
            "int f(void){int x=0;int *p=&x;*p=1;x=2;return x;}",
            vec![DefKind::Assignment],
        ),
        (
            "int f(void){int x=0;int *p=&x;x=2;*p=1;return x;}",
            vec![DefKind::Assignment, DefKind::MemoryWrite],
        ),
    ] {
        let flow = one(code);
        let return_offset = code.rfind('x').unwrap() as u32;
        let return_use = flow
            .uses
            .iter()
            .position(|use_| use_.span.lo == return_offset)
            .expect("return use") as u32;
        assert_eq!(
            flow.definitions_reaching(return_use)
                .map(|definition| definition.kind)
                .collect::<Vec<_>>(),
            expected,
            "{code}"
        );
    }
}

#[test]
fn parentheses_around_a_known_pointer_store_preserve_the_write() {
    for target in ["(*p)", "(((*p)))"] {
        let code = format!("int f(void){{int x=0;int *p=&x;{target}=1;return x;}}");
        let flow = one(&code);
        assert!(flow.memory_complete, "{code}");
        let return_offset = code.rfind('x').unwrap() as u32;
        let return_use = flow
            .uses
            .iter()
            .position(|use_| use_.span.lo == return_offset)
            .expect("return use") as u32;
        assert!(
            flow.definitions_reaching(return_use)
                .any(|definition| definition.kind == DefKind::MemoryWrite),
            "{code}"
        );
    }
}

#[test]
fn increment_through_a_known_pointer_is_a_complete_memory_read_write() {
    for operation in ["(*p)++", "++*p", "(*p)--", "--*p"] {
        let code = format!("int f(int x){{int y=x;int *p=&y;{operation};return y;}}");
        let flow = one(&code);
        assert!(flow.memory_complete, "{operation}: {flow:#?}");
        let return_offset = code.rfind('y').unwrap() as u32;
        let return_use = flow
            .uses
            .iter()
            .position(|use_| use_.span.lo == return_offset)
            .expect("return use") as u32;
        assert!(
            flow.definitions_reaching(return_use)
                .any(|definition| definition.kind == DefKind::MemoryWrite),
            "{operation}: {:#?}",
            flow.definitions
        );
        assert!(summarize(&[flow]).get("f").expect("f").complete);
    }
}

#[test]
fn pointer_arithmetic_prevents_a_complete_local_memory_claim() {
    for code in [
        "int f(void){int x=0;int *p=&x;p=p+1;*p=7;return x;}",
        "int f(int n){int x=0;int *p=&x;int *q=p+n;*q=7;return x;}",
        "int f(void){int x=0;int *p=&x;int *q=p++;*q=7;return x;}",
    ] {
        let flow = one(code);
        assert!(!flow.memory_complete, "{code}");
        assert!(flow.semantic_issues.iter().any(|issue| {
            issue.kind == SemanticIssueKind::UnknownMemoryEffect && issue.span.is_some()
        }));
    }

    let flow = one("int f(int n){int x=0;int *p=&x;int *q=p+n;*q=7;return x;}");
    assert!(flow
        .definitions
        .iter()
        .any(|definition| { definition.name == "x" && definition.kind == DefKind::MemoryWrite }));
}

#[test]
fn arithmetic_that_does_not_produce_a_pointer_does_not_taint_its_target() {
    for code in [
        "int f(int x){int a=0,b=0;int *p=(x+1)?&a:&b;*p=x;return a;}",
        "int f(int x){int a=0;int *p=(x+1,&a);*p=x;return a;}",
        "int f(int x){int a=0,b=0;int *q=&a;int *p=(q++, &b);*p=x;return b;}",
    ] {
        assert!(one(code).memory_complete, "{code}");
    }

    for code in [
        "int f(int x){int a=0;int *q=&a;int *p=q+1;*p=x;return a;}",
        "int f(int x){int a=0;int *q=&a;int *p=x?q+1:&a;*p=x;return a;}",
        "int f(int x){int a=0;int *q=&a;int *p=(x,q+1);*p=x;return a;}",
    ] {
        assert!(!one(code).memory_complete, "{code}");
    }
}

#[test]
fn a_pointer_replaced_through_a_local_double_pointer_keeps_the_new_target() {
    let code = "int f(int x){int a=0,b=0;int *p=&a;int **q=&p;*q=&b;*p=x;return b;}";
    let flow = one(code);
    assert!(flow.memory_complete, "{flow:#?}");
    let return_offset = code.rfind('b').unwrap() as u32;
    let return_use = flow
        .uses
        .iter()
        .position(|use_| use_.span.lo == return_offset)
        .expect("return use") as u32;
    assert!(
        flow.definitions_reaching(return_use).any(|definition| {
            definition.kind == DefKind::MemoryWrite && definition.name == "b"
        }),
        "{:#?}",
        flow.definitions
    );
}

#[test]
fn a_pointer_loaded_after_second_order_replacement_keeps_the_new_target() {
    let code = concat!(
        "int f(int x){int a=0,b=0;int *p=&a;int **q=&p;",
        "*q=&b;int *r=*q;*r=x;return b;}"
    );
    let flow = one(code);
    assert!(flow.memory_complete, "{flow:#?}");
    let return_offset = code.rfind('b').unwrap() as u32;
    let return_use = flow
        .uses
        .iter()
        .position(|use_| use_.span.lo == return_offset)
        .expect("return use") as u32;
    assert!(
        flow.definitions_reaching(return_use).any(|definition| {
            definition.kind == DefKind::MemoryWrite && definition.name == "b"
        }),
        "{:#?}",
        flow.definitions
    );
}

#[test]
fn second_order_pointer_values_union_exact_branches_and_retain_unknown_arithmetic() {
    let conditional = one(concat!(
        "int f(int c,int x){int a=0,b=0;int *p=&a;int **q=&p;",
        "*q=c?&a:&b;*p=x;return a+b;}",
    ));
    assert!(conditional.memory_complete, "{conditional:#?}");
    assert!(conditional
        .definitions
        .iter()
        .any(|definition| { definition.name == "a" && definition.kind == DefKind::MemoryWrite }));
    assert!(conditional
        .definitions
        .iter()
        .any(|definition| { definition.name == "b" && definition.kind == DefKind::MemoryWrite }));

    let arithmetic = one(concat!(
        "int f(int x){int a=0;int *p=&a;int **q=&p;",
        "*q=p+1;*p=x;return a;}",
    ));
    assert!(!arithmetic.memory_complete, "{arithmetic:#?}");
    assert!(arithmetic.semantic_issues.iter().any(|issue| {
        issue.kind == SemanticIssueKind::UnknownMemoryEffect && issue.span.is_some()
    }));
    assert!(arithmetic
        .definitions
        .iter()
        .any(|definition| { definition.name == "a" && definition.kind == DefKind::MemoryWrite }));
}

#[test]
fn a_pointer_loaded_through_a_known_local_double_pointer_is_complete() {
    let code = "int f(int x){int a=0;int *p=&a;int **q=&p;int *r=*q;*r=x;return a;}";
    let flow = one(code);
    assert!(flow.memory_complete, "{:#?}", flow.definitions);
    let return_offset = code.rfind('a').unwrap() as u32;
    let return_use = flow
        .uses
        .iter()
        .position(|use_| use_.span.lo == return_offset)
        .expect("return use") as u32;
    assert!(
        flow.definitions_reaching(return_use)
            .any(|definition| definition.kind == DefKind::MemoryWrite),
        "{:#?}",
        flow.definitions
    );
}

#[test]
fn integer_to_pointer_casts_prevent_a_complete_local_memory_claim() {
    for code in [
        "int f(int x){int a=0;int *p=x?&a:(int*)1;*p=x;return a;}",
        "int f(int x){int a=0;int *p=(int*)x;p=x?&a:p;*p=x;return a;}",
        "int f(int x){int a=0;int *p=x?&a:(int*)0;*p=x;return a;}",
        "int f(int x){int a=0;int *p=(int*)(x?&a:x);*p=x;return a;}",
    ] {
        assert!(!one(code).memory_complete, "{code}");
    }

    for code in [
        "int f(int x){int a=0;int *p=(int*)&a;*p=x;return a;}",
        "int f(int x){int a=0;int *p=&a;int *q=(int*)p;*q=x;return a;}",
        "int f(int x){int a=0,b=0;int *p=(int*)(x?&a:&b);*p=x;return a;}",
        "int f(int x){int a=0;int *p=(int*)(x,&a);*p=x;return a;}",
    ] {
        assert!(one(code).memory_complete, "{code}");
    }

    let known_chain = format!(
        "int f(int x){{int a=0;int *p=&a;p={}p;*p=x;return a;}}",
        "(int*)".repeat(2_048)
    );
    assert!(one(&known_chain).memory_complete);

    let integer_chain = format!(
        "int f(int x){{int a=0;int *p=x?&a:{}1;*p=x;return a;}}",
        "(int*)".repeat(128)
    );
    assert!(!one(&integer_chain).memory_complete);
}

#[test]
fn pointer_integer_pointer_round_trips_are_not_certified_as_local_aliases() {
    for initializer in [
        "(int *)(long)&a",
        "(int *)(unsigned long)(void *)&a",
        "(int *)(long)(long)(long)&a",
    ] {
        let code = format!("int f(int x){{int a=0;int *p={initializer};*p=x;return a;}}");
        assert!(!one(&code).memory_complete, "{initializer}");
    }

    let pointer_only = "int f(int x){int a=0;int *p=(int *)(void *)(char *)&a;*p=x;return a;}";
    assert!(one(pointer_only).memory_complete);
}

#[test]
fn address_of_dereference_reads_the_pointer_but_not_the_pointee() {
    let source = "int f(int x){int y=x;int *p=&y;return &*p != 0;}";
    let flow = one(source);
    assert!(flow.memory_complete, "{flow:#?}");
    assert!(flow.uses.iter().any(|read| read.name == "p"));
    assert!(!flow.uses.iter().any(|read| read.name == "y"));
    let summary = summarize(&[flow]);
    let f = summary.get("f").expect("f");
    assert!(f.complete);
    assert!(!f.flows_to(0, Sink::Return));

    let copied = one("int f(int x){int y=0;int *p=&y;int *q=&*p;*q=x;return y;}");
    assert!(copied.memory_complete, "{copied:#?}");
    assert!(summarize(&[copied])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));

    let nested = one("int f(int x){int y=x;int *p=&y;int **q=&p;return &**q != 0;}");
    assert!(nested.memory_complete, "{nested:#?}");
    let summary = summarize(&[nested]);
    let f = summary.get("f").expect("f");
    assert!(f.complete);
    assert!(!f.flows_to(0, Sink::Return));
}

#[test]
fn dereference_of_address_is_a_direct_read_or_write_of_the_object() {
    let read = one("int f(int x){int y=x;return *&y;}");
    assert!(read.memory_complete, "{read:#?}");
    assert!(read.uses.iter().any(|use_| use_.name == "y"));
    assert!(!read
        .definitions
        .iter()
        .any(|definition| definition.kind == DefKind::AddressTaken));
    assert!(summarize(&[read])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));

    let write = one("int f(int x){int y=0;*&y=x;return y;}");
    assert!(write.memory_complete, "{write:#?}");
    assert!(!write
        .definitions
        .iter()
        .any(|definition| definition.kind == DefKind::AddressTaken));
    assert!(write
        .definitions
        .iter()
        .any(|definition| { definition.name == "y" && definition.kind == DefKind::Assignment }));
    assert!(summarize(&[write])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));

    for (source, expected_kind) in [
        (
            "int f(int x){int y=1;*&y += x;return y;}",
            DefKind::CompoundAssignment,
        ),
        ("int f(int x){int y=x;return (*&y)++;}", DefKind::IncDec),
        ("int f(int x){int y=x;return ++*&y;}", DefKind::IncDec),
    ] {
        let flow = one(source);
        assert!(flow.memory_complete, "{source}: {flow:#?}");
        assert!(
            !flow.definitions.iter().any(|definition| {
                matches!(
                    definition.kind,
                    DefKind::AddressTaken | DefKind::MemoryWrite
                )
            }),
            "{source}: {flow:#?}"
        );
        assert!(
            flow.definitions
                .iter()
                .any(|definition| { definition.name == "y" && definition.kind == expected_kind }),
            "{source}: {flow:#?}"
        );
        assert!(
            summarize(&[flow])
                .get("f")
                .expect("f")
                .flows_to(0, Sink::Return),
            "{source}"
        );
    }

    for source in [
        "int f(int x){int y=x;int *p=&y;return *&*p;}",
        "int f(int x){int y=0;int *p=&y;*&*p=x;return y;}",
    ] {
        let flow = one(source);
        assert!(flow.memory_complete, "{source}: {flow:#?}");
        assert!(summarize(&[flow])
            .get("f")
            .expect("f")
            .flows_to(0, Sink::Return));
    }
}

#[test]
fn an_operation_owned_pointer_load_projects_the_pointee_once() {
    let source = "int f(int x){int y=x;int *p=&y;return *p;}";
    let flow = one(source);
    let lo = source.find("*p;").expect("dereference") as u32;
    let span = Span::new(lo, lo + 2);

    assert!(flow.memory_complete, "{flow:#?}");
    assert_eq!(
        flow.uses
            .iter()
            .filter(|use_| use_.name == "y" && use_.span == span)
            .count(),
        1,
        "the evaluation operation, not a second syntax scan, owns this load"
    );
    assert!(summarize(&[flow])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));
}

#[test]
fn operation_owned_computed_pointer_loads_preserve_may_targets_and_uncertainty() {
    let conditional = one(concat!(
        "int f(int c,int x,int y){int *p=&x;int *q=&y;",
        "return *(c?p:q);}",
    ));
    assert!(conditional.memory_complete, "{conditional:#?}");
    let summary = summarize(&[conditional]);
    let f = summary.get("f").expect("f");
    assert!(f.flows_to(1, Sink::Return));
    assert!(f.flows_to(2, Sink::Return));

    let comma = one(concat!(
        "int f(int x,int y){int *p=&x;int *q=&y;",
        "return *(p,q);}",
    ));
    assert!(comma.memory_complete, "{comma:#?}");
    let summary = summarize(&[comma]);
    let f = summary.get("f").expect("f");
    assert!(!f.flows_to(0, Sink::Return));
    assert!(f.flows_to(1, Sink::Return));

    let arithmetic = one("int f(int x){int *p=&x;return *(p+0);}");
    assert!(!arithmetic.memory_complete, "{arithmetic:#?}");
    assert!(arithmetic.semantic_issues.iter().any(|issue| {
        issue.kind == SemanticIssueKind::UnknownMemoryEffect && issue.span.is_some()
    }));
    assert!(summarize(&[arithmetic])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));
}

#[test]
fn operation_owned_computed_pointer_stores_preserve_may_targets_and_uncertainty() {
    let conditional = one(concat!(
        "int f(int c,int x){int a=0,b=0;int *p=&a;int *q=&b;",
        "*(c?p:q)=x;return a+b;}",
    ));
    assert!(conditional.memory_complete, "{conditional:#?}");
    for name in ["a", "b"] {
        assert!(
            conditional.definitions.iter().any(|definition| {
                definition.name == name && definition.kind == DefKind::MemoryWrite
            }),
            "{name}: {conditional:#?}"
        );
    }
    assert!(summarize(&[conditional])
        .get("f")
        .expect("f")
        .flows_to(1, Sink::Return));

    let comma = one(concat!(
        "int f(int x){int a=1,b=2;int *p=&a;int *q=&b;",
        "*(p,q)=x;return a+b;}",
    ));
    assert!(comma.memory_complete, "{comma:#?}");
    assert!(
        !comma.definitions.iter().any(|definition| {
            definition.name == "a" && definition.kind == DefKind::MemoryWrite
        }),
        "{comma:#?}"
    );
    assert!(
        comma.definitions.iter().any(|definition| {
            definition.name == "b" && definition.kind == DefKind::MemoryWrite
        }),
        "{comma:#?}"
    );

    let arithmetic = one("int f(int x){int a=0;int *p=&a;*(p+0)=x;return a;}");
    assert!(!arithmetic.memory_complete, "{arithmetic:#?}");
    assert!(
        arithmetic.definitions.iter().any(|definition| {
            definition.name == "a" && definition.kind == DefKind::MemoryWrite
        }),
        "{arithmetic:#?}"
    );
    assert!(summarize(&[arithmetic])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));
}

#[test]
fn an_operation_owned_pointer_store_projects_the_write_once() {
    let source = "int f(int x){int y=0;int *p=&y;*p=x;return y;}";
    let flow = one(source);
    let lo = source.rfind("*p=").expect("indirect store") as u32;
    let place = Span::new(lo, lo + 2);

    assert!(flow.memory_complete, "{flow:#?}");
    assert_eq!(
        flow.definitions
            .iter()
            .filter(|definition| {
                definition.name == "y"
                    && definition.kind == DefKind::MemoryWrite
                    && definition.span == place
            })
            .count(),
        1,
        "the StoreScalar operation, not a second syntax scan, owns this write: {:#?}",
        flow.definitions
    );
    assert!(summarize(&[flow])
        .get("f")
        .expect("f")
        .flows_to(0, Sink::Return));
}

#[test]
fn operation_owned_projected_updates_preserve_memory_flow_and_old_value_results() {
    let compound_source = "int f(int x){int y=1;int *p=&y;*p+=x;return y;}";
    let compound = one(compound_source);
    assert!(compound.memory_complete, "{compound:#?}");
    let place = compound_source.find("*p+=").expect("compound place") as u32;
    let place = Span::new(place, place + 2);
    assert_eq!(
        compound
            .uses
            .iter()
            .filter(|use_| use_.name == "y" && use_.span == place)
            .count(),
        1
    );
    assert_eq!(
        compound
            .definitions
            .iter()
            .filter(|definition| {
                definition.name == "y"
                    && definition.kind == DefKind::MemoryWrite
                    && definition.span == place
            })
            .count(),
        1
    );
    assert!(summarize(&[compound])
        .get("f")
        .expect("compound summary")
        .flows_to(0, Sink::Return));

    let postfix_source = "int f(int x){int y=x;int *p=&y;return (*p)++;}";
    let postfix = one(postfix_source);
    assert!(postfix.memory_complete, "{postfix:#?}");
    assert!(summarize(&[postfix])
        .get("f")
        .expect("postfix summary")
        .flows_to(0, Sink::Return));
}

#[test]
fn projected_field_and_element_stores_retain_inputs_and_regions() {
    for (source, expected_uses) in [
        (
            "struct S{int x;};void f(struct S *p,int v){p->x=v;}",
            &["p", "v"][..],
        ),
        (
            "struct S{int x;};void f(struct S *p,int v){p->x+=v;}",
            &["p", "v"][..],
        ),
        (
            "struct S{int x;};void f(struct S object,int v){object.x=v;}",
            &["v"][..],
        ),
        (
            concat!(
                "struct I{int x;};struct O{struct I inner;};",
                "void f(struct O object,int v){object.inner.x=v;}"
            ),
            &["v"][..],
        ),
        (
            "void f(int i,int v){int array[4];array[i]=v;}",
            &["i", "v"][..],
        ),
        ("void f(int i){int array[4];array[i]++;}", &["i"][..]),
        ("void f(int *p,int i,int v){p[i]=v;}", &["p", "i", "v"][..]),
    ] {
        let flow = one(source);
        assert!(flow.memory_complete, "{source}: {flow:#?}");
        assert!(flow.effects_complete, "{source}: {flow:#?}");
        assert!(!flow.memory_accesses.is_empty(), "{source}: {flow:#?}");
        for name in expected_uses {
            assert!(
                flow.uses.iter().any(|use_| use_.name == *name),
                "{source}: {name}"
            );
        }
    }
}

#[test]
fn pointer_parameters_have_abstract_pointee_regions() {
    let flow = one(concat!(
        "struct Pair { int left; int right; }; ",
        "int read_left(struct Pair *p) { return p->left; }",
    ));
    let pointee = flow
        .memory_regions
        .iter()
        .find(|region| {
            matches!(
                region.kind,
                MemoryRegionKind::ParameterPointee { parameter: 0, .. }
            )
        })
        .expect("formal pointee root");
    let field = flow
        .memory_regions
        .iter()
        .find(|region| {
            matches!(
                &region.kind,
                MemoryRegionKind::Field { base, member, .. }
                    if *base == pointee.id && member == "left"
            )
        })
        .expect("field below formal pointee");
    assert_eq!(
        flow.memory_region_name(field.id).as_deref(),
        Some("*p.left")
    );
    assert!(flow
        .memory_accesses
        .iter()
        .any(|access| access.region == field.id && access.kind == MemoryAccessKind::Read));
    assert!(flow.memory_definitions.iter().any(|definition| {
        definition.region == field.id && definition.kind == MemoryDefinitionKind::IncomingParameter
    }));
    assert!(flow.memory_edges.iter().any(|edge| {
        flow.memory_definitions[edge.definition as usize].region == field.id
            && flow.memory_uses[edge.use_ as usize].region == field.id
    }));
}

#[test]
fn copied_formal_pointers_retain_their_pointee_identity() {
    let flow = one(concat!(
        "struct Pair { int left; }; ",
        "int read_copy(struct Pair *p) { struct Pair *q = p; return q->left; }",
    ));
    assert_eq!(
        flow.memory_regions
            .iter()
            .filter(|region| {
                matches!(
                    region.kind,
                    MemoryRegionKind::ParameterPointee { parameter: 0, .. }
                )
            })
            .count(),
        1
    );
    assert!(flow
        .memory_regions
        .iter()
        .filter_map(|region| flow.memory_region_name(region.id))
        .any(|name| name.ends_with(".left")));
}

#[test]
fn distinct_formal_pointees_explicitly_may_alias() {
    let flow = one(concat!(
        "struct Pair { int left; }; ",
        "int exchange(struct Pair *p, struct Pair *q) { ",
        "p->left = 7; return q->left; }",
    ));
    assert!(flow
        .memory_overlaps
        .iter()
        .any(|overlap| overlap.kind == MemoryOverlapKind::ParameterAlias));
    assert!(flow
        .memory_edges
        .iter()
        .any(|edge| edge.overlap == Some(MemoryOverlapKind::ParameterAlias)));
}

#[test]
fn summaries_publish_structural_formal_pointee_effects() {
    use super::interproc::{MemoryEffectPath, ParameterMemoryEffectKind};

    let source = concat!(
        "struct Pair { int left; int values[4]; }; ",
        "int inspect(struct Pair *p, int i) { ",
        "p->left = p->values[i]; return p->left; }",
    );
    let flows = analyze(source).into_parts().0;
    assert!(flows[0].memory_complete, "{:#?}", flows[0].semantic_issues);
    let summary = summarize(&flows).get("inspect").expect("inspect").clone();
    assert!(summary.memory_effects_complete, "{summary:#?}");
    assert!(
        !summary.flows_to(0, Sink::Return),
        "the formal pointer's address is not the loaded field value: {summary:#?}"
    );
    assert!(summary.memory_effects.iter().any(|effect| {
        effect.parameter == 0
            && effect.kind == ParameterMemoryEffectKind::Write
            && effect.path == [MemoryEffectPath::Field("left".to_owned())]
    }));
    assert!(summary.memory_effects.iter().any(|effect| {
        effect.parameter == 0
            && effect.kind == ParameterMemoryEffectKind::Read
            && effect.path
                == [
                    MemoryEffectPath::Field("values".to_owned()),
                    MemoryEffectPath::Elements,
                ]
    }));
}

#[test]
fn formal_pointee_effects_compose_through_calls_and_copies() {
    use super::interproc::{MemoryEffectPath, ParameterMemoryEffectKind};

    let unit = crate::csource::semantic::AnalysisUnit::new(concat!(
        "struct Pair { int left; }; ",
        "void leaf(struct Pair *p) { p->left = 1; } ",
        "void wrapper(struct Pair *q) { struct Pair *copy=q; leaf(copy); }",
    ));
    let wrapper = unit.summaries().get("wrapper").expect("wrapper");
    assert!(wrapper.memory_effects_complete, "{wrapper:#?}");
    assert!(wrapper.complete, "{wrapper:#?}");
    assert_eq!(
        wrapper.memory_effects,
        [super::interproc::ParameterMemoryEffect {
            parameter: 0,
            path: vec![MemoryEffectPath::Field("left".to_owned())],
            kind: ParameterMemoryEffectKind::Write,
            precision: MemoryAccessPrecision::MayAlias,
        }]
    );
}

#[test]
fn known_callee_memory_effects_can_qualify_a_pointer_call_summary() {
    let unit = crate::csource::semantic::AnalysisUnit::new(concat!(
        "struct Pair { int left; }; ",
        "int inspect(struct Pair *p) { return p->left; } ",
        "void wrapper(struct Pair *q) { inspect(q); }",
    ));
    let wrapper = unit.summaries().get("wrapper").expect("wrapper");
    assert!(wrapper.memory_effects_complete, "{wrapper:#?}");
    assert!(wrapper.complete, "{wrapper:#?}");
    assert_eq!(wrapper.memory_effects.len(), 1);
}

#[test]
fn known_callee_writes_are_instantiated_in_the_concrete_caller() {
    let flows = analyze(concat!(
        "struct S { int x; }; ",
        "void touch(struct S *p) { p->x = 1; } ",
        "int caller(void) { struct S s; s.x = 0; touch(&s); return s.x; }",
    ))
    .into_parts()
    .0;
    let caller = flows.iter().find(|flow| flow.name == "caller").unwrap();
    assert!(caller.memory_complete, "{:#?}", caller.semantic_issues);
    assert!(caller
        .memory_definitions
        .iter()
        .all(|definition| definition.kind != MemoryDefinitionKind::CallClobber));
    let call_effect = caller
        .memory_definitions
        .iter()
        .position(|definition| definition.kind == MemoryDefinitionKind::CallEffect)
        .expect("instantiated call write");
    assert_eq!(
        caller
            .memory_region_name(caller.memory_definitions[call_effect].region)
            .as_deref(),
        Some("s.x")
    );
    assert!(caller
        .memory_edges
        .iter()
        .any(|edge| edge.definition == call_effect as u32));
}

#[test]
fn known_read_only_and_pure_callees_do_not_clobber_caller_storage() {
    let read_only = analyze(concat!(
        "struct S { int x; }; ",
        "int inspect(struct S *p) { return p->x; } ",
        "int caller(void) { struct S s; s.x = 1; inspect(&s); return s.x; }",
    ))
    .into_parts()
    .0;
    let caller = read_only.iter().find(|flow| flow.name == "caller").unwrap();
    assert!(caller.memory_complete, "{:#?}", caller.semantic_issues);
    assert!(caller.memory_definitions.iter().all(|definition| {
        !matches!(
            definition.kind,
            MemoryDefinitionKind::CallClobber | MemoryDefinitionKind::CallEffect
        )
    }));
    assert!(caller
        .memory_uses
        .iter()
        .any(|use_| { caller.calls.iter().any(|call| use_.span == call.span) }));

    let pure = analyze(concat!(
        "struct S { int x; }; ",
        "void ignore(struct S *p) { (void)p; } ",
        "int caller(void) { struct S s; s.x = 1; ignore(&s); return s.x; }",
    ))
    .into_parts()
    .0;
    let caller = pure.iter().find(|flow| flow.name == "caller").unwrap();
    assert!(caller.memory_complete, "{:#?}", caller.semantic_issues);
    assert!(caller.memory_definitions.iter().all(|definition| {
        !matches!(
            definition.kind,
            MemoryDefinitionKind::CallClobber | MemoryDefinitionKind::CallEffect
        )
    }));
}

#[test]
fn instantiated_formal_effects_participate_in_parameter_alias_overlap() {
    let flows = analyze(concat!(
        "struct S { int x; }; ",
        "void touch(struct S *p) { p->x = 1; } ",
        "int wrapper(struct S *p, struct S *q) { touch(p); return q->x; }",
    ))
    .into_parts()
    .0;
    let wrapper = flows.iter().find(|flow| flow.name == "wrapper").unwrap();
    let call_effect = wrapper
        .memory_definitions
        .iter()
        .position(|definition| definition.kind == MemoryDefinitionKind::CallEffect)
        .expect("instantiated formal write");
    assert!(wrapper.memory_edges.iter().any(|edge| {
        edge.definition == call_effect as u32
            && edge.overlap == Some(MemoryOverlapKind::ParameterAlias)
    }));
}

#[test]
fn transitive_known_writes_are_instantiated_in_concrete_callers() {
    let flows = analyze(concat!(
        "struct S { int x; }; ",
        "void leaf(struct S *p) { p->x = 1; } ",
        "void wrapper(struct S *p) { leaf(p); } ",
        "int caller(void) { struct S s; wrapper(&s); return s.x; }",
    ))
    .into_parts()
    .0;
    let caller = flows.iter().find(|flow| flow.name == "caller").unwrap();
    assert!(caller.memory_complete, "{:#?}", caller.semantic_issues);
    assert!(caller
        .memory_definitions
        .iter()
        .any(|definition| definition.kind == MemoryDefinitionKind::CallEffect));
    assert!(caller
        .memory_definitions
        .iter()
        .all(|definition| definition.kind != MemoryDefinitionKind::CallClobber));
}

#[test]
fn ambiguous_callee_names_retain_the_conservative_call_clobber() {
    let flows = analyze(concat!(
        "struct S { int x; }; ",
        "void touch(struct S *p) { p->x = 1; } ",
        "void touch(struct S *p) { (void)p; } ",
        "int caller(void) { struct S s; touch(&s); return s.x; }",
    ))
    .into_parts()
    .0;
    let caller = flows.iter().find(|flow| flow.name == "caller").unwrap();
    assert!(!caller.memory_complete);
    assert!(caller
        .memory_definitions
        .iter()
        .any(|definition| definition.kind == MemoryDefinitionKind::CallClobber));
    assert!(caller
        .memory_definitions
        .iter()
        .all(|definition| definition.kind != MemoryDefinitionKind::CallEffect));
}

#[test]
fn projected_accesses_share_structural_memory_region_identity() {
    let source = concat!(
        "struct S{int x;int a[4];};",
        "int f(int i,int v){struct S s;struct S *p=&s;",
        "s.x=v;p->x=v;s.a[i]=v;return s.x+s.a[0];}"
    );
    let flow = one(source);

    let field_regions = flow
        .memory_regions
        .iter()
        .filter(|region| {
            matches!(
                &region.kind,
                MemoryRegionKind::Field { member, .. } if member == "x"
            )
        })
        .map(|region| region.id)
        .collect::<Vec<_>>();
    assert_eq!(field_regions.len(), 1, "{:#?}", flow.memory_regions);
    let field_accesses = flow
        .memory_accesses
        .iter()
        .filter(|access| access.region == field_regions[0])
        .collect::<Vec<_>>();
    assert_eq!(field_accesses.len(), 3, "{:#?}", flow.memory_accesses);
    assert!(field_accesses.iter().any(|access| {
        access.kind == MemoryAccessKind::Write
            && access.precision == MemoryAccessPrecision::MayAlias
    }));
    assert!(field_accesses.iter().any(|access| {
        access.kind == MemoryAccessKind::Read && access.precision == MemoryAccessPrecision::Exact
    }));

    let element_region = flow
        .memory_regions
        .iter()
        .find(|region| matches!(region.kind, MemoryRegionKind::Elements { .. }))
        .expect("array accesses must share an element-summary region");
    let element_accesses = flow
        .memory_accesses
        .iter()
        .filter(|access| access.region == element_region.id)
        .collect::<Vec<_>>();
    assert_eq!(element_accesses.len(), 2, "{:#?}", flow.memory_accesses);
    assert!(element_accesses
        .iter()
        .all(|access| access.precision == MemoryAccessPrecision::MayAlias));
    assert!(flow.memory_overlaps.iter().any(|overlap| {
        overlap.kind == MemoryOverlapKind::Containment && overlap.right == element_region.id
    }));

    let field_use = flow
        .memory_uses
        .iter()
        .position(|use_| use_.region == field_regions[0])
        .expect("final field read");
    assert_eq!(
        flow.memory_edges
            .iter()
            .filter(|edge| edge.use_ == field_use as u32)
            .count(),
        2,
        "{:#?}",
        flow.memory_edges
    );
    let element_use = flow
        .memory_uses
        .iter()
        .position(|use_| use_.region == element_region.id)
        .expect("final element read");
    assert_eq!(
        flow.memory_edges
            .iter()
            .filter(|edge| edge.use_ == element_use as u32)
            .count(),
        1
    );

    assert!(flow.memory_complete, "{flow:#?}");
}

#[test]
fn exact_region_writes_kill_while_branch_writes_join() {
    let linear = one("struct S{int x;};int f(int a,int b){struct S s;s.x=a;s.x=b;return s.x;}");
    assert_eq!(linear.memory_definitions.len(), 2);
    assert_eq!(linear.memory_uses.len(), 1);
    assert_eq!(linear.memory_edges.len(), 1, "{:#?}", linear.memory_edges);
    assert_eq!(linear.memory_edges[0].definition, 1);

    let branch = one(concat!(
        "struct S{int x;};",
        "int f(int c,int a,int b){struct S s;",
        "if(c)s.x=a;else s.x=b;return s.x;}"
    ));
    assert_eq!(branch.memory_definitions.len(), 2);
    assert_eq!(branch.memory_uses.len(), 1);
    assert_eq!(branch.memory_edges.len(), 2, "{:#?}", branch.memory_edges);
    let mut definitions = branch
        .memory_edges
        .iter()
        .map(|edge| edge.definition)
        .collect::<Vec<_>>();
    definitions.sort_unstable();
    assert_eq!(definitions, vec![0, 1]);

    let looped = one(concat!(
        "struct S{int x;};",
        "int f(int c,int a,int b){struct S s;s.x=a;",
        "while(c){b+=s.x;s.x=b;}return s.x;}"
    ));
    assert_eq!(looped.memory_definitions.len(), 2);
    assert_eq!(looped.memory_uses.len(), 2);
    for use_index in 0..2 {
        let mut reaching = looped
            .memory_edges
            .iter()
            .filter(|edge| edge.use_ == use_index)
            .map(|edge| edge.definition)
            .collect::<Vec<_>>();
        reaching.sort_unstable();
        assert_eq!(reaching, vec![0, 1], "{:#?}", looped.memory_edges);
    }
}

#[test]
fn union_member_regions_overlap_but_struct_siblings_do_not() {
    let union = one(concat!(
        "union U{int a;int b;};",
        "int f(int x,int y){union U u;u.a=x;u.b=y;return u.a;}"
    ));
    assert!(union.memory_overlaps.iter().any(|overlap| {
        overlap.kind == MemoryOverlapKind::UnionMembers
            && union.memory_region_name(overlap.left).as_deref() == Some("u.a")
            && union.memory_region_name(overlap.right).as_deref() == Some("u.b")
    }));
    assert_eq!(union.memory_edges.len(), 1, "{:#?}", union.memory_edges);
    let edge = &union.memory_edges[0];
    assert_eq!(edge.definition, 1);
    assert_eq!(edge.overlap, Some(MemoryOverlapKind::UnionMembers));
    assert_eq!(
        union.memory_region_name(edge.definition_region).as_deref(),
        Some("u.b")
    );
    assert_eq!(
        union.memory_region_name(edge.use_region).as_deref(),
        Some("u.a")
    );

    let structure = one(concat!(
        "struct S{int a;int b;};",
        "int f(int x){struct S s;s.a=x;return s.b;}"
    ));
    assert!(!structure
        .memory_overlaps
        .iter()
        .any(|overlap| overlap.kind == MemoryOverlapKind::UnionMembers));
    assert!(
        structure.memory_edges.is_empty(),
        "{:#?}",
        structure.memory_edges
    );

    let nested = one(concat!(
        "struct A{int x;};struct B{int y;};",
        "union U{struct A a;struct B b;};",
        "int f(int x){union U u;u.a.x=x;return u.b.y;}"
    ));
    assert_eq!(nested.memory_edges.len(), 1, "{:#?}", nested.memory_edges);
    assert_eq!(
        nested.memory_edges[0].overlap,
        Some(MemoryOverlapKind::UnionMembers)
    );
    assert_eq!(
        nested
            .memory_region_name(nested.memory_edges[0].definition_region)
            .as_deref(),
        Some("u.a.x")
    );
    assert_eq!(
        nested
            .memory_region_name(nested.memory_edges[0].use_region)
            .as_deref(),
        Some("u.b.y")
    );
}

#[test]
fn incoming_aggregate_parameter_regions_reach_returns_until_overwritten() {
    let direct = one("struct S{int x;};int f(struct S s){return s.x;}");
    assert_eq!(direct.memory_uses.len(), 1);
    assert_eq!(direct.memory_edges.len(), 1);
    assert!(direct.uses.iter().all(|use_| use_.name != "s"));
    let incoming = &direct.memory_definitions[direct.memory_edges[0].definition as usize];
    assert_eq!(incoming.kind, MemoryDefinitionKind::IncomingParameter);
    assert_eq!(
        direct.memory_region_name(incoming.region).as_deref(),
        Some("s.x")
    );
    let summaries = summarize(&[direct]);
    let summary = summaries.get("f").expect("aggregate summary");
    assert!(summary.flows_to(0, Sink::Return), "{summary:#?}");
    assert!(summary.complete, "{summary:#?}");

    let overwritten = one("struct S{int x;};int f(struct S s){s.x=0;return s.x;}");
    assert_eq!(overwritten.memory_edges.len(), 1);
    assert!(overwritten.uses.iter().all(|use_| use_.name != "s"));
    assert_eq!(
        overwritten.memory_definitions[overwritten.memory_edges[0].definition as usize].kind,
        MemoryDefinitionKind::Store
    );
    assert!(!summarize(&[overwritten])
        .get("f")
        .expect("overwritten summary")
        .flows_to(0, Sink::Return));

    let conditional = one("struct S{int x;};int f(struct S s,int c){if(c)s.x=0;return s.x;}");
    let summary = summarize(&[conditional]);
    assert!(summary
        .get("f")
        .expect("conditional summary")
        .flows_to(0, Sink::Return));
}

#[test]
fn pointer_arguments_create_weak_call_clobbers_until_effects_are_known() {
    let projected = one(concat!(
        "struct S{int x;};void touch(struct S *);",
        "int f(int v){struct S s;s.x=v;touch(&s);return s.x;}"
    ));
    assert_eq!(
        projected.memory_edges.len(),
        2,
        "{:#?}",
        projected.memory_edges
    );
    assert!(projected.memory_definitions.iter().any(|definition| {
        definition.kind == MemoryDefinitionKind::CallClobber
            && definition.precision == MemoryAccessPrecision::MayAlias
    }));
    assert!(!projected.memory_complete);

    let overwritten_after_call = one(concat!(
        "struct S{int x;};void touch(struct S *);",
        "int f(int v){struct S s;touch(&s);s.x=v;return s.x;}"
    ));
    assert_eq!(overwritten_after_call.memory_edges.len(), 1);
    assert_eq!(
        overwritten_after_call.memory_definitions
            [overwritten_after_call.memory_edges[0].definition as usize]
            .kind,
        MemoryDefinitionKind::Store
    );

    let scalar = one("void touch(int *);int f(int x){touch(&x);return x;}");
    let return_use = scalar
        .uses
        .iter()
        .position(|use_| use_.span.lo > scalar.calls[0].span.hi)
        .expect("return use");
    let reaching = scalar
        .definitions_reaching(return_use as u32)
        .map(|definition| definition.kind)
        .collect::<Vec<_>>();
    assert!(reaching.contains(&DefKind::Parameter));
    assert!(reaching.contains(&DefKind::MemoryWrite));
    assert!(!scalar.memory_complete);

    let value_only = one("int consume(int);int f(int x){return consume(x);}");
    assert!(value_only.memory_complete, "{value_only:#?}");
    assert!(value_only
        .definitions
        .iter()
        .all(|definition| definition.kind != DefKind::MemoryWrite));

    let array = one(concat!(
        "void touch(int *);",
        "int f(void){int a[4];a[0]=1;touch(a);return a[0];}"
    ));
    assert_eq!(array.memory_edges.len(), 2, "{:#?}", array.memory_edges);
    assert!(array
        .memory_definitions
        .iter()
        .any(|definition| definition.kind == MemoryDefinitionKind::CallClobber));
    assert!(array.uses.iter().all(|use_| use_.name != "a"));
}

#[test]
fn every_eight_write_pointer_and_direct_order_retains_the_concrete_last_write() {
    for mask in 0..(1u32 << 8) {
        let mut code = String::from("int f(void){int x=0;int *p=&x;");
        let mut expected_start = 0;
        let mut expected_kind = DefKind::Assignment;
        for value in 1..=8 {
            expected_start = code.len() as u32;
            if mask & (1 << (value - 1)) == 0 {
                code.push_str(&format!("x={value};"));
                expected_kind = DefKind::Assignment;
            } else {
                code.push_str(&format!("*p={value};"));
                expected_kind = DefKind::MemoryWrite;
            }
        }
        code.push_str("return x;}");
        let flow = one(&code);
        assert!(flow.memory_complete, "{code}");
        let return_offset = code.rfind('x').unwrap() as u32;
        let return_use = flow
            .uses
            .iter()
            .position(|use_| use_.span.lo == return_offset)
            .expect("return use") as u32;
        assert!(
            flow.definitions_reaching(return_use).any(|definition| {
                definition.kind == expected_kind && definition.span.lo == expected_start
            }),
            "mask={mask:#010b} {code}"
        );
    }
}

#[test]
fn analysis_is_total_on_input_that_is_not_c() {
    for junk in ["", "\u{0}\u{1}", "int f(", "}}}", "\u{4e2d}\u{6587}"] {
        let flows = analyze(junk).into_parts().0;
        for flow in &flows {
            assert!(flow.edges.len() <= flow.definitions.len() * flow.uses.len() + 1);
        }
    }
}

#[test]
fn output_is_deterministic() {
    let text = "int f(int n) { int s = 0; for (int i = 0; i < n; i++) { s += i; } return s; }";
    assert_eq!(one(text), one(text));
}

#[test]
fn a_store_nothing_reads_is_dead() {
    let flow = one("int f(void) { int x = 1; x = 2; return 0; }");
    // Both writes of `x` are dead: nothing reads it.
    assert_eq!(flow.dead_stores.len(), 2, "{:?}", flow.definitions);
}

#[test]
fn a_store_something_reads_is_not_dead() {
    let flow = one("int f(void) { int x = 1; return x; }");
    assert!(flow.dead_stores.is_empty(), "{:?}", flow.dead_stores);
}

#[test]
fn an_overwritten_store_is_dead_and_the_survivor_is_not() {
    let flow = one("int f(void) { int x = 1; x = 2; return x; }");
    assert_eq!(flow.dead_stores.len(), 1);
    let dead = &flow.definitions[flow.dead_stores[0] as usize];
    assert_eq!(
        dead.kind,
        DefKind::Declaration,
        "the first write is the dead one"
    );
}

#[test]
fn an_unread_parameter_is_not_a_dead_store() {
    let flow = one("int f(int unused) { return 0; }");
    assert!(flow.dead_stores.is_empty(), "{:?}", flow.dead_stores);
}

#[test]
fn a_decompiler_temporary_that_is_never_read_is_reported() {
    // The shape a decompiler actually emits: a named slot assigned from a
    // call and then ignored.
    let flow = one("int f(int a) { int v1; v1 = g(a); return a; }");
    let dead: Vec<&str> = flow
        .dead_stores
        .iter()
        .map(|i| flow.definitions[*i as usize].name.as_str())
        .collect();
    assert!(dead.contains(&"v1"), "{dead:?}");
}

#[test]
fn the_fixture_corpus_analyzes_without_a_contradiction() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/decompiler_fixtures/src");
    let mut files = 0usize;
    let mut functions = 0usize;
    let mut edges = 0usize;
    let mut dead = 0usize;
    let mut unresolved = 0usize;

    for (_path, text) in crate::test_corpus::sources(&root) {
        files += 1;
        for flow in analyze(&text).into_parts().0 {
            functions += 1;
            edges += flow.edges.len();
            dead += flow.dead_stores.len();
            unresolved += flow.unresolved_uses.len();

            for edge in &flow.edges {
                let definition = flow
                    .definitions
                    .get(edge.def as usize)
                    .expect("edge names a real definition");
                let use_ = flow
                    .uses
                    .get(edge.use_ as usize)
                    .expect("edge names a real use");
                // An edge is about one variable, and both ends agree.
                assert_eq!(definition.binding, use_.binding, "{}", flow.name);
                assert_eq!(edge.name, use_.name, "{}", flow.name);
            }
            // The two defect sets are disjoint from what they describe: a
            // use is unresolved exactly when no edge enters it.
            for index in &flow.unresolved_uses {
                assert!(
                    !flow.edges.iter().any(|e| e.use_ == *index),
                    "{}: use {} is both reached and unresolved",
                    flow.name,
                    index
                );
            }
            for index in &flow.dead_stores {
                assert!(
                    !flow.edges.iter().any(|e| e.def == *index),
                    "{}: definition {} is both read and dead",
                    flow.name,
                    index
                );
            }
        }
    }

    assert!(files > 100, "corpus not found: {files} files");
    assert!(functions > 500, "only {functions} functions");
    assert!(
        edges > 1000,
        "only {edges} edges over {functions} functions"
    );

    // Hand-written C should barely have any dead stores. This bound is
    // deliberately loose --- the corpus grows --- but it is tight enough
    // to fail if a change starts calling ordinary code dead. Each round of
    // over-reporting this analysis went through tripped exactly here:
    // 897 when a bare `int x;` counted as a store, 460 when a write to a
    // global counted, 27 when `++a[i]` counted as a write to `a`.
    assert!(
        dead * 20 < functions,
        "{dead} dead stores over {functions} hand-written functions is too many \
     to be real; something is counting a non-store as a store"
    );
    assert!(unresolved < functions * 3, "{unresolved} unresolved uses");
    eprintln!(
        "corpus: {files} files, {functions} functions, {edges} edges, \
     {dead} dead stores, {unresolved} unresolved uses"
    );
}

// --- declared types ---------------------------------------------------------

/// The declared type of the binding named `name`.
fn type_of(flow: &DataFlow, name: &str) -> Option<CType> {
    flow.type_of(flow.binding_named(name)?).cloned()
}

#[test]
fn a_local_carries_the_type_it_was_declared_with() {
    let flow = one("int f(void) { unsigned long total = 0; return (int)total; }");
    let ty = type_of(&flow, "total").expect("a type for total");
    assert_eq!(ty.specifiers, "unsigned long");
    assert_eq!(ty.pointer_depth, 0);
    assert_eq!(ty.render(), "unsigned long");
}

#[test]
fn a_parameter_carries_its_type_including_pointer_depth() {
    let flow = one("int f(const char *name, int n) { return n; }");
    let name = type_of(&flow, "name").expect("a type for name");
    assert_eq!(name.specifiers, "const char");
    assert_eq!(name.pointer_depth, 1);
    assert!(name.is_const);
    let n = type_of(&flow, "n").expect("a type for n");
    assert_eq!(n.specifiers, "int");
    assert_eq!(n.pointer_depth, 0);
}

#[test]
fn one_declaration_of_several_names_gives_each_its_own_shape() {
    // The specifiers apply to all three; the stars and brackets do not.
    let flow = one("int f(void) { int a = 1, *b, c[4]; return a; }");
    let a = type_of(&flow, "a").expect("a");
    let b = type_of(&flow, "b").expect("b");
    let c = type_of(&flow, "c").expect("c");
    assert_eq!(
        (a.specifiers.as_str(), a.pointer_depth, a.array_rank),
        ("int", 0, 0)
    );
    assert_eq!(
        (b.specifiers.as_str(), b.pointer_depth, b.array_rank),
        ("int", 1, 0)
    );
    assert_eq!(
        (c.specifiers.as_str(), c.pointer_depth, c.array_rank),
        ("int", 0, 1)
    );
}

#[test]
fn initializer_operators_do_not_change_declarator_types() {
    let flow = one("int f(int x, int *p) { int a = x * 2, b = p[0], *c = p; return a + b + *c; }");
    let a = type_of(&flow, "a").expect("a");
    let b = type_of(&flow, "b").expect("b");
    let c = type_of(&flow, "c").expect("c");
    assert_eq!((a.pointer_depth, a.array_rank), (0, 0));
    assert_eq!((b.pointer_depth, b.array_rank), (0, 0));
    assert_eq!((c.pointer_depth, c.array_rank), (1, 0));
}

#[test]
fn a_nested_initializer_declaration_keeps_its_own_type() {
    let flow =
        one("int f(int x, int *p) { int a = ({ int *inner = p; *inner; }), b = x; return a + b; }");
    let a = type_of(&flow, "a").expect("a");
    let b = type_of(&flow, "b").expect("b");
    let inner = type_of(&flow, "inner").expect("inner");
    assert_eq!((a.pointer_depth, a.array_rank), (0, 0));
    assert_eq!((b.pointer_depth, b.array_rank), (0, 0));
    assert_eq!((inner.pointer_depth, inner.array_rank), (1, 0));
}

#[test]
fn brackets_inside_a_function_pointer_parameter_are_not_array_suffixes() {
    let flow = one("int f(void) { int (*callback)(int values[4]); return callback(0); }");
    let callback = type_of(&flow, "callback").expect("callback");
    assert_eq!((callback.pointer_depth, callback.array_rank), (1, 0));
}

#[test]
fn nested_function_pointer_parameters_are_not_outer_bindings() {
    let flow =
        one("int f(void (*callback)(int values[4], size_t), int x) { callback(0, 0); return x; }");
    assert!(flow.binding_named("callback").is_some());
    assert!(flow.binding_named("x").is_some());
    assert!(flow.binding_named("values").is_none());
    assert!(flow.binding_named("size_t").is_none());
    let callback = type_of(&flow, "callback").expect("callback");
    assert_eq!((callback.pointer_depth, callback.array_rank), (1, 0));
}

#[test]
fn a_multidimensional_parameter_records_each_array_suffix() {
    let flow = one("int f(int matrix[4][8], int x) { return matrix[x][0]; }");
    let matrix = type_of(&flow, "matrix").expect("matrix");
    assert_eq!((matrix.pointer_depth, matrix.array_rank), (0, 2));
}

#[test]
fn sizeof_an_array_parameter_uses_its_adjusted_pointer_type() {
    let flow = one("int f(int matrix[4][8]) { return sizeof(matrix); }");
    let matrix = flow.binding_named("matrix").expect("matrix");
    assert!(
        flow.uses.iter().all(|usage| usage.binding != matrix),
        "sizeof of the adjusted parameter type does not read the parameter value"
    );
}

#[test]
fn sizeof_a_local_pointer_typedef_follows_the_alias_without_reading_storage() {
    let flow = one("int f(void) { typedef int *P; P pointer; return sizeof(pointer); }");
    let pointer = flow.binding_named("pointer").expect("pointer");
    assert!(flow.uses.iter().all(|usage| usage.binding != pointer));
    let written = flow.type_of(pointer).expect("written compatibility type");
    assert_eq!(
        (written.specifiers.as_str(), written.pointer_depth),
        ("P", 0)
    );
}

#[test]
fn sizeof_a_file_pointer_typedef_follows_the_visible_alias_chain() {
    let flow = one("typedef int *P; typedef P Q; int f(Q pointer) { return sizeof(pointer); }");
    let pointer = flow.binding_named("pointer").expect("pointer");
    assert!(flow.uses.iter().all(|usage| usage.binding != pointer));
    assert_eq!(
        flow.type_of(pointer).map(|ty| ty.specifiers.as_str()),
        Some("Q")
    );
}

#[test]
fn a_direct_function_parameter_keeps_its_outer_name() {
    let flow = one("int f(int callback(int), int x) { return callback(x); }");
    assert!(flow.binding_named("callback").is_some());
    assert!(flow.binding_named("x").is_some());
}

#[test]
fn an_unnamed_function_pointer_does_not_bind_its_nested_typedef() {
    let flow = one("int f(int (*)(size_t), int x) { return x; }");
    assert!(flow.binding_named("size_t").is_none());
    assert!(flow.binding_named("x").is_some());
}

#[test]
fn unnamed_aggregate_parameter_tags_are_not_value_bindings() {
    for declaration in ["struct point", "union value", "enum mode"] {
        let flow = one(&format!("int f({declaration}, int x) {{ return x; }}"));
        assert_eq!(
            flow.definitions
                .iter()
                .filter(|definition| definition.kind == DefKind::Parameter)
                .map(|definition| definition.name.as_str())
                .collect::<Vec<_>>(),
            vec!["x"],
            "{declaration}"
        );
    }
}

#[test]
fn a_visible_file_typedef_disambiguates_an_unnamed_parameter() {
    let flow = one("typedef unsigned long size_t; int f(const size_t, int x) { return x; }");
    assert!(flow.binding_named("size_t").is_none());
    assert!(flow.binding_named("x").is_some());
}

#[test]
fn an_explicit_parameter_name_may_shadow_a_visible_typedef() {
    for declaration in ["int size_t", "size_t size_t"] {
        let flow = one(&format!(
            "typedef unsigned long size_t; int f({declaration}) {{ return size_t; }}"
        ));
        assert!(flow.binding_named("size_t").is_some(), "{declaration}");
        assert!(flow.unresolved_uses.is_empty(), "{declaration}");
    }
}

#[test]
fn a_later_typedef_does_not_retroactively_change_a_parameter() {
    let flow = one("int f(size_t, int x) { return x; } typedef unsigned long size_t;");
    assert!(flow.binding_named("size_t").is_some());
    assert!(flow.binding_named("x").is_some());
}

#[test]
fn a_typedef_only_affects_later_functions() {
    let flows = analyze(
        "int before(size_t, int x) { return x; }\
         typedef unsigned long size_t;\
         int after(size_t, int x) { return x; }",
    )
    .into_parts()
    .0;
    assert!(flows[0].binding_named("size_t").is_some());
    assert!(flows[1].binding_named("size_t").is_none());
    assert!(flows[1].binding_named("x").is_some());
}

#[test]
fn every_name_in_a_multi_declarator_typedef_is_visible() {
    let flow = one("typedef unsigned long size_t, count_t;\
         int f(size_t, count_t, int x) { return x; }");
    assert!(flow.binding_named("size_t").is_none());
    assert!(flow.binding_named("count_t").is_none());
    assert!(flow.binding_named("x").is_some());
}

#[test]
fn a_named_aggregate_parameter_binds_the_declarator_not_the_tag() {
    for declaration in ["struct point value", "union cell value", "enum mode value"] {
        let flow = one(&format!("int f({declaration}) {{ return value != 0; }}"));
        assert!(flow.binding_named("value").is_some(), "{declaration}");
        assert!(flow
            .binding_named(declaration.split_whitespace().nth(1).unwrap())
            .is_none());
        assert!(flow.unresolved_uses.is_empty(), "{declaration}");
    }
}

#[test]
fn a_struct_tag_is_recorded_as_written() {
    let flow = one("int f(void) { struct point *p; return (int)(long)p; }");
    let ty = type_of(&flow, "p").expect("a type for p");
    assert_eq!(ty.specifiers, "struct point");
    assert_eq!(ty.pointer_depth, 1);
    assert_eq!(ty.render(), "struct point *");
}

#[test]
fn a_typedef_from_a_header_stays_an_opaque_name() {
    // No `#include` resolution, so `uint32_t` is a name and nothing claims to
    // know its width. Recording the spelling is the honest answer.
    let flow = one("int f(void) { uint32_t n = 0; return (int)n; }");
    let ty = type_of(&flow, "n").expect("a type for n");
    assert_eq!(ty.specifiers, "uint32_t");
}

#[test]
fn a_block_scope_typedef_is_not_a_value_binding_or_use() {
    let flow = one(
        "int f(void) { typedef unsigned long word_t; word_t x = 1; return sizeof(word_t) + x; }",
    );
    assert!(flow.binding_named("word_t").is_none());
    assert!(flow.binding_named("x").is_some());
    assert!(flow.unresolved_uses.is_empty());
    assert_eq!(flow.uses.iter().filter(|use_| use_.name == "x").count(), 1);
}

#[test]
fn a_value_declaration_shadows_a_visible_typedef_in_sizeof() {
    let flow =
        one("typedef unsigned long word_t; int f(void) { int word_t = 1; return sizeof(word_t); }");
    assert!(flow.binding_named("word_t").is_some());
    assert!(flow.unresolved_uses.is_empty());
    assert!(
        flow.uses.is_empty(),
        "fixed-size sizeof does not read the value"
    );
}

#[test]
fn a_visible_file_typedef_is_a_type_in_sizeof() {
    let flow = one("typedef unsigned long word_t; int f(void) { return sizeof(word_t); }");
    assert!(flow.binding_named("word_t").is_none());
    assert!(flow.uses.is_empty());
    assert!(flow.unresolved_uses.is_empty());
}

#[test]
fn a_variable_length_array_extent_reads_its_parameter() {
    for declaration in ["int values[n]", "typedef int values_t[n]"] {
        let code = if declaration.starts_with("typedef") {
            format!("int f(int n) {{ {declaration}; return sizeof(values_t); }}")
        } else {
            format!("int f(int n) {{ {declaration}; return sizeof(values); }}")
        };
        let flow = one(&code);
        assert!(flow.uses.iter().any(|use_| use_.name == "n"), "{code}");
        assert_eq!(
            flow.definitions_reaching(
                flow.uses.iter().position(|use_| use_.name == "n").unwrap() as u32
            )
            .map(|definition| definition.name.as_str())
            .collect::<Vec<_>>(),
            vec!["n"],
            "{code}"
        );
        assert!(flow.vla_complete, "{code}: {flow:#?}");
        let summary = summarize(&[flow]);
        let f = summary.get("f").expect("f");
        assert!(f.complete, "{code}: {f:#?}");
        assert!(f.flows_to(0, Sink::Return), "{code}: {f:#?}");
    }

    let aliased_object = one(concat!(
        "int f(int n) { typedef int values_t[n]; values_t values; ",
        "return sizeof(values); }",
    ));
    let summary = summarize(&[aliased_object]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(f.flows_to(0, Sink::Return), "{f:#?}");
}

#[test]
fn an_unresolved_array_extent_prevents_a_complete_negative() {
    let flow = one("int f(void) { int values[EXTERNAL_BOUND]; return sizeof(values); }");
    assert!(!flow.vla_complete);
    let summaries = summarize(&[flow]);
    assert!(!summaries.get("f").unwrap().complete);
}

#[test]
fn unevaluated_extent_operands_do_not_read_values_but_vla_types_do() {
    for bound in ["sizeof(n)", "sizeof(n + 1)", "_Alignof(n)", "sizeof n"] {
        let flow = one(&format!(
            "int f(int n) {{ int values[{bound}]; return sizeof(values); }}"
        ));
        assert!(!flow.uses.iter().any(|use_| use_.name == "n"), "{bound}");
        assert!(flow.vla_complete, "{bound}");
        assert!(summarize(&[flow]).get("f").unwrap().flows.is_empty());
    }

    let vla_type = one("int f(int n) { int values[sizeof(int[n])]; return sizeof(values); }");
    assert_eq!(
        vla_type.uses.iter().filter(|use_| use_.name == "n").count(),
        2,
        "one use forms the bound and one consumes its captured value"
    );
    assert!(vla_type.vla_complete);

    for source in [
        "int f(int n) { typedef int T; int values[sizeof(T[n])]; return sizeof(values); }",
        "int f(int n) { int values[sizeof(const int[n])]; return sizeof(values); }",
        "int f(int n) { int values[sizeof(n) + n]; return sizeof(values); }",
        "int f(int n) { int values[sizeof n + n]; return sizeof(values); }",
    ] {
        let flow = one(source);
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "n").count(),
            2,
            "{source}"
        );
    }

    let alignof_vla = one("int f(int n) { int values[_Alignof(int[n])]; return sizeof(values); }");
    assert!(!alignof_vla.uses.iter().any(|use_| use_.name == "n"));
}

#[test]
fn generic_selection_in_an_extent_fails_closed_without_definite_reads() {
    for bound in [
        "_Generic((n), int: 4, default: 8)",
        "_Generic((n), int: n, default: 8)",
        "_Generic((n), int: 4, default: n)",
        "_Generic((n), int: n, default: n + 1)",
    ] {
        let flow = one(&format!(
            "int f(int n) {{ int values[{bound}]; return sizeof(values); }}"
        ));
        assert!(!flow.uses.iter().any(|use_| use_.name == "n"), "{bound}");
        assert!(!flow.vla_complete, "{bound}");
        let summary = summarize(&[flow]);
        assert!(!summary.get("f").unwrap().complete, "{bound}");
        assert!(summary.get("f").unwrap().flows.is_empty(), "{bound}");
    }
}

#[test]
fn direct_writes_in_array_bounds_kill_the_prior_value() {
    let assigned = one("int f(int n) { int values[n = 4]; return n + sizeof(values) * 0; }");
    assert!(assigned.vla_complete);
    assert_eq!(
        assigned
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        vec![DefKind::Parameter, DefKind::Assignment]
    );
    let return_use = assigned
        .uses
        .iter()
        .position(|use_| use_.name == "n")
        .unwrap() as u32;
    assert_eq!(
        assigned
            .definitions_reaching(return_use)
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        vec![DefKind::Assignment]
    );
    let assigned_summary = summarize(&[assigned]);
    assert!(assigned_summary.get("f").unwrap().complete);
    assert!(assigned_summary.get("f").unwrap().flows.is_empty());

    for bound in ["n++", "++n"] {
        let flow = one(&format!(
            "int f(int n) {{ int values[{bound}]; return n + sizeof(values) * 0; }}"
        ));
        assert!(flow.vla_complete, "{bound}");
        assert_eq!(
            flow.definitions
                .iter()
                .filter(|definition| definition.name == "n")
                .count(),
            2,
            "{bound}"
        );
        assert!(summarize(&[flow])
            .get("f")
            .unwrap()
            .flows_to(0, Sink::Return));
    }

    let compound = one("int f(int n) { int values[n += 1]; return n + sizeof(values) * 0; }");
    assert!(compound.vla_complete);
    let summary = summarize(&[compound]);
    let f = summary.get("f").unwrap();
    assert!(f.complete);
    assert!(f.flows_to(0, Sink::Return));

    let copied = one("int f(int n, int m) { int values[(n = m)]; return n + sizeof(values) * 0; }");
    let copied_summary = summarize(&[copied]);
    assert!(!copied_summary.get("f").unwrap().flows_to(0, Sink::Return));
    assert!(copied_summary.get("f").unwrap().flows_to(1, Sink::Return));
}

#[test]
fn guarded_bound_writes_preserve_the_bypass_definition() {
    for bound in ["c ? n++ : m", "c && n++"] {
        let source = format!(
            "int f(int c, int n, int m) {{ int values[{bound}]; return n + sizeof(values) * 0; }}"
        );
        let flow = one(&source);
        assert!(flow.vla_complete, "{bound}: {:#?}", flow.semantic_issues);
        let return_offset = source.rfind("n + sizeof").expect("return use") as u32;
        let return_use = flow
            .uses
            .iter()
            .position(|use_| use_.name == "n" && use_.span.lo == return_offset)
            .expect("return reads n") as u32;
        let reaching = flow
            .definitions_reaching(return_use)
            .map(|definition| definition.kind)
            .collect::<Vec<_>>();
        assert_eq!(
            reaching,
            [DefKind::Parameter, DefKind::IncDec],
            "the untaken arm retains the incoming value for {bound}"
        );
    }
}

#[test]
fn assignment_bound_value_depends_on_the_rhs_not_the_replaced_target() {
    let flow = one(concat!(
        "int f(int n, int m) { int values[(n = m + 1)]; ",
        "return sizeof(values); }",
    ));
    assert!(flow.vla_complete, "{:#?}", flow.semantic_issues);
    let summaries = summarize(&[flow]);
    let f = summaries.get("f").expect("f");
    assert!(!f.flows_to(0, Sink::Return));
    assert!(f.flows_to(1, Sink::Return));
}

#[test]
fn a_call_on_a_literal_short_circuit_bypass_adds_no_effect_uncertainty() {
    let flow = one(concat!(
        "int opaque(int); int f(int n) { int values[1 || opaque(n)]; ",
        "return sizeof(values); }",
    ));
    assert!(flow.calls.is_empty());
    assert!(flow.effects_complete, "{:#?}", flow.semantic_issues);
    assert!(flow.vla_complete, "{:#?}", flow.semantic_issues);
    assert!(summarize(&[flow]).get("f").expect("f").complete);
}

#[test]
fn sizeof_vla_type_evaluates_its_bound_but_alignof_does_not() {
    let sizeof = one("int f(int n) { int z = sizeof(int[n++]); return n + z * 0; }");
    assert_eq!(
        sizeof
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        vec![DefKind::Parameter, DefKind::IncDec],
        "{:#?}",
        sizeof.definitions
    );
    assert_eq!(
        sizeof.uses.iter().filter(|use_| use_.name == "n").count(),
        2,
        "{:#?}",
        sizeof.uses
    );

    let alignof = one("int f(int n) { int z = _Alignof(int[n++]); return n + z * 0; }");
    assert_eq!(
        alignof
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        vec![DefKind::Parameter],
        "{:#?}",
        alignof.definitions
    );
    assert_eq!(
        alignof.uses.iter().filter(|use_| use_.name == "n").count(),
        1,
        "{:#?}",
        alignof.uses
    );

    let pointer = one("int f(int n) { int z = sizeof(int (*)[n++]); return n + z * 0; }");
    assert!(pointer
        .definitions
        .iter()
        .any(|definition| { definition.name == "n" && definition.kind == DefKind::IncDec }));
    assert!(!pointer.vla_complete, "{pointer:#?}");

    for expression in [
        "sizeof(int[sizeof(n++)])",
        "sizeof(int[_Alignof(int[n++])])",
    ] {
        let flow = one(&format!(
            "int f(int n) {{ int z = {expression}; return n + z * 0; }}"
        ));
        assert_eq!(
            flow.definitions
                .iter()
                .filter(|definition| definition.name == "n")
                .map(|definition| definition.kind)
                .collect::<Vec<_>>(),
            vec![DefKind::Parameter],
            "{expression}: {:#?}",
            flow.definitions
        );
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "n").count(),
            1,
            "{expression}: {:#?}",
            flow.uses
        );
    }
}

#[test]
fn field_names_in_vla_bounds_are_not_resolved_as_local_values() {
    for bound in ["s.n", "p->n"] {
        for context in ["int values[BOUND];", "int z = sizeof(int[BOUND]);"] {
            let statement = context.replace("BOUND", bound);
            let flow = one(&format!(
                "struct S {{ int n; }}; int f(int n, struct S s, struct S *p) {{ {statement} return n; }}"
            ));
            assert_eq!(
                flow.uses.iter().filter(|use_| use_.name == "n").count(),
                1,
                "{statement}: {:#?}",
                flow.uses
            );
            let base = if bound.starts_with('s') { "s" } else { "p" };
            assert!(
                flow.uses.iter().any(|use_| use_.name == base),
                "{statement}"
            );
            assert!(!flow.vla_complete, "{statement}: {flow:#?}");
        }
    }
}

#[test]
fn opaque_calls_and_memory_accesses_in_vla_bounds_fail_closed() {
    for (parameters, bound) in [
        ("int n, int (*size)(int)", "size(n)"),
        ("int n, int *p", "p[n]"),
        ("int n, int *p", "*p + n"),
    ] {
        let flow = one(&format!(
            "int f({parameters}) {{ int values[{bound}]; return n; }}"
        ));
        assert!(!flow.vla_complete, "{bound}: {flow:#?}");
        assert!(!summarize(&[flow]).get("f").expect("f").complete, "{bound}");
    }

    for bound in ["n + 1", "n * 2", "(n + 3) / 2"] {
        let flow = one(&format!(
            "int f(int n) {{ int values[{bound}]; return n; }}"
        ));
        assert!(flow.vla_complete, "{bound}: {flow:#?}");
    }
}

#[test]
fn opaque_bound_call_retains_known_argument_flow_while_qualifying_coverage() {
    let flow = one(concat!(
        "int opaque(int); int f(int n) { int values[opaque(n)]; ",
        "return sizeof(values); }",
    ));
    assert!(!flow.vla_complete, "{flow:#?}");
    assert!(!flow.effects_complete, "{flow:#?}");
    for kind in [
        SemanticIssueKind::UnmodeledTypeValue,
        SemanticIssueKind::UnmodeledEffect,
    ] {
        assert!(
            flow.semantic_issues
                .iter()
                .any(|issue| issue.kind == kind && issue.span.is_some()),
            "{kind:?}: {flow:#?}",
        );
    }
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(!function.complete, "{function:#?}");
    assert!(function.flows_to(0, Sink::Return), "{function:#?}");
}

#[test]
fn nested_bound_call_retains_argument_provenance_and_uncertainty() {
    let flow = one(concat!(
        "int opaque(int); int f(int n) { int values[opaque(n + 1) * 2]; ",
        "return sizeof(values); }",
    ));
    assert!(!flow.vla_complete, "{flow:#?}");
    assert!(!flow.effects_complete, "{flow:#?}");
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(!function.complete, "{function:#?}");
    assert!(function.flows_to(0, Sink::Return), "{function:#?}");
}

#[test]
fn explicit_binary_bound_preserves_both_inputs_without_uncertainty() {
    let flow = one("int f(int n, int m) { int values[n + m]; return sizeof(values); }");
    assert!(flow.vla_complete, "{flow:#?}");
    assert!(!flow
        .semantic_issues
        .iter()
        .any(|issue| issue.kind == SemanticIssueKind::UnmodeledTypeValue));
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(function.complete, "{function:#?}");
    assert!(function.flows_to(0, Sink::Return), "{function:#?}");
    assert!(function.flows_to(1, Sink::Return), "{function:#?}");
}

#[test]
fn nested_binary_bound_preserves_leaf_provenance_through_value_ids() {
    let flow = one(concat!(
        "int f(int n, int m) { int values[((n + 3) * (m - 1)) / 2]; ",
        "return sizeof(values); }",
    ));
    assert!(flow.vla_complete, "{flow:#?}");
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(function.complete, "{function:#?}");
    assert!(function.flows_to(0, Sink::Return), "{function:#?}");
    assert!(function.flows_to(1, Sink::Return), "{function:#?}");
}

#[test]
fn unary_bound_preserves_nested_operand_provenance() {
    let flow = one("int f(int n) { int values[+(n + 1)]; return sizeof(values); }");
    assert!(flow.vla_complete, "{flow:#?}");
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(function.complete, "{function:#?}");
    assert!(function.flows_to(0, Sink::Return), "{function:#?}");
}

#[test]
fn pure_short_circuit_bound_preserves_both_possible_inputs() {
    let flow = one("int f(int n, int m) { int values[n && (m + 1)]; return sizeof(values); }");
    assert!(flow.vla_complete, "{flow:#?}");
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(function.complete, "{function:#?}");
    assert!(function.flows_to(0, Sink::Return), "{function:#?}");
    assert!(function.flows_to(1, Sink::Return), "{function:#?}");
}

#[test]
fn typedef_names_in_vla_bound_casts_are_type_syntax() {
    for source in [
        "typedef int T; int f(int n) { int values[(T)(n)]; return n; }",
        "int f(int n) { typedef int T; int values[(T)n]; return n; }",
    ] {
        let flow = one(source);
        assert!(flow.vla_complete, "{source}: {flow:#?}");
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "n").count(),
            2,
            "{source}: {:#?}",
            flow.uses
        );
        assert!(flow.binding_named("T").is_none(), "{source}");
    }

    let shadowed = one(concat!(
        "typedef int T; int f(int n) { int T = n;",
        "int values[(T)]; return n; }",
    ));
    assert_eq!(
        shadowed.uses.iter().filter(|use_| use_.name == "T").count(),
        1
    );
}

#[test]
fn enumerators_are_constants_not_runtime_value_dependencies() {
    for source in [
        "enum { N = 4 }; int f(void) { int values[N]; return N; }",
        "int f(void) { enum { N = 4, M = N + 1 }; int values[M]; return N; }",
    ] {
        let flow = one(source);
        assert!(flow.vla_complete, "{source}: {flow:#?}");
        assert!(flow.unresolved_bindings.is_empty(), "{source}: {flow:#?}");
        assert!(
            !flow
                .uses
                .iter()
                .any(|use_| matches!(use_.name.as_str(), "N" | "M")),
            "{source}: {:#?}",
            flow.uses
        );
    }

    let shadowed = one("enum { N = 4 }; int f(int N) { int values[N]; return N; }");
    assert!(shadowed.vla_complete, "{shadowed:#?}");
    assert_eq!(
        shadowed.uses.iter().filter(|use_| use_.name == "N").count(),
        2
    );

    let after = one("int f(void) { return N; } enum E { N = 4 };");
    assert_eq!(after.unresolved_uses.len(), 1, "{after:#?}");
    assert_eq!(after.uses[after.unresolved_uses[0] as usize].name, "N");

    let out_of_scope = one("int f(void) { { enum E { N = 4 }; } return N; }");
    assert_eq!(out_of_scope.unresolved_uses.len(), 1, "{out_of_scope:#?}");
    assert_eq!(
        out_of_scope.uses[out_of_scope.unresolved_uses[0] as usize].name,
        "N"
    );

    let sizeof = one("enum E { N = 4 }; int f(void) { return sizeof(int[N]); }");
    assert!(sizeof.vla_complete, "{sizeof:#?}");
    assert!(sizeof.uses.is_empty(), "{sizeof:#?}");
}

#[test]
fn parameter_type_enumerators_follow_function_parameter_scope() {
    for source in [
        "int f(enum { N = 2 } x, int a[N]) { return N + a[0] + x; }",
        "int f(enum E { N = 2 } x) { int a[N]; return N + x; }",
        "int f(enum { N = 2 }, int a[N]) { return N + a[0]; }",
    ] {
        let flow = one(source);
        assert!(flow.vla_complete, "{source}: {flow:#?}");
        assert!(flow.unresolved_uses.is_empty(), "{source}: {flow:#?}");
        assert!(
            !flow.uses.iter().any(|use_| use_.name == "N"),
            "{source}: {:#?}",
            flow.uses
        );
    }

    let nested = one("int f(int (*cb)(enum { N = 2 } x)) { return N; }");
    assert_eq!(nested.unresolved_uses.len(), 1, "{nested:#?}");
    assert_eq!(nested.uses[nested.unresolved_uses[0] as usize].name, "N");
}

#[test]
fn typeof_variably_modified_type_evaluates_its_array_bounds() {
    let incremented = one("int f(int n) { typeof(int[n++]) *p = 0; return n + (p != 0); }");
    assert!(!incremented.vla_complete, "{incremented:#?}");
    assert_eq!(
        incremented
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        [DefKind::Parameter, DefKind::IncDec]
    );
    assert_eq!(
        incremented
            .uses
            .iter()
            .filter(|use_| use_.name == "n")
            .count(),
        2
    );

    let scalar = one("int f(int n) { typeof(int[n + 1]) *p = 0; return n + (p != 0); }");
    assert!(scalar.vla_complete, "{scalar:#?}");
    assert_eq!(
        scalar.uses.iter().filter(|use_| use_.name == "n").count(),
        2
    );

    for declaration in [
        "typeof((int (*)[n++])0) p = 0;",
        "typeof(*(int (*)[n++])0) *p = 0;",
    ] {
        let flow = one(&format!(
            "int f(int n) {{ {declaration} return n + (p != 0); }}"
        ));
        assert_eq!(
            flow.definitions
                .iter()
                .filter(|definition| definition.name == "n")
                .map(|definition| definition.kind)
                .collect::<Vec<_>>(),
            [DefKind::Parameter, DefKind::IncDec],
            "{declaration}: {flow:#?}"
        );
        assert_eq!(
            flow.uses.iter().filter(|use_| use_.name == "n").count(),
            2,
            "{declaration}: {flow:#?}"
        );
    }

    let nested_sizeof = one(concat!(
        "int f(int n) { typeof(int[sizeof(n++)]) *p = 0;",
        "return n + (p != 0); }",
    ));
    assert_eq!(
        nested_sizeof
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        [DefKind::Parameter]
    );
    assert_eq!(
        nested_sizeof
            .uses
            .iter()
            .filter(|use_| use_.name == "n")
            .count(),
        1
    );

    let sizeof_cast = one(concat!(
        "int f(int n) { typeof(sizeof((int (*)[n++])0)) p = 0;",
        "return n + p * 0; }",
    ));
    assert_eq!(
        sizeof_cast
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        [DefKind::Parameter]
    );
    assert_eq!(
        sizeof_cast
            .uses
            .iter()
            .filter(|use_| use_.name == "n")
            .count(),
        1
    );

    let call_cast = one(concat!(
        "int sink(void *); int f(int n) {",
        "typeof(sink((int (*)[n++])0)) p = 0; return n + p * 0; }",
    ));
    assert_eq!(
        call_cast
            .definitions
            .iter()
            .filter(|definition| definition.name == "n")
            .map(|definition| definition.kind)
            .collect::<Vec<_>>(),
        [DefKind::Parameter]
    );
    assert_eq!(
        call_cast
            .uses
            .iter()
            .filter(|use_| use_.name == "n")
            .count(),
        1
    );

    for bound in ["opaque(n)", "*p"] {
        let parameters = if bound == "*p" {
            "int n, int *p"
        } else {
            "int n"
        };
        let flow = one(&format!(
            "int f({parameters}) {{ typeof(int[{bound}]) *q = 0; return n + (q != 0); }}"
        ));
        assert!(!flow.vla_complete, "{bound}: {flow:#?}");
    }
}

#[test]
fn typeof_captured_vla_does_not_claim_complete_type_provenance() {
    let direct = one("int f(int n) { int a[n]; typeof(a) b; return sizeof(b); }");
    assert!(direct.vla_complete, "{direct:#?}");
    let summary = summarize(&[direct]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(f.flows_to(0, Sink::Return), "{f:#?}");

    let parenthesized = one("int f(int n) { int a[n]; typeof(((a))) b; return sizeof(b); }");
    assert!(parenthesized.vla_complete, "{parenthesized:#?}");
    let summary = summarize(&[parenthesized]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(f.flows_to(0, Sink::Return), "{f:#?}");

    let fixed = one("int f(int n) { int a[4]; typeof(a) b; return sizeof(b) + n * 0; }");
    assert!(fixed.vla_complete, "{fixed:#?}");
}

#[test]
fn captured_vla_bound_uses_the_definition_at_type_formation() {
    let captured_before_write = one(concat!(
        "int f(int n) { int a[n]; typeof(a) b; ",
        "n = 1; return sizeof(b); }",
    ));
    let summary = summarize(&[captured_before_write]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(
        f.flows_to(0, Sink::Return),
        "the original n was captured: {f:#?}"
    );

    let captured_after_write = one(concat!(
        "int f(int n) { n = 1; int a[n]; typeof(a) b; ",
        "return sizeof(b); }",
    ));
    let flows = [captured_after_write];
    let summary = summarize(&flows);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(
        !f.flows_to(0, Sink::Return),
        "the constant assignment, not the parameter, was captured: {f:#?}\n{:#?}",
        flows[0]
    );
}

#[test]
fn comma_bound_executes_left_effect_but_captures_only_right_value() {
    let flow = one(concat!(
        "int f(int n, int m) { int values[(n++, m)]; ",
        "return sizeof(values); }",
    ));
    assert!(flow.vla_complete, "{flow:#?}");
    assert!(flow
        .definitions
        .iter()
        .any(|definition| { definition.name == "n" && definition.kind == DefKind::IncDec }));
    let summary = summarize(&[flow]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(!f.flows_to(0, Sink::Return), "discarded n value: {f:#?}");
    assert!(f.flows_to(1, Sink::Return), "captured m value: {f:#?}");

    let constant = one("int f(int n) { int values[(n++, 4)]; return sizeof(values); }");
    assert!(constant.vla_complete, "{constant:#?}");
    assert!(constant
        .definitions
        .iter()
        .any(|definition| { definition.name == "n" && definition.kind == DefKind::IncDec }));
    let summary = summarize(&[constant]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(
        !f.flows_to(0, Sink::Return),
        "constant comma result: {f:#?}"
    );
}

#[test]
fn conditional_bound_captures_condition_and_both_possible_values() {
    let flow = one(concat!(
        "int f(int condition, int left, int right) { ",
        "int values[condition ? left : right]; return sizeof(values); }",
    ));
    assert!(flow.vla_complete, "{flow:#?}");
    let summary = summarize(&[flow]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    for parameter in 0..3 {
        assert!(f.flows_to(parameter, Sink::Return), "{parameter}: {f:#?}");
    }

    let constant_arm = one(concat!(
        "int f(int condition, int value) { ",
        "int values[condition ? value : 4]; return sizeof(values); }",
    ));
    let summary = summarize(&[constant_arm]);
    let f = summary.get("f").expect("f");
    assert!(f.complete, "{f:#?}");
    assert!(f.flows_to(0, Sink::Return));
    assert!(f.flows_to(1, Sink::Return));
}

#[test]
fn conditional_expression_arms_preserve_nested_leaf_provenance() {
    let flow = one(concat!(
        "int f(int c, int n, int m) { int values[c ? (n + 1) : +(m * 2)]; ",
        "return sizeof(values); }",
    ));
    assert!(flow.vla_complete, "{flow:#?}");
    let summary = summarize(&[flow]);
    let function = summary.get("f").expect("f summary");
    assert!(function.complete, "{function:#?}");
    for parameter in 0..3 {
        assert!(function.flows_to(parameter, Sink::Return), "{function:#?}");
    }
}

#[test]
fn semantic_issue_ledger_explains_compatibility_flags() {
    use super::{CoverageDimension, SemanticIssueKind};

    let parsed = analyze("int f(int n) { int a[n]; typeof(a) b; return sizeof(b); }");
    let flow = &parsed.value()[0];
    assert_eq!(flow.function_id.0, 0);
    assert!(!flow.source_id.name().is_empty());
    assert_eq!(flow.analysis_revision, 5);
    assert!(flow.recovery_free);
    assert!(flow.vla_complete);
    assert!(flow.covers(CoverageDimension::TypeValue));
    assert!(!flow
        .semantic_issues
        .iter()
        .any(|issue| issue.kind == SemanticIssueKind::UnmodeledTypeValue));

    let recovered = analyze("int f(int n) { return n }");
    assert!(!recovered.value().is_empty());
    assert!(!recovered.diagnostics().is_empty());
    for flow in recovered.value() {
        assert!(!flow.recovery_free);
        assert!(flow.semantic_issues.iter().any(|issue| {
            issue.kind == SemanticIssueKind::RecoveredSyntax
                && issue.span.is_some()
                && issue.kind.dimensions().contains(&CoverageDimension::Syntax)
        }));
    }
}

#[test]
fn conservative_dispatch_qualifies_control_targets_and_summaries() {
    use super::{CoverageDimension, SemanticIssueKind};

    let flow = one(concat!(
        "int f(int n) { void *p = &&a; if (n) p = &&b; goto *p; ",
        "a:return 1; b:return 2; }",
    ));
    assert!(!flow.control_targets_complete);
    assert!(!flow.covers(CoverageDimension::ControlTargets));
    assert!(flow.semantic_issues.iter().any(|issue| {
        issue.kind == SemanticIssueKind::UnresolvedControlTarget && issue.span.is_some()
    }));
    assert!(!summarize(&[flow]).get("f").expect("summary").complete);

    let exact = one("int g(void) { goto *&&done; done:return 1; }");
    assert!(exact.control_targets_complete);
    assert!(exact.covers(CoverageDimension::ControlTargets));
    assert!(
        summarize(std::slice::from_ref(&exact))
            .get("g")
            .expect("summary")
            .complete,
        "{exact:#?}"
    );

    let nested = one("int h(void **pp) { void *q=&&a; goto **pp; a:return q != 0; }");
    assert!(!nested.control_targets_complete);
    assert!(nested.memory_complete, "{nested:#?}");
}

#[test]
fn unresolved_declared_types_qualify_all_type_dependent_negative_claims() {
    use super::{CoverageDimension, SemanticIssueKind};

    let flow = one("int f(HeaderType value) { return 0; }");
    assert!(flow
        .semantic_issues
        .iter()
        .any(|issue| { issue.kind == SemanticIssueKind::UnknownType && issue.span.is_some() }));
    assert!(!flow.effects_complete);
    assert!(!flow.memory_complete);
    assert!(!flow.vla_complete);
    assert!(!flow.covers(CoverageDimension::Effects));
    assert!(!flow.covers(CoverageDimension::Memory));
    assert!(!flow.covers(CoverageDimension::TypeValue));
    assert!(!summarize(&[flow]).get("f").expect("summary").complete);
}

#[test]
fn one_analysis_unit_owns_dense_function_and_source_identity() {
    use crate::csource::semantic::AnalysisUnit;

    let unit = AnalysisUnit::new("int a(void){return 1;} int b(void){return a();}");
    let flows = super::analyze_unit(&unit);
    assert_eq!(flows.len(), 2);
    assert_eq!(flows[0].function_id.0, 0);
    assert_eq!(flows[1].function_id.0, 1);
    assert_eq!(flows[0].source_id, unit.source_id());
    assert_eq!(flows[1].source_id, unit.source_id());
    assert_eq!(flows[0].source_id, flows[1].source_id);
}

#[test]
fn initializer_and_return_reads_consume_common_evaluation_placement() {
    use crate::csource::eval::{EvaluationPurpose, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let source = "int f(int n, int m){int x=n ? n+1 : m+2;return x+n;}";
    let unit = AnalysisUnit::new(source);
    let flow = &unit.dataflows()[0];
    let plan = &unit.evaluations()[0];

    for operation in plan.operations() {
        if !operation.kind.scalar_inputs().is_empty() {
            for operand in plan.direct_scalar_inputs_of(operation) {
                let occurrence = operand.occurrence();
                if operand.declaration().is_none() {
                    continue;
                }
                let matching = flow
                    .uses
                    .iter()
                    .filter(|use_| use_.span == occurrence)
                    .collect::<Vec<_>>();
                assert_eq!(matching.len(), 1, "{operation:?}");
                assert_eq!(matching[0].node, operation.cfg_node, "{operation:?}");
            }
        }
    }

    let initializer = plan
        .operations()
        .iter()
        .find_map(|operation| match operation.kind {
            TypeValueOp::FinishExpression {
                purpose: EvaluationPurpose::Initialize { declaration, .. },
                expression,
                ..
            } => Some((declaration, expression, operation.cfg_node)),
            _ => None,
        })
        .expect("initializer root");
    let definition = flow
        .definitions
        .iter()
        .find(|definition| definition.span == initializer.0)
        .expect("initialized declaration");
    assert_eq!(definition.node, initializer.2);
    assert_eq!(definition.effect_at, initializer.1.hi);
}

#[test]
fn initializer_and_return_writes_replace_legacy_promoted_definitions() {
    use crate::csource::eval::{ScalarWriteKind, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let source = concat!(
        "int f(int c,int n,int m){",
        "int x=c ? n++ : (m=n+1);",
        "return (x+=n);}",
    );
    let unit = AnalysisUnit::new(source);
    let flow = &unit.dataflows()[0];
    let plan = &unit.evaluations()[0];
    let writes = plan
        .operations()
        .iter()
        .filter_map(|operation| match &operation.kind {
            TypeValueOp::WriteScalar {
                occurrence,
                span,
                kind,
                ..
            } => Some((operation, *occurrence, *span, *kind)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 3);

    for (operation, occurrence, span, kind) in writes {
        let definitions = flow
            .definitions
            .iter()
            .filter(|definition| definition.span == occurrence)
            .collect::<Vec<_>>();
        assert_eq!(definitions.len(), 1, "{operation:?}");
        assert_eq!(definitions[0].node, operation.cfg_node, "{operation:?}");
        assert_eq!(definitions[0].effect_at, span.hi, "{operation:?}");
        if kind == ScalarWriteKind::Assign {
            assert!(
                flow.uses.iter().all(|use_| use_.span != occurrence),
                "plain assignment target must not be read: {operation:?}"
            );
        } else {
            let uses = flow
                .uses
                .iter()
                .filter(|use_| use_.span == occurrence)
                .collect::<Vec<_>>();
            assert_eq!(
                uses.len(),
                1,
                "read-modify-write target: {operation:?}; uses={uses:?}"
            );
            assert_eq!(uses[0].node, operation.cfg_node, "{operation:?}");
        }
    }
}

#[test]
fn ordinary_call_roots_preserve_call_policy_and_result_identity() {
    use crate::csource::eval::{ExecutionCondition, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let direct = AnalysisUnit::new("int id(int x){return x;} int f(int n){return id(n+1);}");
    let plan = &direct.evaluations()[1];
    let flow = &direct.dataflows()[1];
    let call = plan
        .operations()
        .iter()
        .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
        .expect("ordinary call operation");
    assert!(plan.operations().iter().any(|operation| {
        matches!(operation.kind, TypeValueOp::FinishExpression { .. })
            && operation.inputs.as_slice() == [call.output]
    }));
    assert_eq!(flow.calls.len(), 1);
    assert!(flow.calls[0].result_is_returned);
    let TypeValueOp::CallScalar { arguments, .. } = &call.kind else {
        unreachable!("selected operation is a call")
    };
    assert_eq!(flow.calls[0].argument_spans, [arguments[0].occurrence()]);
    assert!(flow.calls[0].arguments[0].is_free());
    assert!(flow
        .semantic_issues
        .iter()
        .all(|issue| issue.kind != SemanticIssueKind::UnmodeledEffect));
    let summaries = direct.summaries();
    let f = summaries.get_by_id(flow.function_id).expect("f summary");
    assert!(f.complete);
    assert!(f.flows_to(0, Sink::Return));

    let transformed = AnalysisUnit::new("int id(int x){return x;} int f(int n){return id(n)+1;}");
    assert!(transformed.evaluations()[1]
        .operations()
        .iter()
        .any(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. })));
    assert!(!transformed.dataflows()[1].calls[0].result_is_returned);

    let bare = AnalysisUnit::new("int id(int x){return x;} int f(int n){return id(n);}");
    let call = bare.evaluations()[1]
        .operations()
        .iter()
        .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
        .expect("ordinary bare-argument call");
    let TypeValueOp::CallScalar { arguments, .. } = &call.kind else {
        unreachable!("selected operation is a call")
    };
    assert_eq!(
        bare.dataflows()[1].calls[0].argument_spans,
        [arguments[0].occurrence()]
    );
    assert!(!bare.dataflows()[1].calls[0].arguments[0].is_free());

    let siblings = AnalysisUnit::new(concat!(
        "int a(int x){return x;} int b(int x){return x;} ",
        "int f(int n){return a(n)+b(n);}",
    ));
    let flow = &siblings.dataflows()[2];
    assert_eq!(flow.calls.len(), 2);
    assert!(flow.calls.iter().all(|call| !call.result_is_returned));
    let summary = siblings
        .summaries()
        .get_by_id(flow.function_id)
        .expect("two-call caller summary");
    assert!(summary.complete);
    assert!(summary.flows_to(0, Sink::Return));

    for source in [
        "int id(int x){return x;} int f(int n){return n=id(n);}",
        "int id(int x){return x;} int f(int n){return id(n=1);}",
    ] {
        let ordered = AnalysisUnit::new(source);
        let plan = &ordered.evaluations()[1];
        let flow = &ordered.dataflows()[1];
        let write = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
            .expect("ordered call/write operation");
        let TypeValueOp::WriteScalar { span, .. } = write.kind else {
            unreachable!("selected operation is a write")
        };
        let definitions = flow
            .definitions
            .iter()
            .filter(|definition| definition.name == "n" && definition.kind == DefKind::Assignment)
            .collect::<Vec<_>>();
        assert_eq!(definitions.len(), 1, "{source}: {definitions:?}");
        assert_eq!(definitions[0].node, write.cfg_node);
        assert_eq!(definitions[0].effect_at, span.hi);
        assert_eq!(flow.calls.len(), 1);
    }

    for source in [
        concat!(
            "int id(int x){return x;} ",
            "int f(int c,int n){return c?id(n):(n=1);}",
        ),
        concat!(
            "int id(int x){return x;} ",
            "int f(int c,int n){return c?(n=1):id(n);}",
        ),
    ] {
        let alternative = AnalysisUnit::new(source);
        let plan = &alternative.evaluations()[1];
        let flow = &alternative.dataflows()[1];
        let write = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
            .expect("alternative write");
        assert!(matches!(write.execution, ExecutionCondition::Guarded(_)));
        let definitions = flow
            .definitions
            .iter()
            .filter(|definition| definition.name == "n" && definition.kind == DefKind::Assignment)
            .collect::<Vec<_>>();
        assert_eq!(definitions.len(), 1, "{source}: {definitions:?}");
        assert_eq!(definitions[0].node, write.cfg_node);
        assert_eq!(flow.calls.len(), 1);
    }

    let intrinsic = AnalysisUnit::new("int f(int n){return __builtin_expect(n+1,1);}");
    assert!(intrinsic.evaluations()[0]
        .operations()
        .iter()
        .any(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. })));
    let summary = intrinsic
        .summaries()
        .get_by_id(intrinsic.dataflows()[0].function_id)
        .expect("intrinsic caller summary");
    assert!(summary.complete);
    assert!(summary.flows_to(0, Sink::Return));
}

#[test]
fn unsequenced_scalar_conflicts_are_localized_and_cannot_certify_effects() {
    for source in [
        "int f(int n){return n++ + n;}",
        "int f(int n){return n++ + n++;}",
        "int a(int); int f(int n){return a(n) + (n=1);}",
    ] {
        let flow = one(source);
        assert!(flow.semantic_issues.iter().any(|issue| {
            issue.kind == SemanticIssueKind::UnsequencedAccess && issue.span.is_some()
        }));
        assert!(!flow.effects_complete, "{source}: {flow:?}");
        assert!(!summarize(&[flow]).get("f").expect("f").complete);
    }

    for source in [
        "int f(int n){return n++ && n;}",
        "int f(int c,int n){return c ? n++ : n;}",
    ] {
        let flow = one(source);
        assert!(flow
            .semantic_issues
            .iter()
            .all(|issue| issue.kind != SemanticIssueKind::UnsequencedAccess));
    }
}

#[test]
fn ordinary_comma_roots_preserve_effects_without_returning_discarded_values() {
    let discarded = AnalysisUnit::new("int f(int n){return (n, 0);}");
    let summary = discarded
        .summaries()
        .get_by_id(discarded.dataflows()[0].function_id)
        .expect("discarded-value summary");
    assert!(summary.complete);
    assert!(!summary.flows_to(0, Sink::Return));

    let incremented = AnalysisUnit::new("int f(int n){return (n++, n);}");
    let flow = &incremented.dataflows()[0];
    assert_eq!(
        flow.definitions
            .iter()
            .filter(|definition| { definition.name == "n" && definition.kind == DefKind::IncDec })
            .count(),
        1
    );
    let summary = incremented
        .summaries()
        .get_by_id(flow.function_id)
        .expect("sequenced increment summary");
    assert!(summary.complete);
    assert!(summary.flows_to(0, Sink::Return));

    let called = AnalysisUnit::new("int id(int x){return x;} int f(int n){return (id(n), 0);}");
    let flow = &called.dataflows()[1];
    assert_eq!(flow.calls.len(), 1);
    assert!(!flow.calls[0].result_is_returned);
    let summary = called
        .summaries()
        .get_by_id(flow.function_id)
        .expect("discarded-call summary");
    assert!(summary.complete);
    assert!(!summary.flows_to(0, Sink::Return));
}

#[test]
fn expression_statements_use_operation_owned_reads_writes_and_calls() {
    use crate::csource::eval::{EvaluationPurpose, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let unit = AnalysisUnit::new(concat!(
        "int id(int x){return x;} ",
        "int f(int n){id(n+1); n++; n+1; return n;}",
    ));
    let plan = &unit.evaluations()[1];
    let flow = &unit.dataflows()[1];
    assert_eq!(
        plan.operations()
            .iter()
            .filter(|operation| matches!(
                operation.kind,
                TypeValueOp::FinishExpression {
                    purpose: EvaluationPurpose::Discard { .. },
                    ..
                }
            ))
            .count(),
        3
    );
    assert_eq!(flow.calls.len(), 1);
    assert!(!flow.calls[0].result_is_returned);
    assert_eq!(
        flow.definitions
            .iter()
            .filter(|definition| { definition.name == "n" && definition.kind == DefKind::IncDec })
            .count(),
        1
    );
    assert!(flow
        .semantic_issues
        .iter()
        .all(|issue| issue.kind != SemanticIssueKind::UnsequencedAccess));

    let conflicted = one("int f(int n){n++ + n; return n;}");
    assert!(conflicted
        .semantic_issues
        .iter()
        .any(|issue| issue.kind == SemanticIssueKind::UnsequencedAccess));
    assert!(!conflicted.effects_complete);
}

#[test]
fn control_conditions_use_operation_owned_reads_writes_and_calls() {
    use crate::csource::eval::{EvaluationPurpose, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let unit = AnalysisUnit::new(concat!(
        "int id(int x){return x;} ",
        "int f(int n){if(id(n+1)){n++;} while(n--){break;} return n;}",
    ));
    let plan = &unit.evaluations()[1];
    let flow = &unit.dataflows()[1];
    assert_eq!(
        plan.operations()
            .iter()
            .filter(|operation| matches!(
                operation.kind,
                TypeValueOp::FinishExpression {
                    purpose: EvaluationPurpose::Control { .. },
                    ..
                }
            ))
            .count(),
        2
    );
    assert_eq!(flow.calls.len(), 1);
    assert!(!flow.calls[0].result_is_returned);

    let writes = plan
        .operations()
        .iter()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::WriteScalar {
                occurrence, span, ..
            } => Some((operation, occurrence, span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 2);
    for (operation, occurrence, span) in writes {
        let definitions = flow
            .definitions
            .iter()
            .filter(|definition| definition.span == occurrence)
            .collect::<Vec<_>>();
        assert_eq!(definitions.len(), 1, "{operation:?}");
        assert_eq!(definitions[0].node, operation.cfg_node, "{operation:?}");
        assert_eq!(definitions[0].effect_at, span.hi, "{operation:?}");
    }

    let conflicted = one("int f(int n){if(n++ + n){} return n;}");
    assert!(conflicted
        .semantic_issues
        .iter()
        .any(|issue| issue.kind == SemanticIssueKind::UnsequencedAccess));
    assert!(!conflicted.effects_complete);
}

#[test]
fn expression_for_clauses_use_operation_owned_writes_and_calls() {
    use crate::csource::eval::{EvaluationPurpose, ForClausePhase, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let unit = AnalysisUnit::new(concat!(
        "int id(int x){return x;} ",
        "int f(int n){for(n=id(n);n<3;n++){continue;} return n;}",
    ));
    let plan = &unit.evaluations()[1];
    let flow = &unit.dataflows()[1];
    let clauses = plan
        .operations()
        .iter()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::FinishExpression {
                purpose: EvaluationPurpose::ForClause { phase, .. },
                ..
            } => Some((phase, operation.cfg_node)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        clauses.iter().map(|(phase, _)| *phase).collect::<Vec<_>>(),
        [ForClausePhase::Init, ForClausePhase::Step]
    );
    assert_ne!(clauses[0].1, clauses[1].1);
    assert_eq!(flow.calls.len(), 1);
    assert!(!flow.calls[0].result_is_returned);

    let writes = plan
        .operations()
        .iter()
        .filter_map(|operation| match operation.kind {
            TypeValueOp::WriteScalar {
                occurrence, span, ..
            } => Some((operation, occurrence, span)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 2);
    for (operation, occurrence, span) in writes {
        let definitions = flow
            .definitions
            .iter()
            .filter(|definition| definition.span == occurrence)
            .collect::<Vec<_>>();
        assert_eq!(definitions.len(), 1, "{operation:?}");
        assert_eq!(definitions[0].node, operation.cfg_node, "{operation:?}");
        assert_eq!(definitions[0].effect_at, span.hi, "{operation:?}");
    }
}

#[test]
fn computed_goto_operands_use_operation_owned_reads_and_calls() {
    use crate::csource::eval::{EvaluationPurpose, TypeValueOp};
    use crate::csource::semantic::AnalysisUnit;

    let unit = AnalysisUnit::new(concat!(
        "void *pick(void *p){return p;} ",
        "int f(void *p){goto *pick(p);}",
    ));
    let plan = &unit.evaluations()[1];
    let flow = &unit.dataflows()[1];
    let finish = plan
        .operations()
        .iter()
        .find(|operation| {
            matches!(
                operation.kind,
                TypeValueOp::FinishExpression {
                    purpose: EvaluationPurpose::IndirectDispatch { .. },
                    ..
                }
            )
        })
        .expect("computed-goto operation root");
    assert_eq!(flow.calls.len(), 1);
    assert!(!flow.calls[0].result_is_returned);
    let call = plan
        .operations()
        .iter()
        .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
        .expect("dispatch call");
    assert_eq!(finish.inputs, [call.output]);
    let TypeValueOp::CallScalar { arguments, .. } = &call.kind else {
        unreachable!("selected operation is a call")
    };
    let occurrence = arguments[0].occurrence();
    let uses = flow
        .uses
        .iter()
        .filter(|use_| use_.span == occurrence)
        .collect::<Vec<_>>();
    assert_eq!(uses.len(), 1);
    assert_eq!(uses[0].node, call.cfg_node);

    let conflicted = one("int f(void *p){goto *(p++ + p);}");
    assert!(conflicted
        .semantic_issues
        .iter()
        .any(|issue| issue.kind == SemanticIssueKind::UnsequencedAccess));
    assert!(!conflicted.effects_complete);
}

#[test]
fn parameter_array_extents_read_only_prior_outer_parameters() {
    for declaration in [
        "int values[n]",
        "int values[static n]",
        "int (*values)[n]",
        "int (values[n])",
        "int ((values[n]))",
        "int (*values[n])(void)",
    ] {
        let flow = one(&format!(
            "int f(int n, {declaration}) {{ return values != 0; }}"
        ));
        assert_eq!(flow.uses.iter().filter(|use_| use_.name == "n").count(), 1);
        assert!(flow.vla_complete, "{declaration}");
    }
    let nested = one("int f(int n, void (*callback)(int nested[n])) { callback(0); return n; }");
    assert_eq!(
        nested.uses.iter().filter(|use_| use_.name == "n").count(),
        1
    );

    let multidimensional = one("int f(int n, int m, int values[(n)][m]) { return values != 0; }");
    assert_eq!(
        multidimensional
            .uses
            .iter()
            .filter(|use_| use_.name == "n")
            .count(),
        1
    );
    assert_eq!(
        multidimensional
            .uses
            .iter()
            .filter(|use_| use_.name == "m")
            .count(),
        1
    );
    assert!(multidimensional.vla_complete);

    let later = one("int f(int values[n], int n) { return values != 0; }");
    assert!(!later.vla_complete, "a later parameter is not yet in scope");
    assert!(!later.uses.iter().any(|use_| use_.name == "n"));
}

#[test]
fn a_block_scope_typedef_stops_at_the_end_of_its_scope() {
    let flow = one("int f(void) { { typedef unsigned long word_t; } return word_t; }");
    assert!(flow
        .unresolved_bindings
        .contains(&flow.binding_named("word_t").unwrap()));
    assert_eq!(flow.unresolved_uses.len(), 1);
}

#[test]
fn qualifiers_are_flagged_and_ignored_when_comparing_shape() {
    let flow = one("int f(void) { const volatile int a = 1; int b = 2; return a + b; }");
    let a = type_of(&flow, "a").expect("a");
    let b = type_of(&flow, "b").expect("b");
    assert!(a.is_const && a.is_volatile);
    assert!(!b.is_const && !b.is_volatile);
    assert!(a.same_shape(&b), "const int and int are the same shape");
}

#[test]
fn two_spellings_we_cannot_resolve_are_reported_as_different() {
    // Without `#include`, `uint32_t` and `unsigned int` are two opaque names.
    // Saying they differ is honest; guessing they match would not be.
    let flow = one("int f(void) { uint32_t a = 0; unsigned int b = 0; return (int)(a + b); }");
    let a = type_of(&flow, "a").expect("a");
    let b = type_of(&flow, "b").expect("b");
    assert!(!a.same_shape(&b));
}

#[test]
fn a_multidimensional_array_counts_every_rank() {
    let flow = one("int f(void) { int m[4][4]; return m[0][0]; }");
    let ty = type_of(&flow, "m").expect("m");
    assert_eq!(ty.array_rank, 2, "{ty:?}");
}

#[test]
fn a_declaration_without_an_initializer_still_has_a_type() {
    // `int x;` writes nothing -- it is not a definition -- but it is a binding
    // and it has a type. The two facts are independent.
    let flow =
        one("int f(int n) { unsigned long slot; slot = (unsigned long)n; return (int)slot; }");
    let binding = flow
        .definitions
        .iter()
        .find(|d| d.name == "slot")
        .map(|d| d.binding)
        .expect("a binding for slot");
    let ty = flow.type_of(binding).expect("a type for slot");
    assert_eq!(ty.specifiers, "unsigned long");
}

#[test]
fn well_typed_source_has_no_type_conflicts() {
    let flow = one("int f(int n) { int a = n; int b = a; return b; }");
    assert!(
        flow.type_conflicts().is_empty(),
        "{:?}",
        flow.type_conflicts()
    );
}

#[test]
fn a_free_binding_has_no_type() {
    // A global's type is not knowable from one translation unit.
    let flow = one("int f(void) { return g; }");
    assert!(flow.type_of(Binding::FREE).is_none());
}

#[test]
fn type_recovery_is_deterministic() {
    let text = "int f(const char *s, int n) { unsigned long t = 0; return (int)t; }";
    assert_eq!(one(text).types, one(text).types);
}

#[test]
fn a_declared_name_is_reachable_even_when_nothing_mentions_it_again() {
    // `int *b;` writes nothing and reads nothing, so it is in neither the
    // definition list nor the use list. Without the binding-name table there
    // is no way to ask about it at all, and an unused local is unreportable.
    let flow = one("int f(void) { int a = 1, *b, c[4]; return a; }");
    let binding = flow.binding_named("b").expect("a binding for b");
    let ty = flow.type_of(binding).expect("a type for b");
    assert_eq!((ty.specifiers.as_str(), ty.pointer_depth), ("int", 1));
}

#[test]
fn an_unused_local_is_reported_and_a_parameter_is_not() {
    let flow = one("int f(int unused_param) { int unused_local; return 0; }");
    let unused: Vec<&str> = flow
        .unused_bindings()
        .iter()
        .filter_map(|b| flow.names.get(b.0 as usize).map(|s| s.as_str()))
        .collect();
    assert_eq!(unused, vec!["unused_local"], "{unused:?}");
}

#[test]
fn a_binding_used_as_projected_storage_is_not_unused() {
    let flow = one("int f(int i){int a[4];a[i]=1;return a[0];}");
    let unused = flow
        .unused_bindings()
        .into_iter()
        .map(|binding| flow.names[binding.0 as usize].as_str())
        .collect::<Vec<_>>();
    assert!(!unused.contains(&"a"), "{unused:?}");
}

#[test]
fn the_corpus_recovers_a_type_for_almost_every_declared_binding() {
    // The gate the plan states: every binding resolves to a specifier or is
    // explicitly empty, and the empty count is reported rather than assumed.
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/decompiler_fixtures/src");
    let mut bindings = 0usize;
    let mut typed = 0usize;
    let mut unresolved_bindings = 0usize;
    let mut conflicts = 0usize;
    let mut unused = 0usize;
    let mut unused_names = Vec::new();
    let mut untyped_examples: Vec<String> = Vec::new();

    for (_path, text) in crate::test_corpus::sources(&root) {
        for flow in analyze(&text).into_parts().0 {
            // The three tables are one row per binding and must agree.
            assert_eq!(flow.names.len(), flow.types.len(), "{}", flow.name);
            bindings += flow.names.len();
            unresolved_bindings += flow.unresolved_bindings.len();
            for binding in &flow.unresolved_bindings {
                assert!(flow.types[binding.0 as usize].is_empty());
            }
            for binding in flow
                .definitions
                .iter()
                .map(|d| d.binding)
                .chain(flow.uses.iter().map(|u| u.binding))
            {
                assert!((binding.0 as usize) < flow.names.len());
            }
            for (index, ty) in flow.types.iter().enumerate() {
                if ty.is_empty() {
                    if untyped_examples.len() < 8 {
                        untyped_examples.push(format!(
                            "{}:{}",
                            flow.name,
                            flow.names.get(index).cloned().unwrap_or_default()
                        ));
                    }
                } else {
                    typed += 1;
                }
            }
            conflicts += flow.type_conflicts().len();
            unused += flow.unused_bindings().len();
            for binding in flow.unused_bindings() {
                unused_names.push(format!("{}:{}", flow.name, flow.names[binding.0 as usize]));
            }
        }
    }

    assert!(bindings > 1000, "only {bindings} bindings");
    let declared = bindings - unresolved_bindings;
    let rate = typed as f64 / declared as f64;
    eprintln!(
        "corpus types: {typed}/{bindings} total; {unresolved_bindings} unresolved; {typed}/{declared} declared = {:.1}%  conflicts={conflicts}  unused={unused}",
        rate * 100.0
    );
    if !untyped_examples.is_empty() {
        eprintln!("  untyped examples: {untyped_examples:?}");
    }
    // Hand-written C declares a type for everything it binds. A rate below
    // this means the reader is losing declarations, not that the corpus is
    // untyped.
    assert!(
        rate > 0.95,
        "only {:.1}% of bindings carry a type",
        rate * 100.0
    );
    // Well-typed source cannot contain a type conflict: a C compiler would
    // have rejected it. Any is a bug in the reader.
    assert_eq!(conflicts, 0, "type conflicts in hand-written C");
    // This pointer is referenced only by sizeof, which does not read its value.
    // Pin the actual binding, not merely a relaxed count threshold.
    assert_eq!(unused, 1, "unused value bindings: {unused_names:?}");
    assert_eq!(unused_names, vec!["sizeof_array_versus_pointer:pointer"]);
}
