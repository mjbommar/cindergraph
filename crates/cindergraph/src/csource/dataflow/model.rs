//! The vocabulary the analysis produces: bindings, definitions, uses, edges.
//!
//! Split from the analysis itself so a consumer can name a [`Definition`] or
//! match on a [`DefKind`] without pulling in the syntax walk or the fixpoint,
//! and so the two halves have one reason to change apiece.

use crate::csource::semantic::{FunctionId, SourceUnitId};
use crate::syntax::ids::Span;

/// Semantic capability affected by an analysis issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CoverageDimension {
    /// Parsing, recovery, declaration boundaries, or lexical identity.
    Syntax,
    /// Runtime reads, writes, calls, and sequencing.
    Effects,
    /// Pointer targets and reads or writes through storage.
    Memory,
    /// Type-driven runtime values such as captured VLA bounds.
    TypeValue,
    /// Destinations of indirect control transfers.
    ControlTargets,
}

impl CoverageDimension {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Syntax => "syntax",
            Self::Effects => "effects",
            Self::Memory => "memory",
            Self::TypeValue => "type_value",
            Self::ControlTargets => "control_targets",
        }
    }
}

/// Why an analysis result cannot certify absence in one or more dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemanticIssueKind {
    /// The parser or CFG builder recovered from at least one diagnostic.
    RecoveredSyntax,
    /// A low-level function analysis was called without TU diagnostics.
    RecoveryContextUnavailable,
    /// An evaluated construct could not be lowered into explicit events.
    UnmodeledEffect,
    /// Conflicting scalar accesses have no language-defined relative order.
    UnsequencedAccess,
    /// A memory access exceeded the supported local points-to model.
    UnknownMemoryEffect,
    /// A type-driven runtime value dependency could not be represented.
    UnmodeledTypeValue,
    /// Declaration type resolution could not establish a structural type.
    UnknownType,
    /// An indirect control transfer required a widened target set.
    UnresolvedControlTarget,
}

impl SemanticIssueKind {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::RecoveredSyntax => "recovered_syntax",
            Self::RecoveryContextUnavailable => "recovery_context_unavailable",
            Self::UnmodeledEffect => "unmodeled_effect",
            Self::UnsequencedAccess => "unsequenced_access",
            Self::UnknownMemoryEffect => "unknown_memory_effect",
            Self::UnmodeledTypeValue => "unmodeled_type_value",
            Self::UnknownType => "unknown_type",
            Self::UnresolvedControlTarget => "unresolved_control_target",
        }
    }

    /// Capabilities whose negative answers this issue qualifies.
    pub const fn dimensions(self) -> &'static [CoverageDimension] {
        match self {
            Self::RecoveredSyntax | Self::RecoveryContextUnavailable => &[
                CoverageDimension::Syntax,
                CoverageDimension::Effects,
                CoverageDimension::Memory,
                CoverageDimension::TypeValue,
                CoverageDimension::ControlTargets,
            ],
            Self::UnmodeledEffect | Self::UnsequencedAccess => &[CoverageDimension::Effects],
            Self::UnknownMemoryEffect => &[CoverageDimension::Memory],
            Self::UnmodeledTypeValue => &[CoverageDimension::TypeValue],
            Self::UnknownType => &[
                CoverageDimension::Effects,
                CoverageDimension::Memory,
                CoverageDimension::TypeValue,
            ],
            Self::UnresolvedControlTarget => &[CoverageDimension::ControlTargets],
        }
    }
}

/// One structured qualification on a function analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticIssue {
    /// Machine-readable reason.
    pub kind: SemanticIssueKind,
    /// Best known affected source range; `None` currently means function-wide.
    pub span: Option<Span>,
}

/// Which variable an event is about: an index into the function's binding
/// table, or [`Binding::FREE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Binding(pub u32);

impl Binding {
    /// Temporary unresolved marker during event collection, or a call argument
    /// that is not a bare name. Final definition/use records have dense IDs;
    /// unresolved spellings are interned separately after lexical resolution.
    pub const FREE: Binding = Binding(u32::MAX);

