#![forbid(unsafe_code)]
// Mechanical extraction preserves several deliberate formulations whose
// readability or test intent Clippy cannot infer. Burn this list down without
// mixing semantic rewrites into the initial history-preserving split.
#![allow(
    clippy::assertions_on_constants,
    clippy::explicit_counter_loop,
    clippy::get_first,
    clippy::identity_op,
    clippy::if_same_then_else,
    clippy::len_zero,
    clippy::manual_slice_size_calculation,
    clippy::needless_range_loop,
    clippy::type_complexity,
    clippy::useless_vec
)]

//! Native, recovery-oriented analysis of C and decompiler-shaped C source.
//!
//! Cindergraph provides a pure Rust parser and analysis stack. It measures
//! functions, constructs control and dependence graphs, computes reaching
//! definitions and bounded call summaries, and serializes graphs without a
//! JVM or external analysis service.
//!
//! # Quick start
//!
//! Analysis results carry a recovered value and diagnostics together:
//!
//! ```
//! use cindergraph::metrics;
//!
//! let parsed = metrics::analyze("int answer(void) { return 42; }");
//! let (report, diagnostics) = parsed.into_parts();
//!
//! assert!(diagnostics.is_empty());
//! assert_eq!(report.functions[0].name, "answer");
//! assert_eq!(report.functions[0].graph.cyclomatic, 1);
//! ```
//!
//! Export uses the general control-flow graph and the language-neutral writer:
//!
//! ```
//! use cindergraph::export::{self, Repr};
//! use cindergraph::syntax::graph_export::{write, Format};
//!
//! let parsed = export::export("int f(int x) { return x; }", Repr::Cfg);
//! assert!(parsed.diagnostics().is_empty());
//! let json = write(&parsed.value()[0], Format::Json);
//! assert!(json.contains("\"directed\": true"));
//! ```
//!
//! # Choosing an API
//!
//! - [`metrics`] measures one translation unit and its recovered functions.
//! - [`parse`] exposes the C syntax tree and diagnostics.
//! - [`dataflow`] provides definitions, uses, reaching edges and call summaries.
//! - [`export`] builds AST, CFG, DDG, CDG and PDG views.
//! - [`normalize`] prepares supported preprocessed or decompiler-shaped input.
//! - [`parity`] is a narrow comparison projection, not the general CFG.
//! - [`syntax`] contains the language-neutral token, tree, CFG and writer types.
//!
//! # Recovery and assurance boundary
//!
//! The parser is deliberately tolerant: useful partial results can accompany
//! diagnostics. Callers must inspect those diagnostics and the dataflow
//! `recovery_free`, `memory_complete` and summary `complete` signals before
//! treating an absent edge as evidence of independence. These signals cover
//! different gaps and are not a proof of general C soundness.
//!
//! Cindergraph does not preprocess headers, resolve ABI layouts, implement all
//! aliasing or global side effects, build a code property graph, or provide a
//! general Joern-compatible query API. Public APIs and serialized schemas are
//! pre-1.0 and can change between minor releases.

pub mod csource;
pub mod syntax;

#[cfg(test)]
mod test_corpus;

pub use csource::dataflow;
pub use csource::export;
pub use csource::metrics;
pub use csource::normalize;
pub use csource::parity;
pub use csource::parse;
