//! The C source front end.
//!
//! C-specific code sitting on the language-neutral substrate in
//! [`crate::syntax`]: token kinds, node tags, the grammar, control-flow event
//! emission, and the parity layer used to reproduce
//! DecBench's structural metric.

pub mod cfg;
pub mod dataflow;
pub mod export;
pub mod lex;
pub mod metrics;
pub mod normalize;
pub mod parity;
pub mod parse;