    /// Whether this is the temporary unresolved/absent marker.
    pub fn is_free(self) -> bool {
        self == Binding::FREE
    }
}

/// Dense identity of one abstract storage region within a function.
///
/// Region IDs are graph-local, just like [`Binding`]. A region is rooted in a
/// resolved binding and may then select fields or the summary of all elements
/// of an array. The element summary is intentionally conservative until index
/// values and object extents have a sound disjointness model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MemoryRegionId(pub u32);

/// Structural identity of an abstract memory region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryRegionKind {
    /// The complete storage owned by a resolved declaration.
    Binding { binding: Binding },
    /// Storage reached through one formal pointer's incoming value.
    ///
    /// This is an abstract caller-owned root, not the local object holding the
    /// pointer itself. `parameter` is the formal position and `binding` keeps
    /// the graph-local declaration identity used for names and diagnostics.
    ParameterPointee { parameter: u32, binding: Binding },
    /// One named member below another region.
    Field {
        base: MemoryRegionId,
        member: String,
        /// Members with the same base overlap because that base is a union.
        overlapping_members: bool,
    },
    /// A may-alias summary of every element below another region.
    Elements { base: MemoryRegionId },
}

impl MemoryRegionKind {
    /// Stable serialized name.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Binding { .. } => "binding",
            Self::ParameterPointee { .. } => "parameter_pointee",
            Self::Field { .. } => "field",
            Self::Elements { .. } => "elements",
        }
    }
}

/// One interned abstract storage region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegion {
    pub id: MemoryRegionId,
    pub kind: MemoryRegionKind,
}

/// Why two distinct abstract regions can name overlapping storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryOverlapKind {
    /// `left` contains `right` through field/element projection.
    Containment,
    /// Distinct members of one union share storage.
    UnionMembers,
    /// Distinct formal pointers may designate the same caller-owned storage.
    ParameterAlias,
}

impl MemoryOverlapKind {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Containment => "containment",
            Self::UnionMembers => "union_members",
            Self::ParameterAlias => "parameter_alias",
        }
    }
}

/// One explicit overlap relationship between distinct regions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryRegionOverlap {
    pub left: MemoryRegionId,
    pub right: MemoryRegionId,
    pub kind: MemoryOverlapKind,
}

/// Direction of one operation-owned memory access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryAccessKind {
    Read,
    Write,
}

impl MemoryAccessKind {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
        }
    }
}

/// Precision of an access-to-region association.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemoryAccessPrecision {
    /// The operation directly names this unique region.
    Exact,
    /// This region may be accessed; alternatives or summarized elements exist.
    MayAlias,
}

impl MemoryAccessPrecision {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::MayAlias => "may_alias",
        }
    }
}

/// One read or write associated with an abstract memory region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryAccess {
    pub region: MemoryRegionId,
    pub kind: MemoryAccessKind,
    pub precision: MemoryAccessPrecision,
    pub node: u32,
    pub span: Span,
    /// Point at which a write becomes visible. Equal to `span.lo` for reads.
    pub effect_at: u32,
}

/// A write participating in the abstract-memory reaching-definition lattice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDefinition {
    pub region: MemoryRegionId,
    pub kind: MemoryDefinitionKind,
    pub precision: MemoryAccessPrecision,
    pub node: u32,
    pub span: Span,
    pub effect_at: u32,
}

/// Origin of an abstract-memory definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryDefinitionKind {
    /// State supplied at function entry by a by-value aggregate parameter.
    IncomingParameter,
    /// An operation-owned store in the function body.
    Store,
    /// A call that may modify storage reachable through a pointer argument.
    CallClobber,
    /// A write instantiated from a complete known callee effect summary.
    CallEffect,
}

