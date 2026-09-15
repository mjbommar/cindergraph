//! The language-neutral parsing substrate.
//!
//! The C front end sits on this layer: source maps and spans, symbol interning,
//! a struct-of-arrays token buffer, diagnostics, the parser event stream, an
//! arena tree, recovery primitives, and a CFG builder that consumes
//! control-flow events rather than C syntax.
//!
//! Four rules hold across the whole module and are checked, not merely
//! intended:
//!
//! * **Language neutrality.** Nothing here names a token kind, node tag,
//!   keyword or grammar rule of any specific language (`REQ-SYN-1`). Languages
//!   supply `u16` tags; the substrate never interprets them.
//! * **Parsing never fails.** Entry points return their product alongside a
//!   diagnostic list rather than choosing between the two in a `Result`.
//!   Resource bounds and malformed-input tests keep recovery failures visible.
//! * **Explicit-stack traversal.** No native recursion in the lexer, the parser
//!   or any tree walk (`REQ-SYN-3`). Decompiler output is adversarial in
//!   exactly this way, and a process that aborts on stack exhaustion cannot
//!   report anything at all.
//! * **Stable construction order.** IDs are assigned in construction order and
//!   collections written to output are ordered. Cross-process tests vary hash
//!   seeds and compare serialized output byte for byte.

pub mod cfg;
pub mod diag;
pub mod dominance;
pub mod event;
pub mod ged;
pub mod graph_export;
pub mod ids;
pub mod intern;
pub mod metrics;
pub mod recover;
pub mod scan;
pub mod source;
pub mod token;
pub mod tree;

pub use ids::{DiagId, NodeId, Span, Symbol, TokenId};
