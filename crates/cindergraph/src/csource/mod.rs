//! The C source front end.
//!
//! C-specific code sitting on the language-neutral [`crate::syntax`] substrate:
//! token kinds, node tags, parsing, normalization, metrics, control flow,
//! dependence analysis, graph export and the separate parity projection.

pub mod cfg;
pub mod dataflow;
pub(crate) mod eval;
pub mod export;
pub mod facts;
pub mod lex;
pub mod metrics;
pub mod normalize;
pub mod parity;
pub mod parse;
pub mod semantic;