impl MemoryDefinitionKind {
    /// Stable serialized name.
    pub const fn name(self) -> &'static str {
        match self {
            Self::IncomingParameter => "incoming_parameter",
            Self::Store => "store",
            Self::CallClobber => "call_clobber",
            Self::CallEffect => "call_effect",
        }
    }
}

/// A read participating in the abstract-memory reaching-definition lattice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryUse {
    pub region: MemoryRegionId,
    pub precision: MemoryAccessPrecision,
    pub node: u32,
    pub span: Span,
}

/// One possible reaching-memory-definition relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryFlowEdge {
    /// Index into [`DataFlow::memory_definitions`].
    pub definition: u32,
    /// Index into [`DataFlow::memory_uses`].
    pub use_: u32,
    pub definition_region: MemoryRegionId,
    pub use_region: MemoryRegionId,
    /// `None` when both endpoints name the same region.
    pub overlap: Option<MemoryOverlapKind>,
}

/// A C type as the source spells it.
///
/// **As written, not resolved.** This front end reads one translation unit and
/// does not process `#include` (`REQ-GEN`), so a typedef from a header is an
/// opaque name and is recorded as one: `uint32_t` is stored as `uint32_t`, and
/// nothing here claims to know it is four bytes. Storing the spelling is
/// useful; claiming a width we cannot derive would not be.
///
/// Deliberately **not** built on `crate::metrics::type_name::normalize_type`.
/// That function reproduces four defects in DecBench's reference
/// implementation on purpose --- it emits the non-C spelling `long long long`,
/// and turns `_Bool` into `_bool` --- because parity with the benchmark is its
/// contract. A general consumer wants the type the programmer wrote, so this
/// reads the specifier text directly and leaves that module to its own job.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CType {
    /// The declaration specifiers, whitespace-collapsed: `unsigned long`,
    /// `struct point`, `const char`. Empty when the declaration had none the
    /// parser recovered.
    pub specifiers: String,
    /// How many `*` sit between the specifiers and the name. `char *argv` is
    /// 1; `char **argv` is 2.
    pub pointer_depth: u32,
    /// How many `[...]` suffixes follow the name. `int m[4][4]` is 2.
    pub array_rank: u32,
    /// `const` appears in the specifiers.
    pub is_const: bool,
    /// `volatile` appears in the specifiers. Load-bearing for a reader: a
    /// `volatile` read cannot be elided, so a dead store to one is not dead.
    pub is_volatile: bool,
    /// `static` appears in the specifiers.
    pub is_static: bool,
    /// `extern` appears in the specifiers.
    pub is_extern: bool,
}

impl CType {
    /// Whether anything was recovered at all.
    pub fn is_empty(&self) -> bool {
        self.specifiers.is_empty() && self.pointer_depth == 0 && self.array_rank == 0
    }

    /// The type as one string: specifiers, then stars, then array brackets.
    ///
    /// A display form, not a canonical one --- two spellings of the same type
    /// render differently, which is why comparison goes through
    /// [`CType::same_shape`] rather than through this.
    pub fn render(&self) -> String {
        let mut out = self.specifiers.clone();
        if self.pointer_depth > 0 {
            if !out.is_empty() {
                out.push(' ');
            }
            for _ in 0..self.pointer_depth {
                out.push('*');
            }
        }
        for _ in 0..self.array_rank {
            out.push_str("[]");
        }
        out
    }

    /// Whether two types agree on everything a dataflow consumer can check.
    ///
    /// Compares the specifier text with qualifiers stripped, plus the pointer
    /// depth and array rank. It is a *shape* test, not a type-equality test:
    /// without `#include` resolution, `uint32_t` and `unsigned int` are two
    /// opaque names and this reports them as different, which is the honest
    /// answer rather than a guess.
    pub fn same_shape(&self, other: &CType) -> bool {
        self.pointer_depth == other.pointer_depth
            && self.array_rank == other.array_rank
            && strip_qualifiers(&self.specifiers) == strip_qualifiers(&other.specifiers)
    }
}

