//! The `Flow` event vocabulary the CFG builder consumes, plus the node and
//! edge kinds and the two graph element types (`CfgNode`, `CfgEdge`) built
//! from them.
//!
//! This is the *vocabulary*, kept separate from the graph type (`super`) and
//! the builder (`super::build`) because it is what a second language front
//! end reuses unchanged: it supplies only the grammar that emits `Flow`
//! events, never a token, node tag or keyword.

use crate::syntax::ids::{NodeId, Span, Symbol};

/// The most nodes one function's CFG may contain before the builder stops.
///
/// `REQ-SYN-4` requires a work budget on every entry point: a malformed or
/// adversarial event stream must produce a diagnostic and a partial result
/// rather than exhausting memory. Sixteen million nodes is far above any real
/// function --- the largest decompiled functions in the fixture corpus are four
/// figures --- so the budget is unreachable by legitimate input and reachable
/// by a generator gone wrong.
pub const MAX_NODES: usize = 1 << 24;

/// Which of the three loop shapes a [`Flow::LoopHeader`] opens.
///
/// The distinction is structural, not syntactic: it decides where the test node
/// sits relative to the body (before it, or after it as a latch) and where
/// `continue` lands. A language whose loops are all one of these three needs no
/// new variant, and `REQ-SYN-10` says not to invent a fourth until a second
/// front end actually needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LoopKind {
    /// The test runs before each iteration, so the body may run zero times.
    While,
    /// The test runs after each iteration, so the body always runs once.
    DoWhile,
    /// The test runs before each iteration and a separate per-iteration step
    /// region, delimited by [`Flow::LoopStep`], runs after the body. `continue`
    /// lands on the step, not on the test.
    For,
}

