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

//! Tolerant, deterministic analysis of C and decompiler-shaped C source.

pub mod csource;
pub mod syntax;

pub use csource::dataflow;
pub use csource::export;
pub use csource::metrics;
pub use csource::normalize;
pub use csource::parity;
pub use csource::parse;