/// The specifier text with storage-class and qualifier words removed, so
/// `const char` and `char` compare equal.
fn strip_qualifiers(specifiers: &str) -> String {
    specifiers
        .split_whitespace()
        .filter(|word| {
            !matches!(
                *word,
                "const"
                    | "volatile"
                    | "restrict"
                    | "__restrict"
                    | "__restrict__"
                    | "static"
                    | "extern"
                    | "register"
                    | "auto"
                    | "inline"
                    | "_Atomic"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One call in a function body, as the syntax walk saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallRecord {
    /// The callee's name, or `None` when the call was indirect --- through a
    /// pointer or a struct member, which names no function.
    pub callee: Option<String>,
    /// The binding at each argument position, in order. A position holding an
    /// expression rather than a bare name is [`Binding::FREE`], which no real
    /// binding equals, so it neither propagates a value nor blocks one.
    pub arguments: Vec<Binding>,
    /// Source extent of each argument expression, including nested calls.
    pub argument_spans: Vec<Span>,
    /// Whether the call's result is returned directly: `return g(x);`.
    pub result_is_returned: bool,
    /// Where the call sits, for a label.
    pub span: Span,
}

/// Pointer identity retained for one actual argument at one call site.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallMemoryArgument {
    pub call_span: Span,
    pub node: u32,
    pub argument: u32,
    /// Concrete local objects this actual argument may designate.
    pub targets: Vec<Binding>,
    /// Formal pointee roots in this caller from which the actual may derive.
    pub parameter_origins: Vec<u32>,
    /// Whether the two may sets above cover every supported source alternative.
    pub complete: bool,
}

/// One write of a variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    /// The variable written.
    pub binding: Binding,
    /// The variable's spelling, for a label.
    pub name: String,
    /// The CFG node the write happens on.
    pub node: u32,
    /// The source the write covers: the target's own name.
    pub span: Span,
    /// The offset at which the write becomes visible to a later read.
    ///
    /// **Not** the end of [`Definition::span`]. C evaluates the right-hand
    /// side first, so in `sum = sum + i` the read of `sum` sees whatever
    /// reached the statement, not the value this statement is about to store.
    /// Ordering same-node events by the target's position instead gets that
    /// backwards and silently drops the loop-carried dependence.
    pub effect_at: u32,
    /// How the write was spelled.
    pub kind: DefKind,
    /// The type declared at *this* site, when the write is a declaration or a
    /// parameter. `None` for an assignment, which declares nothing.
    ///
    /// Separate from [`DataFlow::types`] on purpose: that is the binding's
    /// type, and this is what one site said. In well-typed C they agree; in a
    /// decompiler's output they need not, which is what
    /// [`DataFlow::type_conflicts`] looks for.
    pub declared: Option<CType>,
}

/// One read of a variable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    /// The variable read.
    pub binding: Binding,
    /// The variable's spelling, for a label.
    pub name: String,
    /// The CFG node the read happens on.
    pub node: u32,
    /// The source the read covers.
    pub span: Span,
}

/// The kind of value definition or address-taking event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DefKind {
    /// A parameter, defined at the function entry.
    Parameter,
    /// A declarator, with or without an initializer.
    Declaration,
    /// The left side of `=`.
    Assignment,
    /// The left side of a compound assignment, which also reads.
    CompoundAssignment,
    /// The operand of `++` or `--`, which also reads.
    IncDec,
    /// The operand of `&`. Recorded in the definition-shaped event table so
    /// the points-to pass can recover targets and consumers can see escapes.
    /// It is not inserted into the reaching-definition lattice because `&x`
    /// does not write the value of `x`.
    AddressTaken,
    /// A write through a pointer to a possible local target. A weak update:
    /// it adds a reaching definition without killing alternative writes.
    MemoryWrite,
}