/// One control-flow event, the builder's entire input vocabulary.
///
/// # The scoping sub-vocabulary, and why it is these four
///
/// The sketch in section 5 of the design doc lists the *transfers*; a builder
/// also has to be told where constructs begin and end, because a `break` is
/// meaningless without knowing which construct encloses it. Four events do
/// that, chosen to be the smallest set that keeps every malformed stream
/// diagnosable:
///
/// * [`Flow::Then`] and [`Flow::Else`] separate the two arms of a branch. They
///   are two events rather than one generic "next alternative" so that an
///   `Else` with no `Then` is a diagnostic instead of a silently different
///   graph.
/// * [`Flow::Case`] starts the next arm of a switch. It is separate from
///   `Then`/`Else` for the same reason, and it carries the arm's own span
///   because an arm label is a real jump target a `goto` may name.
/// * [`Flow::LoopStep`] starts a loop's per-iteration step region. Without it a
///   `for` loop's `continue` cannot reach the step, which violates
///   `REQ-GEN-1`'s "`break` and `continue` target the enclosing construct".
/// * [`Flow::EndScope`] closes whichever construct is innermost. One event
///   closes all three because the builder already knows which is open, and a
///   typed close event could only ever disagree with it.
///
/// # Why several variants carry a span the sketch omitted
///
/// `REQ-SYN-7` makes spans total: every CFG node carries one, including nodes
/// recovered from an error. Every variant below except the four scoping events
/// creates exactly one node, so every one of them carries the span of the text
/// it came from. That is the only extension to the sketch's vocabulary.
///
/// # Labelling a construct
///
/// A [`Flow::Label`] immediately followed by a construct-opening event names
/// that construct, which is how `foo: while (...)` and `'a: loop` both reach a
/// labelled `break`. The label remains an ordinary `goto` target as well.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flow {
    /// A straight-line item: one node, one successor.
    Stmt(Span),
    /// A two-way test. The emitter orders the successors by emitting
    /// [`Flow::Then`] for the first and [`Flow::Else`] for the second.
    Branch {
        /// Span of the condition text.
        cond: Span,
    },
    /// Begins the first arm of the innermost open branch.
    Then,
    /// Begins the second arm of the innermost open branch, closing the first.
    Else,
    /// Opens a loop, creating its test node.
    LoopHeader {
        /// Where the test sits relative to the body.
        kind: LoopKind,
        /// Span of the loop's controlling expression.
        cond: Span,
    },
    /// Begins the per-iteration step region of the innermost open loop, which
    /// becomes that loop's `continue` target.
    LoopStep,
    /// Opens a switch, creating its dispatch node.
    Switch {
        /// How many [`Flow::Case`] events the emitter intends to follow with.
        /// The builder only compares it against the number that arrive, so a
        /// mismatch is caught as a diagnostic rather than as a wrong graph.
        arms: u32,
        /// Span of the switched-over expression.
        subject: Span,
    },
    /// Begins the next arm of the innermost open switch. Control reaching this
    /// event from the previous arm becomes a fall-through edge.
    Case {
        /// Span of the arm label.
        span: Span,
        /// Whether this arm is the one every unmatched value dispatches to.
        default: bool,
    },
    /// Closes the innermost open branch, loop or switch.
    EndScope,
    /// Leaves the enclosing loop or switch, or the one carrying `label`.
    Break {
        /// The named construct to leave, or `None` for the innermost.
        label: Option<Symbol>,
        /// Span of the transfer.
        span: Span,
    },
    /// Starts the next iteration of the enclosing loop, or of the one carrying
    /// `label`.
    Continue {
        /// The named loop to continue, or `None` for the innermost.
        label: Option<Symbol>,
        /// Span of the transfer.
        span: Span,
    },
    /// Transfers to a [`Flow::Label`], which may appear later in the stream and
    /// is then resolved by backpatching.
    Goto {
        /// The label to transfer to.
        label: Symbol,
        /// Span of the transfer.
        span: Span,
    },
    /// Transfers through a computed address to any statically known label.
    ///
    /// The target set belongs to this event so an indirect transfer is atomic:
    /// front ends do not have to encode one machine operation as a synthetic
    /// chain of tests and direct gotos.
    IndirectDispatch {
        /// Labels whose addresses are taken in the containing function, in a
        /// deterministic source order.
        targets: Vec<Symbol>,
        /// Whether `targets` is the proven valid destination set or a sound
        /// over-approximation.
        precision: TargetPrecision,
        /// Whether evaluation may fail to produce a valid destination.
        may_be_invalid: bool,
        /// Stable reasons why this transfer could not be resolved exactly.
        reasons: Vec<DispatchUncertainty>,
        /// Span of the complete indirect transfer.
        span: Span,
    },
    /// A named landing pad a [`Flow::Goto`] may target, and the name of a
    /// construct if one opens immediately after it.
    Label {
        /// The name being defined.
        name: Symbol,
        /// Span of the label.
        span: Span,
    },
    /// Leaves the function, flowing to the function-end node.
    Return(Span),
    /// A construct with no successor at all --- a call that never returns, a
    /// trap, an infinite loop the emitter knows cannot exit. A path that ends
    /// here satisfies `REQ-GEN-1` without reaching the function end.
    Diverge(Span),
}

/// Precision of an indirect control-transfer target set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetPrecision {
    /// Every valid destination, and no other destination, is present.
    Exact,
    /// The target set was widened to avoid omitting a valid destination.
    Conservative,
}

impl TargetPrecision {
    /// Stable serialized spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Conservative => "conservative",
        }
    }
}

/// Why an indirect control-transfer target set was widened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DispatchUncertainty {
    /// The dispatch expression has not been resolved to a finite value set.
    UnresolvedValue,
    /// A load may read storage outside the supported memory model.
    UnknownMemory,
    /// Storage containing a label address may be modified after escaping.
    EscapedLabelAddress,
    /// Pointer arithmetic could not be related to a known label table.
    UnsupportedPointerArithmetic,
    /// A table index could not be proven in bounds.
    IndexMayBeOutOfBounds,
    /// Parser recovery obscured part of the dispatch expression.
    RecoveredSyntax,
    /// A configured analysis budget was exhausted.
    BudgetExceeded,
    /// Flow-insensitive assignments may add values unavailable at this point.
    FlowInsensitiveJoin,
}

