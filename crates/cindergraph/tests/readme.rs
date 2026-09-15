//! Executable contracts for the public Rust quick start.

use cindergraph::metrics;

#[test]
fn metrics_quick_start_matches_readme() {
    let parsed = metrics::analyze("int answer(void) { return 42; }");
    let (report, diagnostics) = parsed.into_parts();

    assert!(diagnostics.is_empty());
    assert_eq!(report.functions[0].name, "answer");
    assert_eq!(report.functions[0].graph.cyclomatic, 1);
}