impl DefKind {
    /// This kind's stable name, for a serialized attribute.
    pub const fn name(self) -> &'static str {
        match self {
            DefKind::Parameter => "parameter",
            DefKind::Declaration => "declaration",
            DefKind::Assignment => "assignment",
            DefKind::CompoundAssignment => "compound_assignment",
            DefKind::IncDec => "inc_dec",
            DefKind::AddressTaken => "address_taken",
            DefKind::MemoryWrite => "memory_write",
        }
    }
}

/// One data-dependence edge: a definition a use can see.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowEdge {
    /// Index into [`DataFlow::definitions`].
    pub def: u32,
    /// Index into [`DataFlow::uses`].
    pub use_: u32,
    /// The variable, so an edge is readable without dereferencing either end.
    pub name: String,
}

/// One function's reaching-definition analysis.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DataFlow {
    /// Exact source snapshot that owns every span and graph-local ID here.
    pub source_id: SourceUnitId,
    /// Dense function identity within `source_id`.
    pub function_id: FunctionId,
    /// Semantic result contract revision.
    pub analysis_revision: u32,
    /// Source extent of each CFG node, indexed by its graph-local ID.
    /// Used to evaluate a controller's value in its own expression context.
    pub node_spans: Vec<Span>,
    /// `(comma expression, discarded operand)` spans. The discarded operand
    /// still executes, but its value does not contribute to the enclosing
    /// comma expression's value.
    pub discarded_values: Vec<(Span, Span)>,
    /// The translation unit produced no parser or CFG diagnostics.
    /// False also means diagnostic context was not supplied to the low-level
    /// function analyzer. This is not a certificate of valid or sound C.
    pub recovery_free: bool,
    /// Whether reads, writes and calls were fully lowered into events.
    /// False for opaque selection/variadic builtins and inline assembly whose
    /// evaluated operands or clobbers this syntax-level analysis cannot model.
    pub effects_complete: bool,
    /// Whether accesses stayed within the supported local-pointer model.
    pub memory_complete: bool,
    /// Whether variable-length-array value dependencies are fully represented.
    /// False currently identifies a VLA typedef whose captured bound cannot be
    /// propagated through a later use of the typedef name.
    pub vla_complete: bool,
    /// Whether every indirect control transfer has an exact destination set.
    pub control_targets_complete: bool,
    /// Structured source of truth for coverage qualifications.
    ///
    /// The four compatibility booleans above are derived from this ledger at
    /// the analysis boundary. Early migration stages use function-wide issues;
    /// later semantic lowering can attach precise spans.
    pub semantic_issues: Vec<SemanticIssue>,
    /// Dense binding IDs whose declarations were not recovered in this function.
    /// Equal unresolved spellings share an ID; different spellings do not.
    pub unresolved_bindings: Vec<Binding>,
    /// CFG nodes whose actual kind is return.
    pub return_nodes: Vec<u32>,
    /// Source expressions belonging to return statements, before CFG splitting.
    pub return_spans: Vec<Span>,
    /// Controller-to-controlled CFG node pairs.
    pub control_edges: Vec<(u32, u32)>,
    /// The function's declared name.
    pub name: String,
    /// Every value definition, memory write and address-taking event, in source
    /// order. `AddressTaken` entries describe escapes but are not value writes.
    pub definitions: Vec<Definition>,
    /// Every read, in source order.
    pub uses: Vec<Use>,
    /// Every (definition, use) pair the control-flow graph allows.
    pub edges: Vec<FlowEdge>,
    /// Uses no definition reaches: a global, a parameter of an unrecovered
    /// declarator, or a genuine read-before-write.
    ///
    /// This is the headline defect number the analysis produces, and it is the
    /// same question `defuse_baseline.json` asks of the decompiler's output
    /// from the other side.
    pub unresolved_uses: Vec<u32>,
    /// Every call this function makes, in source order.
    ///
    /// Calls owned by the evaluation plan retain ordered argument positions;
    /// the syntax walk supplies records only for roots that lowering declined.
    /// Interprocedural summaries consume this common representation.
    pub calls: Vec<CallRecord>,
    /// Pointer-aware actual/formal evidence for interprocedural memory effects.
    pub call_memory_arguments: Vec<CallMemoryArgument>,
    /// Interned abstract storage regions referenced by projected accesses.
    pub memory_regions: Vec<MemoryRegion>,
    /// Explicit containment and union-member overlap relationships.
    pub memory_overlaps: Vec<MemoryRegionOverlap>,
    /// Operation-owned reads and writes of abstract storage regions.
    ///
    /// These records expose identity independently of scalar bindings. Their
    /// reaching relationships are in [`DataFlow::memory_edges`]; consult
    /// `memory_complete` before treating an absent access as impossible.
    pub memory_accesses: Vec<MemoryAccess>,
    /// Writes in the region reaching-definition lattice.
    pub memory_definitions: Vec<MemoryDefinition>,
    /// Reads in the region reaching-definition lattice.
    pub memory_uses: Vec<MemoryUse>,
    /// Possible definition-to-use flows over abstract memory regions.
    pub memory_edges: Vec<MemoryFlowEdge>,
    /// The spelling of each binding, in binding order.
    ///
    /// A binding that is declared and never mentioned again appears in neither
    /// [`DataFlow::definitions`] nor [`DataFlow::uses`] --- `int *b;` writes
    /// nothing and reads nothing --- so without this there is no way to ask
    /// about it at all. It is also what makes an unused local reportable.
    pub names: Vec<String>,
    /// The declared type of each binding, in binding order.
    ///
    /// Indexed by [`Binding`]'s inner value, so `types[b.0 as usize]` is the
    /// type of binding `b`. Unresolved bindings have an empty type entry.
    pub types: Vec<CType>,
    /// Definitions no use reads: a dead store.
    ///
    /// The other direction of the same relation, and the one the reaching
    /// analysis gets for free. A decompiler that invents a temporary, assigns
    /// it and never reads it produces one of these per invention, so the count
    /// is a readability measure the execution differential cannot see --- goto
    /// soup and dead stores both pass a test that only checks the return
    /// value.
    ///
    /// A parameter that is never read is deliberately **not** counted: the
    /// caller wrote it, the signature is the contract, and an unused parameter
    /// is a style question rather than a recovered-code defect.
    pub dead_stores: Vec<u32>,
}