impl DispatchUncertainty {
    /// Stable serialized spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::UnresolvedValue => "unresolved_value",
            Self::UnknownMemory => "unknown_memory",
            Self::EscapedLabelAddress => "escaped_label_address",
            Self::UnsupportedPointerArithmetic => "unsupported_pointer_arithmetic",
            Self::IndexMayBeOutOfBounds => "index_may_be_out_of_bounds",
            Self::RecoveredSyntax => "recovered_syntax",
            Self::BudgetExceeded => "budget_exceeded",
            Self::FlowInsensitiveJoin => "flow_insensitive_join",
        }
    }
}

/// Sparse qualifications for one [`NodeKind::IndirectDispatch`] node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndirectDispatchInfo {
    /// The dispatch node whose successors are its current target set.
    pub node: NodeId,
    /// Exactness of those successor edges.
    pub precision: TargetPrecision,
    /// Whether the source expression may produce no valid destination.
    pub may_be_invalid: bool,
    /// Sorted, deduplicated explanations for conservative resolution.
    pub reasons: Vec<DispatchUncertainty>,
}

/// What kind of source construct a [`CfgNode`] came from.
///
/// The kind lets a consumer read the graph without re-deriving structure from
/// degrees. That degree-based derivation is a parity-layer concern and is kept
/// out of the general graph on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeKind {
    /// The unique function entry. Never has a predecessor.
    Entry,
    /// The unique function end. Never has a successor.
    Exit,
    /// A straight-line item.
    Stmt,
    /// A two-way test that is not a loop header.
    Cond,
    /// A loop's test node, the destination of the loop's back edges.
    LoopHeader,
    /// A switch's dispatch node.
    Switch,
    /// A computed multiway transfer to statically known labels.
    IndirectDispatch,
    /// One switch arm's label.
    Case,
    /// A `goto` landing pad.
    Label,
    /// A `goto` transfer.
    Goto,
    /// A `break` transfer.
    Break,
    /// A `continue` transfer.
    Continue,
    /// A `return` transfer.
    Return,
    /// A construct with no successor, including a transfer the builder could
    /// not resolve.
    Diverge,
}

impl NodeKind {
    /// Whether this kind is a structural anchor that coalescing must not
    /// absorb into a chain.
    ///
    /// The entry and the function end are what `REQ-GEN-1`'s first two
    /// invariants are stated in terms of. Contracting them into a neighbouring
    /// statement would make both anchors positional rather than structural, and
    /// deciding whether the function-end node survives is precisely the parity
    /// layer's business, not this one's.
    pub fn is_anchor(self) -> bool {
        matches!(self, NodeKind::Entry | NodeKind::Exit)
    }

    /// This kind's stable name, for a census a caller reads or serializes.
    ///
    /// Stable is the operative word: these strings are keys in
    /// [`crate::syntax::metrics::GraphMetrics::node_kinds`], which reaches
    /// Python and JSON, so renaming one is a breaking change to a consumer
    /// rather than a cosmetic edit.
    pub const fn name(self) -> &'static str {
        match self {
            NodeKind::Entry => "entry",
            NodeKind::Exit => "exit",
            NodeKind::Stmt => "stmt",
            NodeKind::Cond => "cond",
            NodeKind::LoopHeader => "loop_header",
            NodeKind::Switch => "switch",
            NodeKind::IndirectDispatch => "indirect_dispatch",
            NodeKind::Case => "case",
            NodeKind::Label => "label",
            NodeKind::Goto => "goto",
            NodeKind::Break => "break",
            NodeKind::Continue => "continue",
            NodeKind::Return => "return",
            NodeKind::Diverge => "diverge",
        }
    }
}