impl DataFlow {
    /// Record one semantic qualification in deterministic discovery order.
    pub(crate) fn record_issue(&mut self, kind: SemanticIssueKind, span: Option<Span>) {
        let issue = SemanticIssue { kind, span };
        if !self.semantic_issues.contains(&issue) {
            self.semantic_issues.push(issue);
        }
    }

    /// Whether no recorded issue affects `dimension`.
    pub fn covers(&self, dimension: CoverageDimension) -> bool {
        !self
            .semantic_issues
            .iter()
            .any(|issue| issue.kind.dimensions().contains(&dimension))
    }

    /// Bridge legacy pass flags into the structured P1 coverage ledger.
    ///
    /// Passes will migrate to recording precise issues directly. Until then,
    /// this single boundary prevents public booleans and issue records from
    /// disagreeing.
    pub(crate) fn refresh_semantic_issues(&mut self, recovery_issue: Option<SemanticIssueKind>) {
        self.semantic_issues.retain(|issue| {
            !matches!(
                issue.kind,
                SemanticIssueKind::RecoveredSyntax | SemanticIssueKind::RecoveryContextUnavailable
            )
        });
        if let Some(kind) = recovery_issue {
            self.record_issue(kind, None);
        }
        if !self.effects_complete
            && !self
                .semantic_issues
                .iter()
                .any(|issue| issue.kind == SemanticIssueKind::UnmodeledEffect)
        {
            self.record_issue(SemanticIssueKind::UnmodeledEffect, None);
        }
        if !self.memory_complete
            && !self
                .semantic_issues
                .iter()
                .any(|issue| issue.kind == SemanticIssueKind::UnknownMemoryEffect)
        {
            self.record_issue(SemanticIssueKind::UnknownMemoryEffect, None);
        }
        if !self.vla_complete
            && !self
                .semantic_issues
                .iter()
                .any(|issue| issue.kind == SemanticIssueKind::UnmodeledTypeValue)
        {
            self.record_issue(SemanticIssueKind::UnmodeledTypeValue, None);
        }
        self.sync_compatibility_flags();
    }

    /// Replace recovery qualifications with their known diagnostic origins.
    /// Their dimensions remain function-wide until semantic resolution can
    /// prove a narrower affected region.
    pub(crate) fn replace_recovery_issues(
        &mut self,
        kind: Option<SemanticIssueKind>,
        spans: &[Span],
    ) {
        self.semantic_issues.retain(|issue| {
            !matches!(
                issue.kind,
                SemanticIssueKind::RecoveredSyntax | SemanticIssueKind::RecoveryContextUnavailable
            )
        });
        if let Some(kind) = kind {
            if spans.is_empty() {
                self.record_issue(kind, None);
            } else {
                for span in spans {
                    self.record_issue(kind, Some(*span));
                }
            }
        }
        self.sync_compatibility_flags();
    }

    pub(crate) fn sync_compatibility_flags(&mut self) {
        self.recovery_free = !self.semantic_issues.iter().any(|issue| {
            matches!(
                issue.kind,
                SemanticIssueKind::RecoveredSyntax | SemanticIssueKind::RecoveryContextUnavailable
            )
        });
        self.effects_complete = self.covers(CoverageDimension::Effects);
        self.memory_complete = self.covers(CoverageDimension::Memory);
        self.vla_complete = self.covers(CoverageDimension::TypeValue);
        self.control_targets_complete = self.covers(CoverageDimension::ControlTargets);
    }

    /// Definitions that reach `use_index`.
    pub fn definitions_reaching(&self, use_index: u32) -> impl Iterator<Item = &Definition> {
        self.edges
            .iter()
            .filter(move |edge| edge.use_ == use_index)
            .filter_map(|edge| self.definitions.get(edge.def as usize))
    }

    /// Human-readable structural path for an abstract memory region.
    ///
    /// Returns `None` for a malformed/cyclic table or an unknown root binding.
    pub fn memory_region_name(&self, id: MemoryRegionId) -> Option<String> {
        let mut components: Vec<Option<&str>> = Vec::new();
        let mut current = id;
        let mut visited = std::collections::BTreeSet::new();
        loop {
            if !visited.insert(current) {
                return None;
            }
            match &self.memory_regions.get(current.0 as usize)?.kind {
                MemoryRegionKind::Binding { binding } => {
                    let mut name = self.names.get(binding.0 as usize)?.clone();
                    for component in components.iter().rev() {
                        match component {
                            Some(member) => {
                                name.push('.');
                                name.push_str(member);
                            }
                            None => name.push_str("[*]"),
                        }
                    }
                    return Some(name);
                }
                MemoryRegionKind::ParameterPointee { binding, .. } => {
                    let mut name = format!("*{}", self.names.get(binding.0 as usize)?);
                    for component in components.iter().rev() {
                        match component {
                            Some(member) => {
                                name.push('.');
                                name.push_str(member);
                            }
                            None => name.push_str("[*]"),
                        }
                    }
                    return Some(name);
                }
                MemoryRegionKind::Field { base, member, .. } => {
                    components.push(Some(member.as_str()));
                    current = *base;
                }
                MemoryRegionKind::Elements { base } => {
                    components.push(None);
                    current = *base;
                }
            }
        }
    }

    /// Binding whose storage owns `id`, following parent links defensively.
    pub fn memory_region_root_binding(&self, mut id: MemoryRegionId) -> Option<Binding> {
        let mut visited = std::collections::BTreeSet::new();
        loop {
            if !visited.insert(id) {
                return None;
            }
            match self.memory_regions.get(id.0 as usize)?.kind {
                MemoryRegionKind::Binding { binding } => return Some(binding),
                MemoryRegionKind::ParameterPointee { .. } => return None,
                MemoryRegionKind::Field { base, .. } | MemoryRegionKind::Elements { base } => {
                    id = base;
                }
            }
        }
    }