/// Why control moves along a [`CfgEdge`].
///
/// The kind records the *role* the edge plays at its source; whether it also
/// closes a loop is the separate [`CfgEdge::is_back`] flag, because a false arm
/// that happens to be the last thing in a loop body is both at once and
/// collapsing the two would lose one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeKind {
    /// Ordinary sequential succession.
    Fall,
    /// The first successor of a two-way test.
    True,
    /// The second successor of a two-way test.
    False,
    /// A switch dispatching into one of its arms.
    Case,
    /// A switch dispatching into its default arm, or --- when it has no default
    /// arm --- past the whole construct.
    Default,
    /// One switch arm running into the next because it did not break.
    FallThrough,
    /// An explicit transfer: `goto`, `break`, `continue`, `return`.
    Jump,
}

impl EdgeKind {
    /// This kind's stable name, under the same contract as
    /// [`NodeKind::name`]: it is a serialized census key, not a display label.
    pub const fn name(self) -> &'static str {
        match self {
            EdgeKind::Fall => "fall",
            EdgeKind::True => "true",
            EdgeKind::False => "false",
            EdgeKind::Case => "case",
            EdgeKind::Default => "default",
            EdgeKind::FallThrough => "fall_through",
            EdgeKind::Jump => "jump",
        }
    }
}

/// One node of a [`super::Cfg`], carrying every span that was coalesced into it.
///
/// The span list is a list rather than a single covering span because a
/// coalesced chain's members are not contiguous in the source --- a loop body's
/// last statement and the step that follows it are far apart --- so a covering
/// span would claim text that is not in the block. [`CfgNode::span`] computes
/// the cover when a caller genuinely wants one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CfgNode {
    kind: NodeKind,
    spans: Vec<Span>,
}

impl CfgNode {
    /// A node of `kind` covering `spans`, in source order.
    ///
    /// An empty span list is accepted and yields an empty span from
    /// [`CfgNode::span`]; nothing here panics on it (`REQ-SYN-2`).
    pub fn new(kind: NodeKind, spans: Vec<Span>) -> Self {
        Self { kind, spans }
    }

    /// A node of `kind` covering exactly one span.
    pub fn single(kind: NodeKind, span: Span) -> Self {
        Self {
            kind,
            spans: vec![span],
        }
    }

    /// What source construct this node came from.
    pub fn kind(&self) -> NodeKind {
        self.kind
    }

    /// Every span coalesced into this node, in the order control passes them.
    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// The smallest span covering all of [`CfgNode::spans`].
    pub fn span(&self) -> Span {
        let mut iter = self.spans.iter().copied();
        match iter.next() {
            Some(first) => iter.fold(first, |acc, s| acc.to(s)),
            None => Span::default(),
        }
    }
}

/// One directed edge of a [`super::Cfg`].
///
/// Fields are public because an edge is a plain record with no invariant of its
/// own to protect, and because the parity layer assembles its own edge list to
/// hand to [`super::Cfg::from_parts`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CfgEdge {
    /// Where control leaves.
    pub src: NodeId,
    /// Where control arrives.
    pub dst: NodeId,
    /// Why control moves.
    pub kind: EdgeKind,
    /// Whether this edge returns to the head of an enclosing loop, which is
    /// what `REQ-GEN-1`'s "back edges correspond to source loops" is about.
    pub is_back: bool,
}

impl CfgEdge {
    /// An edge from `src` to `dst` that does not close a loop.
    pub fn new(src: NodeId, dst: NodeId, kind: EdgeKind) -> Self {
        Self {
            src,
            dst,
            kind,
            is_back: false,
        }
    }

    /// An edge from `src` to `dst` that returns to an enclosing loop's head.
    pub fn back(src: NodeId, dst: NodeId, kind: EdgeKind) -> Self {
        Self {
            src,
            dst,
            kind,
            is_back: true,
        }
    }
}