    /// Every call this function makes, in the shape a summary consumes.
    pub fn call_sites(&self) -> Vec<crate::csource::dataflow::interproc::CallSite> {
        use crate::csource::dataflow::interproc::CallSite;
        self.calls
            .iter()
            .map(|record| CallSite {
                callee: record.callee.clone(),
                arguments: record.arguments.clone(),
                result_is_returned: record.result_is_returned,
            })
            .collect()
    }

    /// The first binding named `name`, including unresolved names.
    ///
    /// The innermost is not distinguishable here: two shadowed declarations of
    /// one name are two bindings and this returns the first. A caller that
    /// cares about shadowing walks [`DataFlow::names`] itself.
    pub fn binding_named(&self, name: &str) -> Option<Binding> {
        self.names
            .iter()
            .position(|candidate| candidate == name)
            .map(|index| Binding(index as u32))
    }

    /// Bindings that are declared and never read.
    ///
    /// Distinct from a dead store: `int *b;` is not a store at all, so it is
    /// not in [`DataFlow::dead_stores`], but it is still a local nothing uses.
    /// A parameter is excluded for the same reason it is there --- the
    /// signature is the contract.
    pub fn unused_bindings(&self) -> Vec<Binding> {
        let mut used = vec![false; self.names.len()];
        for use_ in &self.uses {
            if let Some(slot) = used.get_mut(use_.binding.0 as usize) {
                *slot = true;
            }
        }
        for access in &self.memory_accesses {
            if let Some(binding) = self.memory_region_root_binding(access.region) {
                if let Some(slot) = used.get_mut(binding.0 as usize) {
                    *slot = true;
                }
            }
        }
        for definition in &self.definitions {
            if matches!(definition.kind, DefKind::Parameter | DefKind::AddressTaken) {
                if let Some(slot) = used.get_mut(definition.binding.0 as usize) {
                    *slot = true;
                }
            }
        }
        (0..self.names.len() as u32)
            .map(Binding)
            .filter(|binding| {
                !self.unresolved_bindings.contains(binding) && !used[binding.0 as usize]
            })
            .collect()
    }

    /// The declared type of `binding`, when one was recovered.
    pub fn type_of(&self, binding: Binding) -> Option<&CType> {
        if binding.is_free() {
            return None;
        }
        self.types
            .get(binding.0 as usize)
            .filter(|ty| !ty.is_empty())
    }

    /// Bindings whose reaching definitions disagree about type.
    ///
    /// The shape of a decompiler's type-recovery failure, findable in the
    /// recovered C without the binary. Empty for well-typed source, because a
    /// C compiler would have rejected the disagreement.
    pub fn type_conflicts(&self) -> Vec<Binding> {
        let mut out: Vec<Binding> = Vec::new();
        for (index, ty) in self.types.iter().enumerate() {
            if ty.is_empty() {
                continue;
            }
            let binding = Binding(index as u32);
            let mut seen: Option<&CType> = None;
            for definition in &self.definitions {
                if definition.binding != binding {
                    continue;
                }
                let Some(other) = definition.declared.as_ref() else {
                    continue;
                };
                match seen {
                    None => seen = Some(other),
                    Some(first) if !first.same_shape(other) => {
                        out.push(binding);
                        break;
                    }
                    _ => {}
                }
            }
        }
        out
    }

    /// Whether `def_index` is a dead store.
    pub fn is_dead_store(&self, def_index: u32) -> bool {
        self.dead_stores.contains(&def_index)
    }

    /// Uses that `def_index` reaches.
    pub fn uses_reached(&self, def_index: u32) -> impl Iterator<Item = &Use> {
        self.edges
            .iter()
            .filter(move |edge| edge.def == def_index)
            .filter_map(|edge| self.uses.get(edge.use_ as usize))
    }
}
