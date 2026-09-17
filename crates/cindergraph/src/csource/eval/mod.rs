//! Executable semantic operations lowered from resolved syntax and types.
//!
//! This module is the ownership boundary between structural C semantics and
//! dataflow. Analyses consume these operations; they do not rediscover type
//! formation or captured-bound reads from enclosing source spans.

use std::collections::{BTreeMap, BTreeSet};

use crate::csource::cfg::FunctionCfg;
use crate::csource::lex::kind::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::csource::semantic::declarations::{FunctionResolution, SymbolId, SymbolKind};
use crate::csource::semantic::types::{BoundSlotId, FunctionTypes};
use crate::syntax::cfg::{Cfg, EdgeKind, NodeKind};
use crate::syntax::ids::{NodeId, Span, TokenId};

/// Dense operation identity within one function evaluation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct OpId(pub(crate) u32);

/// Dense semantic value identity within one function evaluation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ValueId(pub(crate) u32);

/// Stable identity of one resolved scalar storage location.
///
/// Scalar places currently correspond one-to-one with value declarations.
/// Keeping their identity distinct from source coordinates lets later place
/// kinds (fields, elements, dereferences, and unknown memory) join the same
/// operation contract without turning spans into object identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct PlaceId(pub(crate) u32);

impl From<SymbolId> for PlaceId {
    fn from(value: SymbolId) -> Self {
        Self(value.0)
    }
}

/// Dense identity of a non-scalar place in one evaluation plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct ProjectedPlaceId(pub(crate) u32);

/// A storage place attached to an executable operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum SemanticPlace {
    Scalar(PlaceId),
    Projected(ProjectedPlaceId),
}

/// Base identity of a projected place after operation operands are resolved.
///
/// Pointer-member access projects from an evaluated pointer value, while a
/// direct member access projects from the aggregate object's storage place.
/// Keeping those alternatives distinct prevents `object.member` from being
/// represented as a fictitious read of the complete aggregate value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProjectedBase {
    Value(ValueId),
    Place(PlaceId),
    Projected(ProjectedPlaceId),
}

/// Structure of a projected, non-scalar place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ProjectedPlaceKind {
    Dereference {
        address: ValueId,
    },
    Field {
        base: ProjectedBase,
        member: String,
        overlapping_members: bool,
    },
    Element {
        base: ProjectedBase,
        index: ValueId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectedPlace {
    pub(crate) id: ProjectedPlaceId,
    pub(crate) kind: ProjectedPlaceKind,
    pub(crate) span: Span,
}

/// Projection applied to an evaluated base value by a scalar load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectedAccess {
    Dereference,
    Field {
        member: String,
        overlapping_members: bool,
    },
    Element {
        index: ScalarOperand,
    },
}

/// Unresolved operation operand used as the base of a place projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProjectionBaseOperand {
    Value(ScalarOperand),
    Place {
        place: PlaceId,
        declaration: Span,
        occurrence: Span,
    },
    Projected {
        base: Box<ProjectionBaseOperand>,
        access: ProjectedAccess,
        span: Span,
    },
}

impl ProjectionBaseOperand {
    /// How many evaluated values this base contributes to an operation's
    /// inputs, in the order [`Self::scalars`] lists them.
    pub(crate) fn scalar_count(&self) -> usize {
        self.scalars().len()
    }

    fn scalars(&self) -> Vec<ScalarOperand> {
        match self {
            Self::Value(value) => vec![value.clone()],
            Self::Place { .. } => Vec::new(),
            Self::Projected { base, access, .. } => {
                let mut values = base.scalars();
                if let ProjectedAccess::Element { index } = access {
                    values.push(index.clone());
                }
                values
            }
        }
    }

    fn storage_identity_occurrences(&self, out: &mut Vec<Span>) {
        match self {
            Self::Value(_) => {}
            Self::Place { occurrence, .. } => out.push(*occurrence),
            Self::Projected { base, .. } => base.storage_identity_occurrences(out),
        }
    }

    #[cfg(test)]
    fn occurrence(&self) -> Span {
        match self {
            Self::Value(value) => value.occurrence(),
            Self::Place { occurrence, .. } => *occurrence,
            Self::Projected { span, .. } => *span,
        }
    }
}

/// Dense identity for one evaluated expression root within a function.
///
/// This is deliberately distinct from [`BoundSlotId`]: a bound slot is a
/// captured type value, while initializers, returns, and discarded expression
/// statements also evaluate without creating a type slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct EvaluationId(pub(crate) u32);

/// Dense identity for one conditional evaluation region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct GuardId(pub(crate) u32);

/// Which truth value of a condition enters a guarded region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardPolarity {
    WhenTrue,
    WhenFalse,
}

/// How a semantic guard is represented in the executable CFG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GuardControl {
    CfgEdge { branch_node: u32, entry_node: u32 },
    Unresolved,
}

/// A conditional evaluation region rooted at an operation input value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvaluationGuard {
    pub(crate) id: GuardId,
    pub(crate) owner: OpId,
    pub(crate) condition: ValueId,
    pub(crate) polarity: GuardPolarity,
    pub(crate) span: Span,
    pub(crate) control: GuardControl,
}

/// Whether an operation executes whenever its CFG node executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionCondition {
    Unconditional,
    Guarded(Vec<GuardId>),
}

/// Language-defined ordering between two operations in one evaluation root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationOrderKind {
    /// `first` completes before `second` begins.
    SequencedBefore,
    /// Both execute without overlap, but either may execute first.
    IndeterminatelySequenced,
    /// The operations cannot both execute in the same evaluation.
    MutuallyExclusive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EvaluationOrder {
    pub(crate) evaluation: EvaluationId,
    pub(crate) first: OpId,
    pub(crate) second: OpId,
    pub(crate) kind: EvaluationOrderKind,
}

/// A type-driven runtime operation in semantic execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TypeValueOp {
    /// Materialize an integer constant used by an evaluated scalar expression.
    ConstantScalar {
        evaluation: EvaluationId,
        value: ScalarOperand,
    },
    /// Read one resolved scalar while evaluating an expression.
    ReadScalar {
        evaluation: EvaluationId,
        place: PlaceId,
        declaration: Span,
        occurrence: Span,
    },
    /// Produce the address of one resolved scalar place without reading its
    /// stored value.
    AddressOf {
        evaluation: EvaluationId,
        place: PlaceId,
        declaration: Span,
        occurrence: Span,
        span: Span,
    },
    /// Decay a local array object to a pointer to its first element.
    DecayArray {
        evaluation: EvaluationId,
        place: PlaceId,
        declaration: Span,
        occurrence: Span,
    },
    /// Load a scalar value through an evaluated pointer value.
    LoadScalar {
        evaluation: EvaluationId,
        base: ProjectionBaseOperand,
        access: ProjectedAccess,
        span: Span,
    },
    /// Store a scalar value through an evaluated pointer value.
    StoreScalar {
        evaluation: EvaluationId,
        base: ProjectionBaseOperand,
        access: ProjectedAccess,
        prior: Option<ScalarOperand>,
        assigned: ScalarOperand,
        result: ScalarWriteResult,
        target: Span,
        span: Span,
    },
    /// Write one resolved scalar while evaluating an expression.
    WriteScalar {
        evaluation: EvaluationId,
        place: PlaceId,
        declaration: Span,
        occurrence: Span,
        span: Span,
        kind: ScalarWriteKind,
        prior: Option<ScalarOperand>,
        assigned: Option<ScalarOperand>,
        result: ScalarWriteResult,
    },
    /// Select one of two scalar values whose producing operations may
    /// themselves be guarded.
    SelectScalar {
        evaluation: EvaluationId,
        condition: ScalarOperand,
        when_true: ScalarOperand,
        when_false: ScalarOperand,
    },
    /// Compute a side-effect-free scalar value from explicit operands.
    ComputeScalar {
        evaluation: EvaluationId,
        operator: String,
        operands: Vec<ScalarOperand>,
        span: Span,
    },
    /// Compute a side-effect-free unary scalar value.
    UnaryScalar {
        evaluation: EvaluationId,
        operator: String,
        operand: ScalarOperand,
        span: Span,
    },
    /// Compute `&&` or `||`, whose right input is conditionally evaluated.
    ShortCircuitScalar {
        evaluation: EvaluationId,
        operator: String,
        left: ScalarOperand,
        right: ScalarOperand,
        /// The language-defined value on the edge that bypasses `right`:
        /// zero for `&&`, one for `||`.
        bypass: ScalarOperand,
        span: Span,
    },
    /// Execute `before`, discard its value, then yield `value`.
    SequenceScalar {
        evaluation: EvaluationId,
        before: ScalarOperand,
        value: ScalarOperand,
        span: Span,
    },
    /// A call whose result supplies an evaluated scalar expression.
    CallScalar {
        evaluation: EvaluationId,
        callee: String,
        span: Span,
        arguments: Vec<ScalarOperand>,
    },
    /// Evaluate and capture one runtime array bound.
    FormBound {
        evaluation: EvaluationId,
        slot: BoundSlotId,
        declaration: Span,
        expression: Span,
        inputs: Vec<Span>,
    },
    /// Complete an initializer, return, discarded expression, control
    /// condition, or indirect-dispatch operand and hand its value to the
    /// language-level consumer.
    FinishExpression {
        evaluation: EvaluationId,
        purpose: EvaluationPurpose,
        expression: Span,
        value: ScalarOperand,
    },
    /// Consume a previously captured bound at a type-driven value use.
    ReadBound { slot: BoundSlotId, consumer: Span },
}

impl TypeValueOp {
    /// Resolved scalar inputs consumed directly by this operation.
    ///
    /// Leaf reads and writes expose their declaration fields directly. Every
    /// compound operation goes through this interface so adapters do not need
    /// variant-specific operand discovery or source-span inference.
    pub(crate) fn scalar_inputs(&self) -> Vec<ScalarOperand> {
        match self {
            Self::ConstantScalar { value, .. } => vec![value.clone()],
            Self::SelectScalar {
                condition,
                when_true,
                when_false,
                ..
            } => vec![condition.clone(), when_true.clone(), when_false.clone()],
            Self::ComputeScalar { operands, .. } => operands.clone(),
            Self::UnaryScalar { operand, .. } => vec![operand.clone()],
            Self::LoadScalar { base, access, .. } => {
                let mut inputs = base.scalars();
                if let ProjectedAccess::Element { index } = access {
                    inputs.push(index.clone());
                }
                inputs
            }
            Self::StoreScalar {
                base,
                access,
                prior,
                assigned,
                ..
            } => {
                let mut inputs = base.scalars();
                if let ProjectedAccess::Element { index } = access {
                    inputs.push(index.clone());
                }
                inputs.extend(prior.iter().cloned());
                inputs.push(assigned.clone());
                inputs
            }
            Self::ShortCircuitScalar {
                left,
                right,
                bypass,
                ..
            } => vec![left.clone(), right.clone(), bypass.clone()],
            Self::SequenceScalar { value, .. } => vec![value.clone()],
            Self::CallScalar { arguments, .. } => arguments.clone(),
            Self::WriteScalar {
                prior, assigned, ..
            } => prior.iter().chain(assigned.iter()).cloned().collect(),
            Self::FinishExpression { value, .. } => vec![value.clone()],
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ScalarOperand {
    Declaration {
        place: PlaceId,
        declaration: Span,
        occurrence: Span,
    },
    Constant {
        spelling: String,
        occurrence: Span,
    },
    Produced {
        expression: Span,
    },
}

impl ScalarOperand {
    pub(crate) fn declaration(&self) -> Option<Span> {
        match self {
            Self::Declaration { declaration, .. } => Some(*declaration),
            Self::Constant { .. } | Self::Produced { .. } => None,
        }
    }

    pub(crate) fn place(&self) -> Option<PlaceId> {
        match self {
            Self::Declaration { place, .. } => Some(*place),
            Self::Constant { .. } | Self::Produced { .. } => None,
        }
    }

    pub(crate) fn occurrence(&self) -> Span {
        match self {
            Self::Declaration { occurrence, .. } | Self::Constant { occurrence, .. } => *occurrence,
            Self::Produced { expression } => *expression,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarWriteKind {
    Assign,
    CompoundAssign,
    Increment,
}

/// Which side of a scalar write is the value of the enclosing expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarWriteResult {
    PreWrite,
    PostWrite,
}

/// Language-level consumer of one completed expression value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EvaluationPurpose {
    Initialize {
        place: Option<PlaceId>,
        declaration: Span,
    },
    Return {
        statement: Span,
    },
    Discard {
        statement: Span,
    },
    Control {
        statement: Span,
        kind: ControlConditionKind,
    },
    ForClause {
        statement: Span,
        phase: ForClausePhase,
    },
    IndirectDispatch {
        statement: Span,
    },
}

/// One operation-derived source of a pointer value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PointerValueSource {
    Address(PlaceId),
    Copy { place: PlaceId, occurrence: Span },
    Load { pointer: PlaceId, occurrence: Span },
    Unknown,
}

/// A pointer-valued initialization or direct assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PointerValueConstraint {
    pub(crate) destination: PlaceId,
    pub(crate) occurrence: Span,
    pub(crate) effect: Span,
    pub(crate) sources: Vec<PointerValueSource>,
}

/// A pointer value consumed as the address of an operation-owned scalar load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PointerLoadConstraint {
    pub(crate) span: Span,
    pub(crate) sources: Vec<PointerValueSource>,
}

/// A pointer value consumed as the address of an operation-owned scalar store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PointerStoreConstraint {
    pub(crate) span: Span,
    pub(crate) target: Span,
    pub(crate) address_sources: Vec<PointerValueSource>,
    pub(crate) assigned: ValueId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PointerCallConstraint {
    pub(crate) span: Span,
    pub(crate) node: u32,
    pub(crate) arguments: Vec<(u32, Vec<PointerValueSource>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ControlConditionKind {
    If,
    While,
    DoWhile,
    For,
    Switch,
}

/// Repeated-loop clause whose value is discarded after its effects complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForClausePhase {
    Init,
    Step,
}

/// One placed operation with stable identity and executable-CFG ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EvaluationOp {
    pub(crate) id: OpId,
    /// Expression root that owns this operation. `ReadBound` consumes a type
    /// slot later and therefore has no expression-root owner.
    pub(crate) evaluation: Option<EvaluationId>,
    pub(crate) cfg_node: u32,
    pub(crate) inputs: Vec<ValueId>,
    pub(crate) output: ValueId,
    pub(crate) place: Option<SemanticPlace>,
    pub(crate) execution: ExecutionCondition,
    pub(crate) kind: TypeValueOp,
}

/// Why an expression root produced no operations.
///
/// The plan cannot lower every scalar form yet; a consumer that reads the
/// plan as "everything that executes" must be told where it is silent, or
/// it will read silence as absence. Each declined root keeps its purpose and
/// span so an export can stand an explicit unknown operation in its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeclinedReason {
    /// A form `lower_scalar_expression` has no rule for: a cast, `sizeof`
    /// as an operand, a file-scope object, a non-integer literal, a call
    /// through a local function pointer, an assignment to a non-place.
    UnsupportedForm,
    /// Two side effects with no dependency, short-circuit or
    /// mutual-exclusion proof between them.
    UnsequencedEffects,
    /// A braced initializer: aggregate destinations are not scalar roots.
    BracedInitializer,
    /// At least one lowered operation had no CFG node to be placed in.
    Unplaced,
}

impl DeclinedReason {
    /// Stable lowercase spelling for serialized exports.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::UnsupportedForm => "unsupported_form",
            Self::UnsequencedEffects => "unsequenced_effects",
            Self::BracedInitializer => "braced_initializer",
            Self::Unplaced => "unplaced",
        }
    }
}

/// One expression root the plan declined to lower.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclinedRoot {
    pub(crate) purpose: EvaluationPurpose,
    pub(crate) expression: Span,
    /// The innermost CFG node covering the expression, when there is one.
    pub(crate) cfg_node: Option<u32>,
    pub(crate) reason: DeclinedReason,
}

impl EvaluationOp {
    /// The source span this operation is diagnosed at: the occurrence for a
    /// leaf read, the whole expression for a compound operation.
    pub(crate) fn span(&self) -> Span {
        operation_span(&self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvaluationValue {
    id: ValueId,
    producer: Option<OpId>,
    scalar: Option<ScalarOperand>,
}

/// Evaluation facts for one resolved function.
#[derive(Debug, Clone, Default)]
pub(crate) struct EvaluationPlan {
    operations: Vec<EvaluationOp>,
    values: Vec<EvaluationValue>,
    projected_places: Vec<ProjectedPlace>,
    guards: Vec<EvaluationGuard>,
    order: Vec<EvaluationOrder>,
    unsequenced_roots: Vec<Span>,
    ambiguous_size_names: Vec<Span>,
    lowered_formations: Vec<Span>,
    declined: Vec<DeclinedRoot>,
}

impl EvaluationPlan {
    pub(crate) fn build(
        tree: &Tree,
        text: &str,
        token_spans: &[Span],
        function: &FunctionCfg,
        resolution: &FunctionResolution,
        types: &FunctionTypes,
    ) -> Self {
        let arena = tree.arena();
        let root = function.node;
        let mut plan = Self::default();
        let mut kinds = Vec::new();
        let mut next_evaluation = types
            .bound_slots()
            .map(|(slot, _)| slot.0 + 1)
            .max()
            .unwrap_or(0);
        for (slot, bound) in types.bound_slots() {
            let evaluation = EvaluationId(slot.0);
            let form_inputs = if let Some((mut scalar, result_inputs)) = simple_scalar_bound_ops(
                tree,
                text,
                token_spans,
                resolution,
                types,
                evaluation,
                bound.expression,
            ) {
                plan.lowered_formations.push(bound.expression);
                kinds.append(&mut scalar);
                result_inputs
            } else {
                bound.input_declarations.clone()
            };
            kinds.push(TypeValueOp::FormBound {
                evaluation,
                slot,
                declaration: bound.declaration,
                expression: bound.expression,
                inputs: form_inputs,
            });
        }
        let mut declined = Vec::new();
        let mut root_of_evaluation = BTreeMap::new();
        for (purpose, expression) in ordinary_expression_roots(tree, token_spans, root, resolution)
        {
            let evaluation = EvaluationId(next_evaluation);
            let (mut operations, value) = match lower_scalar_span(
                tree,
                text,
                token_spans,
                resolution,
                types,
                evaluation,
                expression,
            ) {
                Ok(lowered) => lowered,
                Err(ScalarRootFailure::UnsequencedEffects) => {
                    plan.unsequenced_roots.push(expression);
                    declined.push((purpose, expression, DeclinedReason::UnsequencedEffects));
                    continue;
                }
                Err(ScalarRootFailure::Unsupported) => {
                    declined.push((purpose, expression, DeclinedReason::UnsupportedForm));
                    continue;
                }
            };
            root_of_evaluation.insert(evaluation, (purpose, expression));
            next_evaluation += 1;
            kinds.append(&mut operations);
            kinds.push(TypeValueOp::FinishExpression {
                evaluation,
                purpose,
                expression,
                value,
            });
        }
        for (purpose, expression) in braced_initializer_roots(tree, token_spans, root, resolution) {
            declined.push((purpose, expression, DeclinedReason::BracedInitializer));
        }
        for node in arena.preorder(root) {
            if arena.tag(node) != Some(NodeTag::UnaryExpr.as_u16())
                || !starts_with_size_operator(tree, text, node)
            {
                continue;
            }
            let Some(consumer) = lone_parenthesized_name(tree, token_spans, node) else {
                continue;
            };
            plan.ambiguous_size_names.push(consumer);
            let Some(declaration) =
                resolution.resolve_at(text.get(consumer.range()).unwrap_or_default(), consumer.lo)
            else {
                continue;
            };
            if !matches!(declaration.kind, SymbolKind::Value | SymbolKind::Typedef) {
                continue;
            }
            kinds.extend(
                types
                    .runtime_bound_slots_of_declaration(declaration.span)
                    .into_iter()
                    .map(|(slot, _)| TypeValueOp::ReadBound { slot, consumer }),
            );
        }
        let body_start = arena
            .preorder(root)
            .find(|node| arena.tag(*node) == Some(NodeTag::CompoundStmt.as_u16()))
            .and_then(|node| arena.span(node, token_spans))
            .map_or(function.span.hi, |span| span.lo);
        let mut placement_index = function
            .cfg
            .nodes()
            .iter()
            .enumerate()
            .map(|(index, node)| (node.span(), index as u32))
            .collect::<Vec<_>>();
        placement_index.sort_by_key(|(span, index)| (span.hi.saturating_sub(span.lo), *index));
        let mut unplaced_evaluations = BTreeSet::new();
        let mut placed = Vec::with_capacity(kinds.len());
        for kind in kinds {
            match cfg_node_for_span(&placement_index, operation_span(&kind), body_start) {
                Some(cfg_node) => placed.push((cfg_node, kind)),
                None => {
                    unplaced_evaluations.extend(operation_evaluation(&kind));
                }
            }
        }
        // A root with an unplaced operation is incomplete; keep the placed
        // operations (dataflow consumes them today) but say so.
        for evaluation in &unplaced_evaluations {
            if let Some((purpose, expression)) = root_of_evaluation.get(evaluation) {
                declined.push((*purpose, *expression, DeclinedReason::Unplaced));
            }
        }
        declined.sort_by_key(|(_, expression, _)| (expression.lo, expression.hi));
        plan.declined = declined
            .into_iter()
            .map(|(purpose, expression, reason)| DeclinedRoot {
                purpose,
                expression,
                cfg_node: cfg_node_for_span(&placement_index, expression, body_start),
                reason,
            })
            .collect();
        let mut expression_result = BTreeMap::new();
        let mut formed_slot_value = BTreeMap::new();
        let mut produced_expression = BTreeMap::new();
        let mut projected_by_kind = BTreeMap::new();
        for (index, (cfg_node, kind)) in placed.into_iter().enumerate() {
            let id = OpId(index as u32);
            let evaluation = operation_evaluation(&kind);
            let mut inputs = Vec::new();
            for scalar in kind.scalar_inputs() {
                if let ScalarOperand::Produced { expression } = scalar {
                    inputs.extend(produced_expression.get(&expression).copied());
                    continue;
                }
                let value = ValueId(plan.values.len() as u32);
                plan.values.push(EvaluationValue {
                    id: value,
                    producer: None,
                    scalar: Some(scalar),
                });
                inputs.push(value);
            }
            match &kind {
                TypeValueOp::FormBound {
                    inputs: leaves,
                    expression,
                    ..
                } => {
                    if plan.lowered_formations.contains(expression) {
                        inputs
                            .extend(evaluation.and_then(|id| expression_result.get(&id).copied()));
                    } else {
                        for declaration in leaves {
                            let place = resolution
                                .declaration_at(*declaration)
                                .filter(|symbol| symbol.kind == SymbolKind::Value)
                                .map(|symbol| PlaceId::from(symbol.id));
                            let Some(place) = place else {
                                continue;
                            };
                            let value = ValueId(plan.values.len() as u32);
                            plan.values.push(EvaluationValue {
                                id: value,
                                producer: None,
                                scalar: Some(ScalarOperand::Declaration {
                                    place,
                                    declaration: *declaration,
                                    occurrence: *expression,
                                }),
                            });
                            inputs.push(value);
                        }
                    }
                }
                TypeValueOp::ReadBound { slot, .. } => {
                    inputs.extend(formed_slot_value.get(slot).copied());
                }
                _ => {}
            }
            let place = match &kind {
                TypeValueOp::ReadScalar { place, .. }
                | TypeValueOp::AddressOf { place, .. }
                | TypeValueOp::WriteScalar { place, .. } => Some(SemanticPlace::Scalar(*place)),
                TypeValueOp::LoadScalar {
                    base, access, span, ..
                } => materialize_projected_place(
                    base,
                    access,
                    *span,
                    &inputs,
                    &mut projected_by_kind,
                    &mut plan.projected_places,
                )
                .map(SemanticPlace::Projected),
                TypeValueOp::StoreScalar {
                    base,
                    access,
                    target,
                    ..
                } => materialize_projected_place(
                    base,
                    access,
                    *target,
                    &inputs,
                    &mut projected_by_kind,
                    &mut plan.projected_places,
                )
                .map(SemanticPlace::Projected),
                _ => None,
            };
            let output = ValueId(plan.values.len() as u32);
            let scalar = match &kind {
                TypeValueOp::ReadScalar {
                    place,
                    declaration,
                    occurrence,
                    ..
                } => Some(ScalarOperand::Declaration {
                    place: *place,
                    declaration: *declaration,
                    occurrence: *occurrence,
                }),
                _ => None,
            };
            plan.values.push(EvaluationValue {
                id: output,
                producer: Some(id),
                scalar,
            });
            if matches!(&kind, TypeValueOp::FormBound { .. }) {
                if let TypeValueOp::FormBound { slot, .. } = &kind {
                    formed_slot_value.insert(*slot, output);
                }
            } else if !matches!(&kind, TypeValueOp::ReadBound { .. }) {
                let result = match &kind {
                    TypeValueOp::WriteScalar {
                        result: ScalarWriteResult::PreWrite,
                        ..
                    } => inputs.first().copied().unwrap_or(output),
                    TypeValueOp::StoreScalar {
                        base,
                        access,
                        prior: Some(_),
                        result: ScalarWriteResult::PreWrite,
                        ..
                    } => inputs
                        .get(projected_address_input_count(base, access))
                        .copied()
                        .unwrap_or(output),
                    _ => output,
                };
                produced_expression.insert(operation_span(&kind), result);
                if let Some(evaluation) = evaluation {
                    expression_result.insert(evaluation, result);
                }
            }
            plan.operations.push(EvaluationOp {
                id,
                evaluation,
                cfg_node,
                inputs,
                output,
                place,
                execution: ExecutionCondition::Unconditional,
                kind,
            });
        }
        plan.attach_conditional_regions(&function.cfg);
        plan.attach_evaluation_order();
        plan
    }

    pub(crate) fn operations(&self) -> &[EvaluationOp] {
        &self.operations
    }

    pub(crate) fn projected_places(&self) -> &[ProjectedPlace] {
        &self.projected_places
    }

    pub(crate) fn unsequenced_roots(&self) -> &[Span] {
        &self.unsequenced_roots
    }

    /// Expression roots that produced no operations, with the reason, in
    /// source order.
    pub(crate) fn declined_roots(&self) -> &[DeclinedRoot] {
        &self.declined
    }

    /// Conditional evaluation regions, indexed by `GuardId`.
    pub(crate) fn guards(&self) -> &[EvaluationGuard] {
        &self.guards
    }

    /// The scalar operand a leaf value stands for, when it has one.
    pub(crate) fn value_scalar(&self, value: ValueId) -> Option<&ScalarOperand> {
        self.values
            .get(value.0 as usize)
            .and_then(|value| value.scalar.as_ref())
    }

    #[cfg(test)]
    pub(crate) fn order(&self) -> &[EvaluationOrder] {
        &self.order
    }

    fn attach_evaluation_order(&mut self) {
        if !self.operations.iter().any(|operation| {
            matches!(
                operation.kind,
                TypeValueOp::WriteScalar { .. }
                    | TypeValueOp::StoreScalar { .. }
                    | TypeValueOp::CallScalar { .. }
            )
        }) {
            return;
        }
        let expression_producer = self
            .operations
            .iter()
            .map(|operation| (operation_span(&operation.kind), operation.id))
            .collect::<BTreeMap<_, _>>();
        let effectful = self
            .operations
            .iter()
            .map(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::WriteScalar { .. }
                        | TypeValueOp::StoreScalar { .. }
                        | TypeValueOp::CallScalar { .. }
                )
            })
            .collect::<Vec<_>>();
        let mut edges = vec![Vec::<OpId>::new(); self.operations.len()];
        let mut add_sequence = |evaluation: EvaluationId, first: OpId, second: OpId| {
            if first == second {
                return;
            }
            if let Some(successors) = edges.get_mut(first.0 as usize) {
                successors.push(second);
            }
            if effectful.get(first.0 as usize).copied().unwrap_or(false)
                || effectful.get(second.0 as usize).copied().unwrap_or(false)
            {
                self.order.push(EvaluationOrder {
                    evaluation,
                    first,
                    second,
                    kind: EvaluationOrderKind::SequencedBefore,
                });
            }
        };
        for operation in &self.operations {
            let Some(evaluation) = operation.evaluation else {
                continue;
            };
            for input in &operation.inputs {
                if let Some(first) = self
                    .values
                    .get(input.0 as usize)
                    .and_then(|value| value.producer)
                {
                    add_sequence(evaluation, first, operation.id);
                }
            }
            if let TypeValueOp::SequenceScalar { before, value, .. } = &operation.kind {
                if let (Some(first), Some(second)) = (
                    expression_producer.get(&before.occurrence()).copied(),
                    expression_producer.get(&value.occurrence()).copied(),
                ) {
                    add_sequence(evaluation, first, second);
                }
            }
            if let ExecutionCondition::Guarded(guards) = &operation.execution {
                for guard in guards {
                    if let Some(first) = self
                        .guards
                        .get(guard.0 as usize)
                        .and_then(|guard| self.values.get(guard.condition.0 as usize))
                        .and_then(|value| value.producer)
                    {
                        add_sequence(evaluation, first, operation.id);
                    }
                }
            }
        }
        self.order.sort_by_key(|order| (order.first, order.second));
        self.order.dedup_by_key(|order| (order.first, order.second));

        let effects = self
            .operations
            .iter()
            .filter(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::CallScalar { .. } | TypeValueOp::WriteScalar { .. }
                )
            })
            .collect::<Vec<_>>();
        for (index, first) in effects.iter().enumerate() {
            for second in &effects[index + 1..] {
                if first.evaluation != second.evaluation {
                    continue;
                }
                let Some(evaluation) = first.evaluation else {
                    continue;
                };
                if operation_reaches(&edges, first.id, second.id)
                    || operation_reaches(&edges, second.id, first.id)
                {
                    continue;
                }
                let kind = if operations_are_mutually_exclusive(first, second, &self.guards) {
                    EvaluationOrderKind::MutuallyExclusive
                } else if matches!(first.kind, TypeValueOp::CallScalar { .. })
                    && matches!(second.kind, TypeValueOp::CallScalar { .. })
                {
                    EvaluationOrderKind::IndeterminatelySequenced
                } else {
                    continue;
                };
                self.order.push(EvaluationOrder {
                    evaluation,
                    first: first.id,
                    second: second.id,
                    kind,
                });
            }
        }
        self.order.sort_by_key(|order| (order.first, order.second));
    }

    fn attach_conditional_regions(&mut self, cfg: &Cfg) {
        let mut candidates = Vec::new();
        for operation in &self.operations {
            match &operation.kind {
                TypeValueOp::ShortCircuitScalar {
                    operator,
                    left,
                    right,
                    ..
                } => {
                    let Some(condition) = operation.inputs.first().copied() else {
                        continue;
                    };
                    let polarity = if operator == "&&" {
                        GuardPolarity::WhenTrue
                    } else {
                        GuardPolarity::WhenFalse
                    };
                    candidates.push((
                        operation.id,
                        condition,
                        left.occurrence(),
                        polarity,
                        right.occurrence(),
                    ));
                }
                TypeValueOp::SelectScalar {
                    condition: condition_operand,
                    when_true,
                    when_false,
                    ..
                } => {
                    let Some(condition) = operation.inputs.first().copied() else {
                        continue;
                    };
                    candidates.push((
                        operation.id,
                        condition,
                        condition_operand.occurrence(),
                        GuardPolarity::WhenTrue,
                        when_true.occurrence(),
                    ));
                    candidates.push((
                        operation.id,
                        condition,
                        condition_operand.occurrence(),
                        GuardPolarity::WhenFalse,
                        when_false.occurrence(),
                    ));
                }
                _ => {}
            }
        }
        for (owner, condition, condition_span, polarity, span) in candidates {
            let control = guard_cfg_edge(cfg, condition_span, polarity).map_or(
                GuardControl::Unresolved,
                |(branch_node, entry_node)| GuardControl::CfgEdge {
                    branch_node,
                    entry_node,
                },
            );
            let id = GuardId(self.guards.len() as u32);
            self.guards.push(EvaluationGuard {
                id,
                owner,
                condition,
                polarity,
                span,
                control,
            });
            for operation in &mut self.operations {
                let candidate = operation_span(&operation.kind);
                if operation.id != owner && span.lo <= candidate.lo && candidate.hi <= span.hi {
                    match &mut operation.execution {
                        ExecutionCondition::Unconditional => {
                            operation.execution = ExecutionCondition::Guarded(vec![id]);
                        }
                        ExecutionCondition::Guarded(guards) => guards.push(id),
                    }
                }
            }
        }
    }

    /// Declaration-backed inputs of an operation, resolved through `ValueId`.
    #[cfg(test)]
    pub(crate) fn scalar_inputs_of(&self, operation: &EvaluationOp) -> Vec<ScalarOperand> {
        if !matches!(
            operation.kind,
            TypeValueOp::AddressOf { .. }
                | TypeValueOp::WriteScalar { .. }
                | TypeValueOp::SelectScalar { .. }
                | TypeValueOp::ComputeScalar { .. }
                | TypeValueOp::UnaryScalar { .. }
                | TypeValueOp::LoadScalar { .. }
                | TypeValueOp::StoreScalar { .. }
                | TypeValueOp::ShortCircuitScalar { .. }
                | TypeValueOp::SequenceScalar { .. }
                | TypeValueOp::CallScalar { .. }
                | TypeValueOp::FinishExpression { .. }
        ) {
            return Vec::new();
        }
        operation
            .inputs
            .iter()
            .filter_map(|value| self.values.get(value.0 as usize)?.scalar.clone())
            .collect()
    }

    /// Scalar occurrences evaluated directly by `operation`.
    ///
    /// Produced values can retain scalar provenance, but consuming such a
    /// value does not evaluate its source occurrence again. Event adapters use
    /// this view; transitive provenance walks producer edges separately.
    pub(crate) fn direct_scalar_inputs_of(&self, operation: &EvaluationOp) -> Vec<ScalarOperand> {
        if let TypeValueOp::ReadScalar {
            place,
            declaration,
            occurrence,
            ..
        } = operation.kind
        {
            return vec![ScalarOperand::Declaration {
                place,
                declaration,
                occurrence,
            }];
        }
        operation
            .inputs
            .iter()
            .filter_map(|value| {
                let record = self.values.get(value.0 as usize)?;
                record
                    .producer
                    .is_none()
                    .then(|| record.scalar.clone())
                    .flatten()
            })
            .collect()
    }

    /// Source names used only to identify direct storage, not read its value.
    pub(crate) fn storage_identity_occurrences_of(&self, operation: &EvaluationOp) -> Vec<Span> {
        if let TypeValueOp::DecayArray { occurrence, .. } = operation.kind {
            return vec![occurrence];
        }
        let base = match &operation.kind {
            TypeValueOp::LoadScalar { base, .. } | TypeValueOp::StoreScalar { base, .. } => base,
            _ => return Vec::new(),
        };
        let mut occurrences = Vec::new();
        base.storage_identity_occurrences(&mut occurrences);
        occurrences
    }

    /// Pointer values consumed by operation-owned indirect loads.
    pub(crate) fn pointer_load_constraints(&self) -> Vec<PointerLoadConstraint> {
        self.operations
            .iter()
            .filter_map(|load| {
                let TypeValueOp::LoadScalar { span, .. } = load.kind else {
                    return None;
                };
                let place = match load.place? {
                    SemanticPlace::Projected(place) => place,
                    SemanticPlace::Scalar(_) => return None,
                };
                let address = match self.projected_places.get(place.0 as usize)?.kind {
                    ProjectedPlaceKind::Dereference { address } => address,
                    ProjectedPlaceKind::Field { .. } | ProjectedPlaceKind::Element { .. } => {
                        return None;
                    }
                };
                let mut sources = BTreeSet::new();
                self.collect_pointer_sources(address, &mut BTreeSet::new(), &mut sources);
                if sources.is_empty() {
                    sources.insert(PointerValueSource::Unknown);
                }
                Some(PointerLoadConstraint {
                    span,
                    sources: sources.into_iter().collect(),
                })
            })
            .collect()
    }

    /// Pointer values consumed by operation-owned indirect stores.
    pub(crate) fn pointer_store_constraints(&self) -> Vec<PointerStoreConstraint> {
        self.operations
            .iter()
            .filter_map(|store| {
                let TypeValueOp::StoreScalar { target, span, .. } = store.kind else {
                    return None;
                };
                let place = match store.place? {
                    SemanticPlace::Projected(place) => place,
                    SemanticPlace::Scalar(_) => return None,
                };
                let address = match self.projected_places.get(place.0 as usize)?.kind {
                    ProjectedPlaceKind::Dereference { address } => address,
                    ProjectedPlaceKind::Field { .. } | ProjectedPlaceKind::Element { .. } => {
                        return None;
                    }
                };
                let mut address_sources = BTreeSet::new();
                self.collect_pointer_sources(address, &mut BTreeSet::new(), &mut address_sources);
                if address_sources.is_empty() {
                    address_sources.insert(PointerValueSource::Unknown);
                }
                Some(PointerStoreConstraint {
                    span,
                    target,
                    address_sources: address_sources.into_iter().collect(),
                    assigned: *store.inputs.last()?,
                })
            })
            .collect()
    }

    /// Pointer sources of one already-materialized value.
    pub(crate) fn pointer_sources(&self, value: ValueId) -> Vec<PointerValueSource> {
        let mut sources = BTreeSet::new();
        self.collect_pointer_sources(value, &mut BTreeSet::new(), &mut sources);
        if sources.is_empty() {
            sources.insert(PointerValueSource::Unknown);
        }
        sources.into_iter().collect()
    }

    /// Pointer-like argument sources consumed by operation-owned calls.
    pub(crate) fn pointer_call_constraints(&self) -> Vec<PointerCallConstraint> {
        self.operations
            .iter()
            .filter_map(|operation| {
                let TypeValueOp::CallScalar { span, .. } = operation.kind else {
                    return None;
                };
                let arguments = operation
                    .inputs
                    .iter()
                    .enumerate()
                    .filter_map(|(position, value)| {
                        let sources = self.pointer_sources(*value);
                        sources
                            .iter()
                            .any(|source| !matches!(source, PointerValueSource::Unknown))
                            .then_some((position as u32, sources))
                    })
                    .collect::<Vec<_>>();
                (!arguments.is_empty()).then_some(PointerCallConstraint {
                    span,
                    node: operation.cfg_node,
                    arguments,
                })
            })
            .collect()
    }

    /// Pointer-value constraints expressed entirely in operation/value terms.
    pub(crate) fn pointer_value_constraints(&self) -> Vec<PointerValueConstraint> {
        self.operations
            .iter()
            .filter_map(|operation| {
                let (destination, occurrence, effect, value) = match &operation.kind {
                    TypeValueOp::FinishExpression {
                        purpose:
                            EvaluationPurpose::Initialize {
                                place: Some(place),
                                declaration,
                            },
                        expression,
                        ..
                    } => (
                        *place,
                        *declaration,
                        *expression,
                        *operation.inputs.first()?,
                    ),
                    TypeValueOp::WriteScalar {
                        place,
                        occurrence,
                        span,
                        kind: ScalarWriteKind::Assign,
                        assigned: Some(_),
                        ..
                    } => (*place, *occurrence, *span, *operation.inputs.last()?),
                    _ => return None,
                };
                let mut sources = BTreeSet::new();
                self.collect_pointer_sources(value, &mut BTreeSet::new(), &mut sources);
                if sources.is_empty() {
                    sources.insert(PointerValueSource::Unknown);
                }
                Some(PointerValueConstraint {
                    destination,
                    occurrence,
                    effect,
                    sources: sources.into_iter().collect(),
                })
            })
            .collect()
    }

    fn collect_pointer_sources(
        &self,
        value: ValueId,
        visited: &mut BTreeSet<ValueId>,
        sources: &mut BTreeSet<PointerValueSource>,
    ) {
        if !visited.insert(value) {
            sources.insert(PointerValueSource::Unknown);
            return;
        }
        let Some(record) = self.values.get(value.0 as usize) else {
            sources.insert(PointerValueSource::Unknown);
            return;
        };
        let Some(producer) = record.producer else {
            match record.scalar {
                Some(ScalarOperand::Declaration {
                    place, occurrence, ..
                }) => {
                    sources.insert(PointerValueSource::Copy { place, occurrence });
                }
                _ => {
                    sources.insert(PointerValueSource::Unknown);
                }
            }
            return;
        };
        let Some(operation) = self.operations.get(producer.0 as usize) else {
            sources.insert(PointerValueSource::Unknown);
            return;
        };
        match operation.kind {
            TypeValueOp::AddressOf { place, .. } => {
                sources.insert(PointerValueSource::Address(place));
            }
            TypeValueOp::DecayArray { place, .. } => {
                sources.insert(PointerValueSource::Address(place));
            }
            TypeValueOp::ReadScalar {
                place, occurrence, ..
            } => {
                sources.insert(PointerValueSource::Copy { place, occurrence });
            }
            TypeValueOp::LoadScalar { .. } => {
                let Some(SemanticPlace::Projected(place)) = operation.place else {
                    sources.insert(PointerValueSource::Unknown);
                    return;
                };
                if !matches!(
                    self.projected_places
                        .get(place.0 as usize)
                        .map(|place| &place.kind),
                    Some(ProjectedPlaceKind::Dereference { .. })
                ) {
                    sources.insert(PointerValueSource::Unknown);
                    return;
                }
                let mut address_sources = BTreeSet::new();
                if let Some(address) = operation.inputs.first() {
                    self.collect_pointer_sources(
                        *address,
                        &mut BTreeSet::new(),
                        &mut address_sources,
                    );
                }
                if address_sources.len() == 1 {
                    if let Some(PointerValueSource::Copy { place, occurrence }) =
                        address_sources.iter().next().copied()
                    {
                        sources.insert(PointerValueSource::Load {
                            pointer: place,
                            occurrence,
                        });
                        return;
                    }
                }
                sources.insert(PointerValueSource::Unknown);
            }
            TypeValueOp::SelectScalar { .. } => {
                for input in operation.inputs.iter().skip(1) {
                    let mut branch = visited.clone();
                    self.collect_pointer_sources(*input, &mut branch, sources);
                }
            }
            TypeValueOp::SequenceScalar { .. } => {
                if let Some(value) = operation.inputs.last() {
                    self.collect_pointer_sources(*value, visited, sources);
                } else {
                    sources.insert(PointerValueSource::Unknown);
                }
            }
            TypeValueOp::FinishExpression { .. } => {
                for input in &operation.inputs {
                    self.collect_pointer_sources(*input, visited, sources);
                }
            }
            TypeValueOp::ComputeScalar { .. } | TypeValueOp::UnaryScalar { .. } => {
                // Arithmetic and casts prevent a complete target claim, but
                // their pointer operands remain valid may-target evidence.
                for input in &operation.inputs {
                    self.collect_pointer_sources(*input, visited, sources);
                }
                sources.insert(PointerValueSource::Unknown);
            }
            TypeValueOp::StoreScalar {
                ref base,
                ref access,
                result,
                ..
            } => {
                let value = match result {
                    ScalarWriteResult::PostWrite => operation.inputs.last(),
                    ScalarWriteResult::PreWrite => operation
                        .inputs
                        .get(projected_address_input_count(base, access)),
                };
                if let Some(value) = value {
                    self.collect_pointer_sources(*value, visited, sources);
                } else {
                    sources.insert(PointerValueSource::Unknown);
                }
            }
            TypeValueOp::WriteScalar {
                kind: ScalarWriteKind::Assign,
                result,
                ..
            } => {
                let input = match result {
                    ScalarWriteResult::PreWrite => operation.inputs.first(),
                    ScalarWriteResult::PostWrite => operation.inputs.last(),
                };
                if let Some(input) = input {
                    self.collect_pointer_sources(*input, visited, sources);
                } else {
                    sources.insert(PointerValueSource::Unknown);
                }
            }
            _ => {
                sources.insert(PointerValueSource::Unknown);
            }
        }
    }

    pub(crate) fn ambiguous_size_names(&self) -> &[Span] {
        &self.ambiguous_size_names
    }

    pub(crate) fn reads_bound_at(&self, span: Span) -> bool {
        self.operations.iter().any(|operation| {
            matches!(operation.kind, TypeValueOp::ReadBound { consumer, .. } if consumer == span)
        })
    }

    pub(crate) fn models_formation_in(&self, span: Span) -> bool {
        self.lowered_formations
            .iter()
            .any(|expression| span.lo <= expression.lo && expression.hi <= span.hi)
    }

    /// Evaluated roots whose bound formation lies inside `span`.
    pub(crate) fn formation_evaluations_in(&self, span: Span) -> Vec<EvaluationId> {
        self.operations
            .iter()
            .filter_map(|operation| match &operation.kind {
                TypeValueOp::FormBound { expression, .. }
                    if span.lo <= expression.lo && expression.hi <= span.hi =>
                {
                    operation.evaluation
                }
                _ => None,
            })
            .collect()
    }

    /// Whether `span` is an effect syntactically owned by a lowered bound.
    ///
    /// Once the evaluation plan accepts a bound, its reads and writes are the
    /// canonical events. The generic syntax walk must not promote the same
    /// structured assignment or increment a second time.
    pub(crate) fn lowered_formation_contains(&self, span: Span) -> bool {
        self.lowered_formations
            .iter()
            .any(|expression| expression.lo <= span.lo && span.hi <= expression.hi)
    }

    /// Place-effect extents owned by ordinary evaluated roots.
    ///
    /// The syntax compatibility path must not rediscover writes or address
    /// operations already owned by the common evaluation plan.
    pub(crate) fn ordinary_place_effect_spans(&self) -> Vec<Span> {
        let ordinary = self
            .operations
            .iter()
            .filter_map(|operation| {
                matches!(operation.kind, TypeValueOp::FinishExpression { .. })
                    .then_some(operation.evaluation)
                    .flatten()
            })
            .collect::<Vec<_>>();
        self.operations
            .iter()
            .filter_map(|operation| match &operation.kind {
                TypeValueOp::WriteScalar { span, .. } | TypeValueOp::AddressOf { span, .. }
                    if operation
                        .evaluation
                        .is_some_and(|owner| ordinary.contains(&owner)) =>
                {
                    Some(*span)
                }
                _ => None,
            })
            .collect()
    }

    pub(crate) fn bound_inputs(&self, slot: BoundSlotId) -> Option<Vec<Span>> {
        let formation = self
            .operations
            .iter()
            .find(|operation| match &operation.kind {
                TypeValueOp::FormBound {
                    slot: candidate, ..
                } => *candidate == slot,
                _ => false,
            })?;
        let mut declarations = Vec::new();
        let mut pending = formation.inputs.clone();
        let mut seen = Vec::new();
        while let Some(value) = pending.pop() {
            if seen.contains(&value) {
                continue;
            }
            seen.push(value);
            let Some(record) = self.values.get(value.0 as usize) else {
                continue;
            };
            if let Some(declaration) = record.scalar.as_ref().and_then(ScalarOperand::declaration) {
                if !declarations.contains(&declaration) {
                    declarations.push(declaration);
                }
            } else if let Some(producer) = record.producer {
                if let Some(operation) = self.operations.get(producer.0 as usize) {
                    pending.extend(operation.inputs.iter().copied());
                }
            }
        }
        declarations.sort_unstable();
        Some(declarations)
    }
}

fn materialize_projected_place(
    base: &ProjectionBaseOperand,
    access: &ProjectedAccess,
    span: Span,
    inputs: &[ValueId],
    by_kind: &mut BTreeMap<ProjectedPlaceKind, ProjectedPlaceId>,
    places: &mut Vec<ProjectedPlace>,
) -> Option<ProjectedPlaceId> {
    let mut cursor = 0;
    let base = materialize_projected_base(base, inputs, &mut cursor, by_kind, places)?;
    let kind = projected_kind(base, access, inputs, &mut cursor)?;
    Some(intern_projected_place(kind, span, by_kind, places))
}

fn projected_address_input_count(base: &ProjectionBaseOperand, access: &ProjectedAccess) -> usize {
    base.scalars().len() + usize::from(matches!(access, ProjectedAccess::Element { .. }))
}

fn materialize_projected_base(
    base: &ProjectionBaseOperand,
    inputs: &[ValueId],
    cursor: &mut usize,
    by_kind: &mut BTreeMap<ProjectedPlaceKind, ProjectedPlaceId>,
    places: &mut Vec<ProjectedPlace>,
) -> Option<ProjectedBase> {
    match base {
        ProjectionBaseOperand::Value(_) => {
            let value = *inputs.get(*cursor)?;
            *cursor += 1;
            Some(ProjectedBase::Value(value))
        }
        ProjectionBaseOperand::Place { place, .. } => Some(ProjectedBase::Place(*place)),
        ProjectionBaseOperand::Projected { base, access, span } => {
            let base = materialize_projected_base(base, inputs, cursor, by_kind, places)?;
            let kind = projected_kind(base, access, inputs, cursor)?;
            Some(ProjectedBase::Projected(intern_projected_place(
                kind, *span, by_kind, places,
            )))
        }
    }
}

fn projected_kind(
    base: ProjectedBase,
    access: &ProjectedAccess,
    inputs: &[ValueId],
    cursor: &mut usize,
) -> Option<ProjectedPlaceKind> {
    match access {
        ProjectedAccess::Dereference => Some(ProjectedPlaceKind::Dereference {
            address: match base {
                ProjectedBase::Value(value) => value,
                ProjectedBase::Place(_) | ProjectedBase::Projected(_) => return None,
            },
        }),
        ProjectedAccess::Field {
            member,
            overlapping_members,
        } => Some(ProjectedPlaceKind::Field {
            base,
            member: member.clone(),
            overlapping_members: *overlapping_members,
        }),
        ProjectedAccess::Element { .. } => {
            let index = *inputs.get(*cursor)?;
            *cursor += 1;
            Some(ProjectedPlaceKind::Element { base, index })
        }
    }
}

fn intern_projected_place(
    kind: ProjectedPlaceKind,
    span: Span,
    by_kind: &mut BTreeMap<ProjectedPlaceKind, ProjectedPlaceId>,
    places: &mut Vec<ProjectedPlace>,
) -> ProjectedPlaceId {
    if let Some(id) = by_kind.get(&kind) {
        return *id;
    }
    let id = ProjectedPlaceId(places.len() as u32);
    by_kind.insert(kind.clone(), id);
    places.push(ProjectedPlace { id, kind, span });
    id
}

fn operation_evaluation(operation: &TypeValueOp) -> Option<EvaluationId> {
    match operation {
        TypeValueOp::ReadScalar { evaluation, .. }
        | TypeValueOp::ConstantScalar { evaluation, .. }
        | TypeValueOp::AddressOf { evaluation, .. }
        | TypeValueOp::DecayArray { evaluation, .. }
        | TypeValueOp::WriteScalar { evaluation, .. }
        | TypeValueOp::SelectScalar { evaluation, .. }
        | TypeValueOp::ComputeScalar { evaluation, .. }
        | TypeValueOp::UnaryScalar { evaluation, .. }
        | TypeValueOp::LoadScalar { evaluation, .. }
        | TypeValueOp::StoreScalar { evaluation, .. }
        | TypeValueOp::ShortCircuitScalar { evaluation, .. }
        | TypeValueOp::SequenceScalar { evaluation, .. }
        | TypeValueOp::CallScalar { evaluation, .. }
        | TypeValueOp::FormBound { evaluation, .. }
        | TypeValueOp::FinishExpression { evaluation, .. } => Some(*evaluation),
        TypeValueOp::ReadBound { .. } => None,
    }
}

fn operation_span(operation: &TypeValueOp) -> Span {
    match operation {
        TypeValueOp::ConstantScalar { value, .. } => value.occurrence(),
        TypeValueOp::ReadScalar { occurrence, .. } => *occurrence,
        TypeValueOp::AddressOf { span, .. } => *span,
        TypeValueOp::DecayArray { occurrence, .. } => *occurrence,
        TypeValueOp::WriteScalar { span, .. } => *span,
        TypeValueOp::SelectScalar {
            condition,
            when_false,
            ..
        } => Span::new(condition.occurrence().lo, when_false.occurrence().hi),
        TypeValueOp::ComputeScalar { span, .. } => *span,
        TypeValueOp::UnaryScalar { span, .. } => *span,
        TypeValueOp::LoadScalar { span, .. } => *span,
        TypeValueOp::StoreScalar { span, .. } => *span,
        TypeValueOp::ShortCircuitScalar { span, .. } => *span,
        TypeValueOp::SequenceScalar { span, .. } => *span,
        TypeValueOp::CallScalar { span, .. } => *span,
        TypeValueOp::FormBound { expression, .. } => *expression,
        TypeValueOp::FinishExpression { expression, .. } => *expression,
        TypeValueOp::ReadBound { consumer, .. } => *consumer,
    }
}

/// Scalar expression roots that already have an explicit grammatical owner.
///
/// Braced initializers are excluded: their element sequencing and aggregate
/// destinations require typed places rather than one scalar result.
fn ordinary_expression_roots(
    tree: &Tree,
    token_spans: &[Span],
    root: NodeId,
    resolution: &FunctionResolution,
) -> Vec<(EvaluationPurpose, Span)> {
    let arena = tree.arena();
    let mut roots = Vec::new();
    for node in arena.preorder(root) {
        match arena.tag(node).and_then(NodeTag::from_u16) {
            Some(NodeTag::Decl) => {
                let mut declaration = None;
                for child in arena.children_iter(node) {
                    match arena.tag(child).and_then(NodeTag::from_u16) {
                        Some(NodeTag::Declarator) => {
                            declaration = arena.preorder(child).find_map(|inner| {
                                (arena.tag(inner) == Some(NodeTag::DeclName.as_u16()))
                                    .then(|| arena.span(inner, token_spans))
                                    .flatten()
                            });
                        }
                        Some(NodeTag::Initializer) => {
                            let expression = arena.children_iter(child).find_map(|inner| {
                                arena
                                    .tag(inner)
                                    .and_then(NodeTag::from_u16)
                                    .is_some_and(NodeTag::is_expression)
                                    .then(|| arena.span(inner, token_spans))
                                    .flatten()
                            });
                            if let (Some(declaration), Some(expression)) = (declaration, expression)
                            {
                                roots.push((
                                    EvaluationPurpose::Initialize {
                                        place: resolution
                                            .declaration_at(declaration)
                                            .filter(|symbol| symbol.kind == SymbolKind::Value)
                                            .map(|symbol| PlaceId::from(symbol.id)),
                                        declaration,
                                    },
                                    expression,
                                ));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Some(NodeTag::ReturnStmt) => {
                let statement = arena.span(node, token_spans);
                let expression = arena.children_iter(node).find_map(|child| {
                    arena
                        .tag(child)
                        .and_then(NodeTag::from_u16)
                        .is_some_and(NodeTag::is_expression)
                        .then(|| arena.span(child, token_spans))
                        .flatten()
                });
                if let (Some(statement), Some(expression)) = (statement, expression) {
                    roots.push((EvaluationPurpose::Return { statement }, expression));
                }
            }
            Some(NodeTag::ExprStmt) => {
                let statement = arena.span(node, token_spans);
                let expression = arena.children_iter(node).find_map(|child| {
                    arena
                        .tag(child)
                        .and_then(NodeTag::from_u16)
                        .is_some_and(NodeTag::is_expression)
                        .then(|| arena.span(child, token_spans))
                        .flatten()
                });
                if let (Some(statement), Some(expression)) = (statement, expression) {
                    roots.push((EvaluationPurpose::Discard { statement }, expression));
                }
            }
            Some(NodeTag::GotoStmt) => {
                let statement = arena.span(node, token_spans);
                let expression = arena
                    .children_iter(node)
                    .find(|child| {
                        arena
                            .tag(*child)
                            .and_then(NodeTag::from_u16)
                            .is_some_and(NodeTag::is_expression)
                    })
                    .and_then(|computed| {
                        // The parser represents GNU `goto *expr` with a
                        // synthetic outer unary `*`. That star belongs to the
                        // statement grammar, not to a memory load.
                        arena.children_iter(computed).find_map(|child| {
                            arena
                                .tag(child)
                                .and_then(NodeTag::from_u16)
                                .is_some_and(NodeTag::is_expression)
                                .then(|| arena.span(child, token_spans))
                                .flatten()
                        })
                    });
                if let (Some(statement), Some(expression)) = (statement, expression) {
                    roots.push((
                        EvaluationPurpose::IndirectDispatch { statement },
                        expression,
                    ));
                }
            }
            Some(
                tag @ (NodeTag::IfStmt
                | NodeTag::WhileStmt
                | NodeTag::DoWhileStmt
                | NodeTag::SwitchStmt),
            ) => {
                let statement = arena.span(node, token_spans);
                let expression = arena.children_iter(node).find_map(|child| {
                    arena
                        .tag(child)
                        .and_then(NodeTag::from_u16)
                        .is_some_and(NodeTag::is_expression)
                        .then(|| arena.span(child, token_spans))
                        .flatten()
                });
                let kind = match tag {
                    NodeTag::IfStmt => ControlConditionKind::If,
                    NodeTag::WhileStmt => ControlConditionKind::While,
                    NodeTag::DoWhileStmt => ControlConditionKind::DoWhile,
                    NodeTag::SwitchStmt => ControlConditionKind::Switch,
                    _ => unreachable!("matched control statement"),
                };
                if let (Some(statement), Some(expression)) = (statement, expression) {
                    roots.push((EvaluationPurpose::Control { statement, kind }, expression));
                }
            }
            Some(NodeTag::ForStmt) => {
                let statement = arena.span(node, token_spans);
                let clause_expression = |tag: NodeTag| {
                    arena
                        .children_iter(node)
                        .find(|child| arena.tag(*child) == Some(tag.as_u16()))
                        .and_then(|clause| {
                            arena.children_iter(clause).find_map(|child| {
                                arena
                                    .tag(child)
                                    .and_then(NodeTag::from_u16)
                                    .is_some_and(NodeTag::is_expression)
                                    .then(|| arena.span(child, token_spans))
                                    .flatten()
                            })
                        })
                };
                if let (Some(statement), Some(expression)) =
                    (statement, clause_expression(NodeTag::ForInit))
                {
                    roots.push((
                        EvaluationPurpose::ForClause {
                            statement,
                            phase: ForClausePhase::Init,
                        },
                        expression,
                    ));
                }
                if let (Some(statement), Some(expression)) =
                    (statement, clause_expression(NodeTag::ForCond))
                {
                    roots.push((
                        EvaluationPurpose::Control {
                            statement,
                            kind: ControlConditionKind::For,
                        },
                        expression,
                    ));
                }
                if let (Some(statement), Some(expression)) =
                    (statement, clause_expression(NodeTag::ForStep))
                {
                    roots.push((
                        EvaluationPurpose::ForClause {
                            statement,
                            phase: ForClausePhase::Step,
                        },
                        expression,
                    ));
                }
            }
            _ => {}
        }
    }
    roots
}

/// Declarations initialized by a braced list, which
/// [`ordinary_expression_roots`] deliberately skips.
fn braced_initializer_roots(
    tree: &Tree,
    token_spans: &[Span],
    root: NodeId,
    resolution: &FunctionResolution,
) -> Vec<(EvaluationPurpose, Span)> {
    let arena = tree.arena();
    let mut roots = Vec::new();
    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::Decl.as_u16()) {
            continue;
        }
        let mut declaration = None;
        for child in arena.children_iter(node) {
            match arena.tag(child).and_then(NodeTag::from_u16) {
                Some(NodeTag::Declarator) => {
                    declaration = arena.preorder(child).find_map(|inner| {
                        (arena.tag(inner) == Some(NodeTag::DeclName.as_u16()))
                            .then(|| arena.span(inner, token_spans))
                            .flatten()
                    });
                }
                Some(NodeTag::Initializer) => {
                    let list = arena.children_iter(child).find_map(|inner| {
                        (arena.tag(inner) == Some(NodeTag::InitList.as_u16()))
                            .then(|| arena.span(inner, token_spans))
                            .flatten()
                    });
                    if let (Some(declaration), Some(expression)) = (declaration, list) {
                        roots.push((
                            EvaluationPurpose::Initialize {
                                place: resolution
                                    .declaration_at(declaration)
                                    .filter(|symbol| symbol.kind == SymbolKind::Value)
                                    .map(|symbol| PlaceId::from(symbol.id)),
                                declaration,
                            },
                            expression,
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    roots
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScalarRootFailure {
    Unsupported,
    UnsequencedEffects,
}

fn lower_scalar_span(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
    evaluation: EvaluationId,
    expression: Span,
) -> Result<(Vec<TypeValueOp>, ScalarOperand), ScalarRootFailure> {
    let tokens = expression_tokens(tree, text, token_spans, expression);
    let (operations, value, _) = lower_scalar_expression(&tokens, resolution, types, evaluation)
        .ok_or(ScalarRootFailure::Unsupported)?;
    let calls_a_resolved_object = operations.iter().any(|operation| {
        let TypeValueOp::CallScalar { callee, span, .. } = operation else {
            return false;
        };
        resolution
            .resolve_at(callee, span.lo)
            .is_some_and(|declaration| declaration.kind == SymbolKind::Value)
    });
    let writes = operations.iter().any(|operation| {
        matches!(
            operation,
            TypeValueOp::WriteScalar { .. } | TypeValueOp::StoreScalar { .. }
        )
    });
    if calls_a_resolved_object {
        return Err(ScalarRootFailure::Unsupported);
    }
    if writes && !side_effect_pairs_are_supported(&operations) {
        return Err(ScalarRootFailure::UnsequencedEffects);
    }
    Ok((operations, value))
}

fn side_effect_pairs_are_supported(operations: &[TypeValueOp]) -> bool {
    let mut producer = BTreeMap::new();
    let mut predecessors = vec![Vec::new(); operations.len()];
    for (index, operation) in operations.iter().enumerate() {
        for input in operation.scalar_inputs() {
            if let ScalarOperand::Produced { expression } = input {
                if let Some(previous) = producer.get(&expression).copied() {
                    predecessors[index].push(previous);
                }
            }
        }
        if let TypeValueOp::SequenceScalar { before, .. } = operation {
            if let Some(previous) = producer.get(&before.occurrence()).copied() {
                predecessors[index].push(previous);
            }
        }
        producer.insert(operation_span(operation), index);
    }
    let calls = operations
        .iter()
        .enumerate()
        .filter_map(|(index, operation)| {
            matches!(operation, TypeValueOp::CallScalar { .. }).then_some(index)
        })
        .collect::<Vec<_>>();
    let writes = operations
        .iter()
        .enumerate()
        .filter_map(|(index, operation)| {
            matches!(
                operation,
                TypeValueOp::WriteScalar { .. } | TypeValueOp::StoreScalar { .. }
            )
            .then_some(index)
        })
        .collect::<Vec<_>>();
    let pair_is_ordered = |first: usize, second: usize| {
        index_depends_on(&predecessors, first, second)
            || index_depends_on(&predecessors, second, first)
            || type_value_ops_are_sequenced(&operations[first], &operations[second], operations)
            || type_value_ops_are_sequenced(&operations[second], &operations[first], operations)
            || type_value_ops_are_mutually_exclusive(
                &operations[first],
                &operations[second],
                operations,
            )
    };
    if !calls
        .iter()
        .all(|call| writes.iter().all(|write| pair_is_ordered(*call, *write)))
    {
        return false;
    }
    for (offset, first) in writes.iter().enumerate() {
        for second in &writes[offset + 1..] {
            let may_alias = match (&operations[*first], &operations[*second]) {
                (
                    TypeValueOp::WriteScalar {
                        place: first_place, ..
                    },
                    TypeValueOp::WriteScalar {
                        place: second_place,
                        ..
                    },
                ) => first_place == second_place,
                _ => true,
            };
            if may_alias && !pair_is_ordered(*first, *second) {
                return false;
            }
        }
        for (read, operation) in operations.iter().enumerate() {
            let TypeValueOp::ReadScalar {
                place: read_place, ..
            } = operation
            else {
                continue;
            };
            let may_alias = match &operations[*first] {
                TypeValueOp::WriteScalar {
                    place: written_place,
                    ..
                } => written_place == read_place,
                TypeValueOp::StoreScalar { .. } => true,
                _ => false,
            };
            if may_alias && !pair_is_ordered(*first, read) {
                return false;
            }
        }
    }
    true
}

fn type_value_ops_are_sequenced(
    first: &TypeValueOp,
    second: &TypeValueOp,
    operations: &[TypeValueOp],
) -> bool {
    let first = operation_span(first);
    let second = operation_span(second);
    operations.iter().any(|operation| match operation {
        TypeValueOp::SequenceScalar { before, value, .. } => {
            span_contains(before.occurrence(), first) && span_contains(value.occurrence(), second)
        }
        TypeValueOp::ShortCircuitScalar { left, right, .. } => {
            span_contains(left.occurrence(), first) && span_contains(right.occurrence(), second)
        }
        TypeValueOp::SelectScalar {
            condition,
            when_true,
            when_false,
            ..
        } => {
            span_contains(condition.occurrence(), first)
                && (span_contains(when_true.occurrence(), second)
                    || span_contains(when_false.occurrence(), second))
        }
        _ => false,
    })
}

fn type_value_ops_are_mutually_exclusive(
    first: &TypeValueOp,
    second: &TypeValueOp,
    operations: &[TypeValueOp],
) -> bool {
    let first = operation_span(first);
    let second = operation_span(second);
    operations.iter().any(|operation| {
        let TypeValueOp::SelectScalar {
            when_true,
            when_false,
            ..
        } = operation
        else {
            return false;
        };
        let when_true = when_true.occurrence();
        let when_false = when_false.occurrence();
        (span_contains(when_true, first) && span_contains(when_false, second))
            || (span_contains(when_true, second) && span_contains(when_false, first))
    })
}

fn span_contains(outer: Span, inner: Span) -> bool {
    outer.lo <= inner.lo && inner.hi <= outer.hi
}

fn index_depends_on(predecessors: &[Vec<usize>], operation: usize, dependency: usize) -> bool {
    let mut pending = vec![operation];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(candidate) = pending.pop() {
        if !seen.insert(candidate) {
            continue;
        }
        for previous in predecessors.get(candidate).into_iter().flatten() {
            if *previous == dependency {
                return true;
            }
            pending.push(*previous);
        }
    }
    false
}

fn operation_reaches(edges: &[Vec<OpId>], source: OpId, sink: OpId) -> bool {
    let mut pending = vec![source];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(operation) = pending.pop() {
        if !seen.insert(operation) {
            continue;
        }
        for successor in edges.get(operation.0 as usize).into_iter().flatten() {
            if *successor == sink {
                return true;
            }
            pending.push(*successor);
        }
    }
    false
}

fn operations_are_mutually_exclusive(
    first: &EvaluationOp,
    second: &EvaluationOp,
    guards: &[EvaluationGuard],
) -> bool {
    let ExecutionCondition::Guarded(first_guards) = &first.execution else {
        return false;
    };
    let ExecutionCondition::Guarded(second_guards) = &second.execution else {
        return false;
    };
    first_guards.iter().any(|first| {
        second_guards.iter().any(|second| {
            let Some(first) = guards.get(first.0 as usize) else {
                return false;
            };
            let Some(second) = guards.get(second.0 as usize) else {
                return false;
            };
            first.owner == second.owner && first.polarity != second.polarity
        })
    })
}

fn simple_scalar_bound_ops(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
    evaluation: EvaluationId,
    expression: Span,
) -> Option<(Vec<TypeValueOp>, Vec<Span>)> {
    let tokens = expression_tokens(tree, text, token_spans, expression);
    let (operations, result, _) = lower_scalar_expression(&tokens, resolution, types, evaluation)?;
    // A syntactically explicit load is still not a resolved type value. Until
    // the operation-derived points-to model can prove its target, keep the
    // complete bound formation on the conservative compatibility path.
    if operations.iter().any(|operation| {
        matches!(
            operation,
            TypeValueOp::LoadScalar { .. } | TypeValueOp::StoreScalar { .. }
        )
    }) {
        return None;
    }
    if operations
        .iter()
        .filter(|operation| matches!(operation, TypeValueOp::CallScalar { .. }))
        .count()
        > 1
    {
        return None;
    }
    let result_inputs = scalar_result_declarations(&operations, &result);
    Some((operations, result_inputs))
}

fn expression_tokens<'a>(
    tree: &Tree,
    text: &'a str,
    token_spans: &[Span],
    expression: Span,
) -> Vec<(&'a str, Span, u16)> {
    let start = token_spans.partition_point(|span| span.lo < expression.lo);
    let end = token_spans.partition_point(|span| span.hi <= expression.hi);
    token_spans[start..end]
        .iter()
        .copied()
        .enumerate()
        .map(|(offset, span)| {
            let token = TokenId::new((start + offset) as u32);
            (
                tree.tokens().text(token, text).trim(),
                span,
                tree.tokens().kind(token),
            )
        })
        .collect()
}

fn scalar_result_declarations(operations: &[TypeValueOp], result: &ScalarOperand) -> Vec<Span> {
    let mut pending = vec![result.clone()];
    let mut seen = std::collections::BTreeSet::new();
    let mut declarations = std::collections::BTreeSet::new();
    while let Some(operand) = pending.pop() {
        match operand {
            ScalarOperand::Declaration { declaration, .. } => {
                declarations.insert(declaration);
            }
            ScalarOperand::Constant { .. } => {}
            ScalarOperand::Produced { expression } if seen.insert(expression) => {
                let Some(operation) = operations
                    .iter()
                    .rev()
                    .find(|operation| operation_span(operation) == expression)
                else {
                    continue;
                };
                if let TypeValueOp::ReadScalar { declaration, .. } = operation {
                    declarations.insert(*declaration);
                } else {
                    pending.extend(operation.scalar_inputs());
                }
            }
            ScalarOperand::Produced { .. } => {}
        }
    }
    declarations.into_iter().collect()
}

fn strip_outer_token_parentheses<'a>(
    mut tokens: &'a [(&'a str, Span, u16)],
) -> &'a [(&'a str, Span, u16)] {
    loop {
        if tokens.len() < 2 || tokens[0].0 != "(" || tokens[tokens.len() - 1].0 != ")" {
            return tokens;
        }
        let mut depth = 0u32;
        let mut closes_early = false;
        for (index, (token, _, _)) in tokens.iter().enumerate() {
            match *token {
                "(" => depth += 1,
                ")" => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 && index + 1 != tokens.len() {
                        closes_early = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if depth != 0 || closes_early {
            return tokens;
        }
        tokens = &tokens[1..tokens.len() - 1];
    }
}

fn direct_update_target(
    tokens: &[(&str, Span, u16)],
    resolution: &FunctionResolution,
) -> Option<(PlaceId, Span, Span)> {
    let tokens = strip_outer_token_parentheses(tokens);
    let tokens = if tokens.first()?.0 == "*" {
        let address = strip_outer_token_parentheses(&tokens[1..]);
        if address.first()?.0 != "&" {
            return None;
        }
        strip_outer_token_parentheses(&address[1..])
    } else {
        tokens
    };
    let [(name, occurrence, _)] = tokens else {
        return None;
    };
    let declaration = resolution
        .resolve_at(name, occurrence.lo)
        .filter(|declaration| declaration.kind == SymbolKind::Value)?;
    Some((PlaceId::from(declaration.id), declaration.span, *occurrence))
}

fn lower_scalar_expression(
    tokens: &[(&str, Span, u16)],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
    evaluation: EvaluationId,
) -> Option<(Vec<TypeValueOp>, ScalarOperand, Vec<Span>)> {
    let tokens = strip_outer_token_parentheses(tokens);
    if tokens.len() == 1 {
        let (spelling, occurrence, _) = tokens[0];
        if is_integer_literal(spelling) {
            return Some((
                Vec::new(),
                ScalarOperand::Constant {
                    spelling: spelling.to_owned(),
                    occurrence,
                },
                Vec::new(),
            ));
        }
        let declaration = resolution
            .resolve_at(spelling, occurrence.lo)
            .filter(|declaration| declaration.kind == SymbolKind::Value)?;
        if types.adjusted_is_array(declaration.span) {
            return Some((
                vec![TypeValueOp::DecayArray {
                    evaluation,
                    place: PlaceId::from(declaration.id),
                    declaration: declaration.span,
                    occurrence,
                }],
                ScalarOperand::Produced {
                    expression: occurrence,
                },
                vec![declaration.span],
            ));
        }
        return Some((
            vec![TypeValueOp::ReadScalar {
                evaluation,
                place: PlaceId::from(declaration.id),
                declaration: declaration.span,
                occurrence,
            }],
            ScalarOperand::Produced {
                expression: occurrence,
            },
            vec![declaration.span],
        ));
    }
    let direct_increment = if matches!(tokens.first()?.0, "++" | "--") {
        Some((&tokens[1..], ScalarWriteResult::PostWrite))
    } else if matches!(tokens.last()?.0, "++" | "--") {
        Some((&tokens[..tokens.len() - 1], ScalarWriteResult::PreWrite))
    } else {
        None
    };
    if let Some(((place, declaration, occurrence), result)) =
        direct_increment.and_then(|(target, result)| {
            direct_update_target(target, resolution).map(|target| (target, result))
        })
    {
        let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
        return Some((
            vec![
                TypeValueOp::ReadScalar {
                    evaluation,
                    place,
                    declaration,
                    occurrence,
                },
                TypeValueOp::WriteScalar {
                    evaluation,
                    place,
                    declaration,
                    occurrence,
                    span,
                    kind: ScalarWriteKind::Increment,
                    prior: Some(ScalarOperand::Produced {
                        expression: occurrence,
                    }),
                    assigned: None,
                    result,
                },
            ],
            ScalarOperand::Produced { expression: span },
            vec![declaration],
        ));
    }
    let projected_increment = if matches!(tokens.first()?.0, "++" | "--") {
        Some((&tokens[1..], tokens[0], ScalarWriteResult::PostWrite))
    } else if matches!(tokens.last()?.0, "++" | "--") {
        Some((
            &tokens[..tokens.len() - 1],
            tokens[tokens.len() - 1],
            ScalarWriteResult::PreWrite,
        ))
    } else {
        None
    }
    .filter(|(target, _, _)| {
        let target = strip_outer_token_parentheses(target);
        top_level_commas(target).is_empty()
            && top_level_conditional(target).is_none()
            && top_level_assignment(target).is_none()
            && top_level_binary(target).is_none()
            && (target.first().is_some_and(|token| token.0 == "*")
                || terminal_postfix_projection(target).is_some())
    });
    if let Some((target, operator, result)) = projected_increment {
        let (mut operations, base, access, inputs) =
            lower_projected_target(target, resolution, types, evaluation)?;
        let target_span = Span::new(target[0].1.lo, target[target.len() - 1].1.hi);
        let prior = ScalarOperand::Produced {
            expression: target_span,
        };
        operations.push(TypeValueOp::LoadScalar {
            evaluation,
            base: base.clone(),
            access: access.clone(),
            span: target_span,
        });
        operations.push(TypeValueOp::ConstantScalar {
            evaluation,
            value: ScalarOperand::Constant {
                spelling: "1".to_owned(),
                occurrence: operator.1,
            },
        });
        let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
        operations.push(TypeValueOp::ComputeScalar {
            evaluation,
            operator: if operator.0 == "++" { "+" } else { "-" }.to_owned(),
            operands: vec![
                prior.clone(),
                ScalarOperand::Produced {
                    expression: operator.1,
                },
            ],
            span,
        });
        operations.push(TypeValueOp::StoreScalar {
            evaluation,
            base,
            access,
            prior: Some(prior),
            assigned: ScalarOperand::Produced { expression: span },
            result,
            target: target_span,
            span,
        });
        return Some((
            operations,
            ScalarOperand::Produced { expression: span },
            inputs,
        ));
    }
    let commas = top_level_commas(tokens);
    if !commas.is_empty() {
        let mut boundaries = commas.into_iter();
        let first = boundaries.next().expect("non-empty comma list");
        let (mut operations, mut value, mut inputs) =
            lower_scalar_expression(&tokens[..first], resolution, types, evaluation)?;
        let mut start = first + 1;
        for end in boundaries.chain(std::iter::once(tokens.len())) {
            let (mut next_operations, next, next_inputs) =
                lower_scalar_expression(&tokens[start..end], resolution, types, evaluation)?;
            operations.append(&mut next_operations);
            for declaration in next_inputs {
                if !inputs.contains(&declaration) {
                    inputs.push(declaration);
                }
            }
            let span = Span::new(value.occurrence().lo, next.occurrence().hi);
            operations.push(TypeValueOp::SequenceScalar {
                evaluation,
                before: value,
                value: next,
                span,
            });
            value = ScalarOperand::Produced { expression: span };
            start = end + 1;
        }
        return Some((operations, value, inputs));
    }
    if let Some(operator) = top_level_assignment(tokens) {
        let target = strip_outer_token_parentheses(&tokens[..operator]);
        if let Some((place, declaration, occurrence)) = direct_update_target(target, resolution) {
            let (mut operations, assigned, mut inputs) =
                lower_scalar_expression(&tokens[operator + 1..], resolution, types, evaluation)?;
            let compound = tokens[operator].0 != "=";
            let prior = compound.then_some(ScalarOperand::Produced {
                expression: occurrence,
            });
            if compound {
                operations.insert(
                    0,
                    TypeValueOp::ReadScalar {
                        evaluation,
                        place,
                        declaration,
                        occurrence,
                    },
                );
                if !inputs.contains(&declaration) {
                    inputs.insert(0, declaration);
                }
            }
            let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
            operations.push(TypeValueOp::WriteScalar {
                evaluation,
                place,
                declaration,
                occurrence,
                span,
                kind: if compound {
                    ScalarWriteKind::CompoundAssign
                } else {
                    ScalarWriteKind::Assign
                },
                prior,
                assigned: Some(assigned),
                result: ScalarWriteResult::PostWrite,
            });
            return Some((
                operations,
                ScalarOperand::Produced { expression: span },
                inputs,
            ));
        }
        let projected_assignment = terminal_postfix_projection(target).is_some()
            || target.first().is_some_and(|token| token.0 == "*");
        if projected_assignment {
            let (mut operations, base, access, mut inputs) =
                lower_projected_target(target, resolution, types, evaluation)?;
            let target_span = Span::new(tokens[0].1.lo, tokens[operator - 1].1.hi);
            let compound = tokens[operator].0 != "=";
            let prior = compound.then_some(ScalarOperand::Produced {
                expression: target_span,
            });
            if compound {
                operations.push(TypeValueOp::LoadScalar {
                    evaluation,
                    base: base.clone(),
                    access: access.clone(),
                    span: target_span,
                });
            }
            let (mut assigned_operations, mut assigned, assigned_inputs) =
                lower_scalar_expression(&tokens[operator + 1..], resolution, types, evaluation)?;
            operations.append(&mut assigned_operations);
            for declaration in assigned_inputs {
                if !inputs.contains(&declaration) {
                    inputs.push(declaration);
                }
            }
            let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
            if compound {
                operations.push(TypeValueOp::ComputeScalar {
                    evaluation,
                    operator: tokens[operator].0.trim_end_matches('=').to_owned(),
                    operands: vec![prior.clone()?, assigned],
                    span,
                });
                assigned = ScalarOperand::Produced { expression: span };
            }
            operations.push(TypeValueOp::StoreScalar {
                evaluation,
                base,
                access,
                prior,
                assigned,
                result: ScalarWriteResult::PostWrite,
                target: target_span,
                span,
            });
            return Some((
                operations,
                ScalarOperand::Produced { expression: span },
                inputs,
            ));
        }
        return None;
    }
    if let Some((question, colon)) = top_level_conditional(tokens) {
        let (mut condition_ops, condition, mut inputs) =
            lower_scalar_expression(&tokens[..question], resolution, types, evaluation)?;
        let (mut true_ops, when_true, true_inputs) =
            lower_scalar_expression(&tokens[question + 1..colon], resolution, types, evaluation)?;
        let (mut false_ops, when_false, false_inputs) =
            lower_scalar_expression(&tokens[colon + 1..], resolution, types, evaluation)?;
        condition_ops.append(&mut true_ops);
        condition_ops.append(&mut false_ops);
        for declaration in true_inputs.into_iter().chain(false_inputs) {
            if !inputs.contains(&declaration) {
                inputs.push(declaration);
            }
        }
        let span = Span::new(condition.occurrence().lo, when_false.occurrence().hi);
        condition_ops.push(TypeValueOp::SelectScalar {
            evaluation,
            condition,
            when_true,
            when_false,
        });
        return Some((
            condition_ops,
            ScalarOperand::Produced { expression: span },
            inputs,
        ));
    }
    if let Some(callee) = scalar_call(tokens) {
        let mut operations = Vec::new();
        let mut arguments = Vec::new();
        let mut inputs = Vec::new();
        let mut depth = 0u32;
        let mut start = 2usize;
        for index in 2..tokens.len() {
            let token = tokens[index].0;
            let at_end = index + 1 == tokens.len();
            match token {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" if !at_end => depth = depth.saturating_sub(1),
                _ => {}
            }
            if (token == "," && depth == 0) || at_end {
                let argument = &tokens[start..index];
                if !argument.is_empty() {
                    let (mut argument_ops, result, argument_inputs) =
                        lower_scalar_expression(argument, resolution, types, evaluation)?;
                    operations.append(&mut argument_ops);
                    arguments.push(result);
                    for declaration in argument_inputs {
                        if !inputs.contains(&declaration) {
                            inputs.push(declaration);
                        }
                    }
                }
                start = index + 1;
            }
        }
        let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
        operations.push(TypeValueOp::CallScalar {
            evaluation,
            callee: callee.to_owned(),
            span,
            arguments,
        });
        return Some((
            operations,
            ScalarOperand::Produced { expression: span },
            inputs,
        ));
    }
    let Some(operator) = top_level_binary(tokens) else {
        if let Some(postfix) = terminal_postfix_projection(tokens) {
            match postfix {
                TerminalProjection::Field { operator, member } if tokens[operator].0 == "->" => {
                    let (mut operations, base, inputs) = lower_scalar_expression(
                        &tokens[..operator],
                        resolution,
                        types,
                        evaluation,
                    )?;
                    let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
                    operations.push(TypeValueOp::LoadScalar {
                        evaluation,
                        base: ProjectionBaseOperand::Value(base),
                        access: projected_field_access(
                            &tokens[..operator],
                            true,
                            member,
                            resolution,
                            types,
                        ),
                        span,
                    });
                    return Some((
                        operations,
                        ScalarOperand::Produced { expression: span },
                        inputs,
                    ));
                }
                TerminalProjection::Field { operator, member } if tokens[operator].0 == "." => {
                    let (mut operations, base, inputs) =
                        lower_projection_base(&tokens[..operator], resolution, types, evaluation)?;
                    let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
                    operations.push(TypeValueOp::LoadScalar {
                        evaluation,
                        base,
                        access: projected_field_access(
                            &tokens[..operator],
                            false,
                            member,
                            resolution,
                            types,
                        ),
                        span,
                    });
                    return Some((
                        operations,
                        ScalarOperand::Produced { expression: span },
                        inputs,
                    ));
                }
                TerminalProjection::Element { open } => {
                    let (mut operations, base, mut inputs) =
                        lower_element_base(&tokens[..open], resolution, types, evaluation)?;
                    let (mut index_operations, index, index_inputs) = lower_scalar_expression(
                        &tokens[open + 1..tokens.len() - 1],
                        resolution,
                        types,
                        evaluation,
                    )?;
                    operations.append(&mut index_operations);
                    for declaration in index_inputs {
                        if !inputs.contains(&declaration) {
                            inputs.push(declaration);
                        }
                    }
                    let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
                    operations.push(TypeValueOp::LoadScalar {
                        evaluation,
                        base,
                        access: ProjectedAccess::Element { index },
                        span,
                    });
                    return Some((
                        operations,
                        ScalarOperand::Produced { expression: span },
                        inputs,
                    ));
                }
                TerminalProjection::Field { .. } => return None,
            }
        }
        if tokens[0].0 == "&" {
            let target = strip_outer_token_parentheses(&tokens[1..]);
            let [(name, occurrence, _)] = target else {
                return None;
            };
            let declaration = resolution
                .resolve_at(name, occurrence.lo)
                .filter(|declaration| declaration.kind == SymbolKind::Value)?;
            let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
            return Some((
                vec![TypeValueOp::AddressOf {
                    evaluation,
                    place: PlaceId::from(declaration.id),
                    declaration: declaration.span,
                    occurrence: *occurrence,
                    span,
                }],
                ScalarOperand::Produced { expression: span },
                Vec::new(),
            ));
        }
        if tokens[0].0 == "*" {
            let (mut operations, address, inputs) =
                lower_scalar_expression(&tokens[1..], resolution, types, evaluation)?;
            let span = Span::new(tokens[0].1.lo, address.occurrence().hi);
            operations.push(TypeValueOp::LoadScalar {
                evaluation,
                base: ProjectionBaseOperand::Value(address),
                access: ProjectedAccess::Dereference,
                span,
            });
            return Some((
                operations,
                ScalarOperand::Produced { expression: span },
                inputs,
            ));
        }
        let operator = unary_operator(tokens)?;
        let (mut operations, operand, inputs) =
            lower_scalar_expression(&tokens[1..], resolution, types, evaluation)?;
        let span = Span::new(tokens[0].1.lo, operand.occurrence().hi);
        operations.push(TypeValueOp::UnaryScalar {
            evaluation,
            operator: operator.to_owned(),
            operand,
            span,
        });
        return Some((
            operations,
            ScalarOperand::Produced { expression: span },
            inputs,
        ));
    };
    let (mut left_ops, left, mut inputs) =
        lower_scalar_expression(&tokens[..operator], resolution, types, evaluation)?;
    let (mut right_ops, right, right_inputs) =
        lower_scalar_expression(&tokens[operator + 1..], resolution, types, evaluation)?;
    left_ops.append(&mut right_ops);
    for declaration in right_inputs {
        if !inputs.contains(&declaration) {
            inputs.push(declaration);
        }
    }
    let span = Span::new(left.occurrence().lo, right.occurrence().hi);
    if matches!(tokens[operator].0, "&&" | "||") {
        let operator_span = tokens[operator].1;
        let bypass = ScalarOperand::Produced {
            expression: operator_span,
        };
        left_ops.push(TypeValueOp::ConstantScalar {
            evaluation,
            value: ScalarOperand::Constant {
                spelling: if tokens[operator].0 == "&&" { "0" } else { "1" }.to_owned(),
                occurrence: operator_span,
            },
        });
        left_ops.push(TypeValueOp::ShortCircuitScalar {
            evaluation,
            operator: tokens[operator].0.to_owned(),
            left,
            right,
            bypass,
            span,
        });
    } else {
        left_ops.push(TypeValueOp::ComputeScalar {
            evaluation,
            operator: tokens[operator].0.to_owned(),
            operands: vec![left, right],
            span,
        });
    }
    Some((
        left_ops,
        ScalarOperand::Produced { expression: span },
        inputs,
    ))
}

enum TerminalProjection<'a> {
    Field { operator: usize, member: &'a str },
    Element { open: usize },
}

fn lower_element_base(
    tokens: &[(&str, Span, u16)],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
    evaluation: EvaluationId,
) -> Option<(Vec<TypeValueOp>, ProjectionBaseOperand, Vec<Span>)> {
    let direct = strip_outer_token_parentheses(tokens);
    if let [(name, occurrence, _)] = direct {
        let declaration = resolution
            .resolve_at(name, occurrence.lo)
            .filter(|declaration| declaration.kind == SymbolKind::Value)?;
        if types.adjusted_is_array(declaration.span) {
            return Some((
                Vec::new(),
                ProjectionBaseOperand::Place {
                    place: PlaceId::from(declaration.id),
                    declaration: declaration.span,
                    occurrence: *occurrence,
                },
                vec![declaration.span],
            ));
        }
    }
    if projected_expression_type(tokens, resolution, types)
        .is_some_and(|ty| types.type_is_array(ty))
    {
        let (operations, base, access, inputs) =
            lower_projected_target(tokens, resolution, types, evaluation)?;
        let span = Span::new(tokens.first()?.1.lo, tokens.last()?.1.hi);
        return Some((
            operations,
            ProjectionBaseOperand::Projected {
                base: Box::new(base),
                access,
                span,
            },
            inputs,
        ));
    }
    let (operations, value, inputs) =
        lower_scalar_expression(tokens, resolution, types, evaluation)?;
    Some((operations, ProjectionBaseOperand::Value(value), inputs))
}

/// Resolve the structural type of a simple member chain without evaluating it.
/// Unsupported bases return `None`; callers then retain the conservative
/// scalar-value path rather than guessing that a field is an array.
fn projected_expression_type(
    tokens: &[(&str, Span, u16)],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
) -> Option<crate::csource::semantic::types::TypeId> {
    let tokens = strip_outer_token_parentheses(tokens);
    if let [(name, occurrence, _)] = tokens {
        let declaration = resolution
            .resolve_at(name, occurrence.lo)
            .filter(|declaration| declaration.kind == SymbolKind::Value)?;
        return types.adjusted_type_of_declaration(declaration.span);
    }
    match terminal_postfix_projection(tokens)? {
        TerminalProjection::Field { operator, member } => {
            let base = projected_expression_type(&tokens[..operator], resolution, types)?;
            types.member_type(base, tokens[operator].0 == "->", member)
        }
        TerminalProjection::Element { open } => {
            let base = projected_expression_type(&tokens[..open], resolution, types)?;
            types.element_type(base)
        }
    }
}

fn projected_field_access(
    base: &[(&str, Span, u16)],
    through_pointer: bool,
    member: &str,
    resolution: &FunctionResolution,
    types: &FunctionTypes,
) -> ProjectedAccess {
    let overlapping_members = projected_expression_type(base, resolution, types)
        .and_then(|ty| types.record_kind(ty, through_pointer))
        == Some(crate::csource::semantic::types::RecordKind::Union);
    ProjectedAccess::Field {
        member: member.to_owned(),
        overlapping_members,
    }
}

fn lower_projected_target(
    tokens: &[(&str, Span, u16)],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
    evaluation: EvaluationId,
) -> Option<(
    Vec<TypeValueOp>,
    ProjectionBaseOperand,
    ProjectedAccess,
    Vec<Span>,
)> {
    let tokens = strip_outer_token_parentheses(tokens);
    if tokens.first().is_some_and(|token| token.0 == "*") {
        let (operations, address, inputs) =
            lower_scalar_expression(&tokens[1..], resolution, types, evaluation)?;
        return Some((
            operations,
            ProjectionBaseOperand::Value(address),
            ProjectedAccess::Dereference,
            inputs,
        ));
    }
    match terminal_postfix_projection(tokens)? {
        TerminalProjection::Field { operator, member } if tokens[operator].0 == "->" => {
            let (operations, base, inputs) =
                lower_scalar_expression(&tokens[..operator], resolution, types, evaluation)?;
            Some((
                operations,
                ProjectionBaseOperand::Value(base),
                projected_field_access(&tokens[..operator], true, member, resolution, types),
                inputs,
            ))
        }
        TerminalProjection::Field { operator, member } if tokens[operator].0 == "." => {
            let (operations, base, inputs) =
                lower_projection_base(&tokens[..operator], resolution, types, evaluation)?;
            Some((
                operations,
                base,
                projected_field_access(&tokens[..operator], false, member, resolution, types),
                inputs,
            ))
        }
        TerminalProjection::Element { open } => {
            let (mut operations, base, mut inputs) =
                lower_element_base(&tokens[..open], resolution, types, evaluation)?;
            let (mut index_operations, index, index_inputs) = lower_scalar_expression(
                &tokens[open + 1..tokens.len() - 1],
                resolution,
                types,
                evaluation,
            )?;
            operations.append(&mut index_operations);
            for declaration in index_inputs {
                if !inputs.contains(&declaration) {
                    inputs.push(declaration);
                }
            }
            Some((operations, base, ProjectedAccess::Element { index }, inputs))
        }
        TerminalProjection::Field { .. } => None,
    }
}

fn lower_projection_base(
    tokens: &[(&str, Span, u16)],
    resolution: &FunctionResolution,
    types: &FunctionTypes,
    evaluation: EvaluationId,
) -> Option<(Vec<TypeValueOp>, ProjectionBaseOperand, Vec<Span>)> {
    let tokens = strip_outer_token_parentheses(tokens);
    if let [(name, occurrence, _)] = tokens {
        let declaration = resolution
            .resolve_at(name, occurrence.lo)
            .filter(|declaration| declaration.kind == SymbolKind::Value)?;
        return Some((
            Vec::new(),
            ProjectionBaseOperand::Place {
                place: PlaceId::from(declaration.id),
                declaration: declaration.span,
                occurrence: *occurrence,
            },
            vec![declaration.span],
        ));
    }
    if tokens.first().is_some_and(|token| token.0 == "*") {
        let (operations, address, inputs) =
            lower_scalar_expression(&tokens[1..], resolution, types, evaluation)?;
        let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
        return Some((
            operations,
            ProjectionBaseOperand::Projected {
                base: Box::new(ProjectionBaseOperand::Value(address)),
                access: ProjectedAccess::Dereference,
                span,
            },
            inputs,
        ));
    }
    let TerminalProjection::Field { operator, member } = terminal_postfix_projection(tokens)?
    else {
        return None;
    };
    let span = Span::new(tokens[0].1.lo, tokens[tokens.len() - 1].1.hi);
    let (operations, base, inputs) = if tokens[operator].0 == "." {
        lower_projection_base(&tokens[..operator], resolution, types, evaluation)?
    } else if tokens[operator].0 == "->" {
        let (operations, value, inputs) =
            lower_scalar_expression(&tokens[..operator], resolution, types, evaluation)?;
        (operations, ProjectionBaseOperand::Value(value), inputs)
    } else {
        return None;
    };
    Some((
        operations,
        ProjectionBaseOperand::Projected {
            base: Box::new(base),
            access: projected_field_access(
                &tokens[..operator],
                tokens[operator].0 == "->",
                member,
                resolution,
                types,
            ),
            span,
        },
        inputs,
    ))
}

fn terminal_postfix_projection<'a>(
    tokens: &'a [(&'a str, Span, u16)],
) -> Option<TerminalProjection<'a>> {
    if tokens.len() >= 3 && matches!(tokens[tokens.len() - 2].0, "." | "->") {
        let operator = tokens.len() - 2;
        let mut depth = 0u32;
        for (index, token) in tokens[..operator].iter().enumerate() {
            match token.0 {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => {}
            }
            if depth == 0 && index + 1 == operator {
                return Some(TerminalProjection::Field {
                    operator,
                    member: tokens[tokens.len() - 1].0,
                });
            }
        }
    }
    if tokens.last()?.0 != "]" {
        return None;
    }
    let mut parentheses = 0u32;
    let mut braces = 0u32;
    let mut brackets = 0u32;
    let mut candidate = None;
    for (index, token) in tokens.iter().enumerate() {
        match token.0 {
            "(" => parentheses += 1,
            ")" => parentheses = parentheses.saturating_sub(1),
            "{" => braces += 1,
            "}" => braces = braces.saturating_sub(1),
            "[" if parentheses == 0 && braces == 0 && brackets == 0 => {
                candidate = Some(index);
                brackets = 1;
            }
            "[" => brackets += 1,
            "]" => brackets = brackets.saturating_sub(1),
            _ => {}
        }
    }
    (parentheses == 0 && braces == 0 && brackets == 0)
        .then_some(TerminalProjection::Element { open: candidate? })
}

fn top_level_commas(tokens: &[(&str, Span, u16)]) -> Vec<usize> {
    let mut depth = 0u32;
    let mut conditionals = 0u32;
    let mut commas = Vec::new();
    for (index, (token, _, _)) in tokens.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            "?" if depth == 0 => conditionals += 1,
            ":" if depth == 0 && conditionals > 0 => conditionals -= 1,
            "," if depth == 0 && conditionals == 0 => commas.push(index),
            _ => {}
        }
    }
    commas
}

fn top_level_conditional(tokens: &[(&str, Span, u16)]) -> Option<(usize, usize)> {
    let mut delimiter_depth = 0u32;
    let mut question = None;
    let mut nested_conditionals = 0u32;
    for (index, (token, _, _)) in tokens.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => delimiter_depth += 1,
            ")" | "]" | "}" => delimiter_depth = delimiter_depth.saturating_sub(1),
            "?" if delimiter_depth == 0 => {
                if question.is_none() {
                    question = Some(index);
                } else {
                    nested_conditionals += 1;
                }
            }
            ":" if delimiter_depth == 0 && question.is_some() => {
                if nested_conditionals == 0 {
                    return question.map(|question| (question, index));
                }
                nested_conditionals -= 1;
            }
            _ => {}
        }
    }
    None
}

fn scalar_call<'a>(tokens: &'a [(&str, Span, u16)]) -> Option<&'a str> {
    let tokens = strip_outer_token_parentheses(tokens);
    if tokens.len() < 3
        || tokens[1].0 != "("
        || tokens[tokens.len() - 1].0 != ")"
        || TokenKind::from_u16(tokens[0].2) != Some(TokenKind::Identifier)
    {
        return None;
    }
    let mut depth = 0u32;
    for (index, (token, _, _)) in tokens.iter().enumerate().skip(1) {
        match *token {
            "(" => depth += 1,
            ")" => {
                depth = depth.saturating_sub(1);
                if depth == 0 && index + 1 != tokens.len() {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(tokens[0].0)
}

fn unary_operator<'a>(tokens: &'a [(&str, Span, u16)]) -> Option<&'a str> {
    let tokens = strip_outer_token_parentheses(tokens);
    (tokens.len() > 1 && matches!(tokens[0].0, "+" | "-" | "!" | "~")).then_some(tokens[0].0)
}

/// The outer assignment operator, excluding assignments in `?:` arms.
///
/// Assignment is right associative, so the first top-level operator owns the
/// rest of the expression (`a = b = c`). The middle operand of `?:` is an
/// expression in its own right and is lowered after the conditional splits it.
fn top_level_assignment(tokens: &[(&str, Span, u16)]) -> Option<usize> {
    let mut delimiter_depth = 0u32;
    let mut conditional_depth = 0u32;
    for (index, (token, _, _)) in tokens.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => delimiter_depth += 1,
            ")" | "]" | "}" => delimiter_depth = delimiter_depth.saturating_sub(1),
            "?" if delimiter_depth == 0 => conditional_depth += 1,
            ":" if delimiter_depth == 0 && conditional_depth > 0 => conditional_depth -= 1,
            "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "<<=" | ">>=" | "&=" | "^=" | "|="
                if delimiter_depth == 0
                    && conditional_depth == 0
                    && index > 0
                    && index + 1 < tokens.len() =>
            {
                return Some(index);
            }
            _ => {}
        }
    }
    None
}

fn top_level_binary(tokens: &[(&str, Span, u16)]) -> Option<usize> {
    let mut depth = 0u32;
    let mut selected: Option<(usize, u8)> = None;
    for (index, (token, _, _)) in tokens.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth = depth.saturating_sub(1),
            _ if depth == 0 => {
                let precedence = match *token {
                    "||" => 0,
                    "&&" => 1,
                    "|" => 2,
                    "^" => 3,
                    "&" => 4,
                    "==" | "!=" => 5,
                    "<" | "<=" | ">" | ">=" => 6,
                    "<<" | ">>" => 7,
                    "+" | "-" => 8,
                    "*" | "/" | "%" => 9,
                    _ => continue,
                };
                if index == 0 || index + 1 == tokens.len() {
                    continue;
                }
                if selected.is_none_or(|(_, current)| precedence <= current) {
                    selected = Some((index, precedence));
                }
            }
            _ => {}
        }
    }
    selected.map(|(index, _)| index)
}

fn is_integer_literal(value: &str) -> bool {
    let digits = value.trim_end_matches(|character: char| character.is_ascii_alphabetic());
    !digits.is_empty()
        && digits
            .chars()
            .all(|character| character.is_ascii_hexdigit() || matches!(character, 'x' | 'X'))
}

fn guard_cfg_edge(cfg: &Cfg, condition: Span, polarity: GuardPolarity) -> Option<(u32, u32)> {
    let edge_kind = match polarity {
        GuardPolarity::WhenTrue => EdgeKind::True,
        GuardPolarity::WhenFalse => EdgeKind::False,
    };
    cfg.nodes()
        .iter()
        .enumerate()
        .filter(|(_, node)| {
            let span = node.span();
            node.kind() == NodeKind::Cond && span.lo <= condition.lo && condition.hi <= span.hi
        })
        .filter_map(|(index, node)| {
            let branch = NodeId::new(index as u32);
            let entry = cfg
                .successor_edges(branch)
                .iter()
                .find(|edge| edge.kind == edge_kind)?
                .dst;
            let span = node.span();
            Some((
                (span.hi.saturating_sub(span.lo), index),
                (branch.raw(), entry.raw()),
            ))
        })
        .min_by_key(|(key, _)| *key)
        .map(|(_, nodes)| nodes)
}

fn cfg_node_for_span(owners: &[(Span, u32)], span: Span, body_start: u32) -> Option<u32> {
    let placed = owners
        .iter()
        .find_map(|(owner, index)| (owner.lo <= span.lo && span.hi <= owner.hi).then_some(*index));
    placed.or_else(|| (span.lo < body_start).then_some(0))
}

fn starts_with_size_operator(tree: &Tree, text: &str, node: NodeId) -> bool {
    let Some((first, end)) = tree.arena().token_extent(node) else {
        return false;
    };
    first < end
        && matches!(
            tree.tokens().text(TokenId::new(first), text).trim(),
            "sizeof" | "_Alignof" | "alignof" | "__alignof__"
        )
}

fn lone_parenthesized_name(tree: &Tree, token_spans: &[Span], node: NodeId) -> Option<Span> {
    let arena = tree.arena();
    let operand = arena.children_iter(node).find(|child| {
        matches!(
            arena.tag(*child).and_then(NodeTag::from_u16),
            Some(NodeTag::ParenExpr | NodeTag::NameRef)
        )
    })?;
    let names = arena
        .preorder(operand)
        .filter(|inner| arena.tag(*inner) == Some(NodeTag::NameRef.as_u16()))
        .collect::<Vec<_>>();
    if names.len() != 1
        || arena.preorder(operand).any(|inner| {
            !matches!(
                arena.tag(inner).and_then(NodeTag::from_u16),
                Some(NodeTag::ParenExpr | NodeTag::NameRef)
            )
        })
    {
        return None;
    }
    arena.span(names[0], token_spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::cfg::function_cfgs;
    use crate::csource::parse::parse;
    use crate::csource::semantic::AnalysisUnit;

    #[test]
    fn semantic_guard_polarity_resolves_to_cfg_arm_edges() {
        let source = "int f(int c, int n, int m) { int x = c ? n : m; return x; }";
        let tree = parse(source).into_parts().0;
        let functions = function_cfgs(&tree, source).into_parts().0;
        let cfg = &functions[0].cfg;
        let offset = source.rfind("c ?").expect("condition occurrence") as u32;
        let condition = Span::new(offset, offset + 1);
        let truthy = guard_cfg_edge(cfg, condition, GuardPolarity::WhenTrue).expect("true CFG arm");
        let falsy =
            guard_cfg_edge(cfg, condition, GuardPolarity::WhenFalse).expect("false CFG arm");
        assert_eq!(truthy.0, falsy.0);
        assert_ne!(truthy.1, falsy.1);
    }

    #[test]
    fn initializers_and_returns_are_distinct_owned_evaluation_roots() {
        let source = "int f(int n) { int a[n]; int x = n + 1; return x * 2; }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let roots = operations
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
            .collect::<Vec<_>>();
        assert_eq!(roots.len(), 2);

        let TypeValueOp::FinishExpression {
            evaluation: initializer_evaluation,
            purpose: EvaluationPurpose::Initialize { declaration, .. },
            expression: initializer_expression,
            ..
        } = roots[0].kind
        else {
            panic!("first ordinary root must initialize x")
        };
        let TypeValueOp::FinishExpression {
            evaluation: return_evaluation,
            purpose: EvaluationPurpose::Return { statement },
            expression: return_expression,
            ..
        } = roots[1].kind
        else {
            panic!("second ordinary root must return a value")
        };
        assert_eq!(&source[declaration.range()], "x");
        assert_eq!(&source[initializer_expression.range()], "n + 1");
        assert_eq!(&source[return_expression.range()], "x * 2");
        assert_eq!(&source[statement.range()], "return x * 2;");
        assert_eq!(initializer_evaluation, EvaluationId(1));
        assert_eq!(return_evaluation, EvaluationId(2));
        assert_eq!(roots[0].inputs.len(), 1);
        assert_eq!(roots[1].inputs.len(), 1);
        for root in roots {
            let producer = operations
                .iter()
                .find(|candidate| candidate.output == root.inputs[0])
                .expect("completed root consumes its expression producer");
            assert_eq!(producer.evaluation, root.evaluation);
            assert!(producer.id.0 < root.id.0);
            assert_eq!(producer.cfg_node, root.cfg_node);
        }
    }

    #[test]
    fn expression_statements_are_owned_discarded_evaluation_roots() {
        let source = "int f(int n) { n + 1; n++; return n; }";
        let unit = AnalysisUnit::new(source);
        let finishes = unit.evaluations()[0]
            .operations()
            .iter()
            .filter_map(|operation| match operation.kind {
                TypeValueOp::FinishExpression { purpose, .. } => Some((purpose, operation)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(finishes.len(), 3);
        assert!(matches!(
            finishes[0].0,
            EvaluationPurpose::Discard { statement } if &source[statement.range()] == "n + 1;"
        ));
        assert!(matches!(
            finishes[1].0,
            EvaluationPurpose::Discard { statement } if &source[statement.range()] == "n++;"
        ));
        assert!(matches!(finishes[2].0, EvaluationPurpose::Return { .. }));
        assert!(finishes.iter().all(|(_, finish)| finish.inputs.len() == 1));
    }

    #[test]
    fn control_conditions_are_exact_owned_evaluation_roots() {
        let source = concat!(
            "int f(int a,int b,int c,int d,int e){",
            "if(a){} while(b){} do{}while(c); ",
            "for(a=0;d;a++){} switch(e){default:return 0;}}",
        );
        let unit = AnalysisUnit::new(source);
        let mut controls = unit.evaluations()[0]
            .operations()
            .iter()
            .filter_map(|operation| match operation.kind {
                TypeValueOp::FinishExpression {
                    purpose: EvaluationPurpose::Control { statement, kind },
                    expression,
                    ..
                } => Some((expression, statement, kind, operation)),
                _ => None,
            })
            .collect::<Vec<_>>();
        controls.sort_by_key(|(expression, _, _, _)| expression.lo);

        assert_eq!(controls.len(), 5);
        let expected = [
            ("a", "if", ControlConditionKind::If),
            ("b", "while", ControlConditionKind::While),
            ("c", "do", ControlConditionKind::DoWhile),
            ("d", "for", ControlConditionKind::For),
            ("e", "switch", ControlConditionKind::Switch),
        ];
        for ((expression, statement, kind, finish), (text, prefix, expected_kind)) in
            controls.into_iter().zip(expected)
        {
            assert_eq!(&source[expression.range()], text);
            assert!(source[statement.range()].starts_with(prefix));
            assert_eq!(kind, expected_kind);
            assert_eq!(finish.inputs.len(), 1);
            let producer = unit.evaluations()[0]
                .operations()
                .iter()
                .find(|candidate| candidate.output == finish.inputs[0])
                .expect("control root consumes its condition producer");
            assert_eq!(producer.evaluation, finish.evaluation);
            assert_eq!(producer.cfg_node, finish.cfg_node);
        }
    }

    #[test]
    fn expression_for_clauses_are_distinct_owned_evaluation_roots() {
        let source = "int f(int n){for(n=0;n<3;n++){continue;} return n;}";
        let unit = AnalysisUnit::new(source);
        let clauses = unit.evaluations()[0]
            .operations()
            .iter()
            .filter_map(|operation| match operation.kind {
                TypeValueOp::FinishExpression {
                    purpose: EvaluationPurpose::ForClause { statement, phase },
                    expression,
                    ..
                } => Some((statement, phase, expression, operation)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(clauses.len(), 2);
        assert_eq!(clauses[0].1, ForClausePhase::Init);
        assert_eq!(&source[clauses[0].2.range()], "n=0");
        assert_eq!(clauses[1].1, ForClausePhase::Step);
        assert_eq!(&source[clauses[1].2.range()], "n++");
        for (statement, _, _, finish) in clauses {
            assert!(source[statement.range()].starts_with("for"));
            assert_eq!(finish.inputs.len(), 1);
            let producer = unit.evaluations()[0]
                .operations()
                .iter()
                .find(|candidate| candidate.output == finish.inputs[0])
                .expect("for clause consumes its expression producer");
            assert_eq!(producer.evaluation, finish.evaluation);
            assert_eq!(producer.cfg_node, finish.cfg_node);
        }

        let declared = AnalysisUnit::new("int f(void){for(int i=0;i<3;i++){} return 0;}");
        let purposes = declared.evaluations()[0]
            .operations()
            .iter()
            .filter_map(|operation| match operation.kind {
                TypeValueOp::FinishExpression { purpose, .. } => Some(purpose),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(purposes.iter().any(|purpose| matches!(
            purpose,
            EvaluationPurpose::Initialize { declaration, .. }
                if &declared.source()[declaration.range()] == "i"
        )));
        assert!(!purposes.iter().any(|purpose| matches!(
            purpose,
            EvaluationPurpose::ForClause {
                phase: ForClausePhase::Init,
                ..
            }
        )));
        assert!(purposes.iter().any(|purpose| matches!(
            purpose,
            EvaluationPurpose::ForClause {
                phase: ForClausePhase::Step,
                ..
            }
        )));
    }

    #[test]
    fn computed_goto_owns_its_operand_without_modeling_the_marker_as_a_load() {
        let source = "int f(void *p){goto *pick(p);}";
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
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
            .expect("computed-goto operand root");
        let TypeValueOp::FinishExpression {
            expression,
            purpose: EvaluationPurpose::IndirectDispatch { statement },
            ..
        } = finish.kind
        else {
            unreachable!("selected operation is the dispatch consumer")
        };
        assert_eq!(&source[expression.range()], "pick(p)");
        assert_eq!(&source[statement.range()], "goto *pick(p);");
        assert_eq!(finish.inputs.len(), 1);
        assert!(plan.operations().iter().any(|operation| matches!(
            &operation.kind,
            TypeValueOp::CallScalar { callee, span, .. }
                if callee == "pick" && *span == expression
        )));
        assert!(plan.operations().iter().all(|operation| !matches!(
            &operation.kind,
            TypeValueOp::UnaryScalar { operator, .. } if operator == "*"
        )));

        let direct = AnalysisUnit::new("int f(void){goto done; done:return 0;}");
        assert!(direct.evaluations()[0]
            .operations()
            .iter()
            .all(|operation| {
                !matches!(
                    operation.kind,
                    TypeValueOp::FinishExpression {
                        purpose: EvaluationPurpose::IndirectDispatch { .. },
                        ..
                    }
                )
            }));
    }

    #[test]
    fn one_owned_plan_connects_bound_formation_to_sizeof_consumption() {
        let source = "int f(int n) { int a[n]; typeof(a) b; return sizeof(b); }";
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let formed = plan
            .operations()
            .iter()
            .find_map(|operation| match &operation.kind {
                TypeValueOp::FormBound {
                    slot,
                    expression,
                    inputs,
                    ..
                } => Some((operation.id, operation.cfg_node, *slot, *expression, inputs)),
                _ => None,
            })
            .expect("bound formation");
        let read = plan
            .operations()
            .iter()
            .find_map(|operation| match &operation.kind {
                TypeValueOp::ReadBound { slot, consumer } => {
                    Some((operation.id, operation.cfg_node, *slot, *consumer))
                }
                _ => None,
            })
            .expect("bound consumption");
        assert_eq!(formed.0, OpId(1));
        assert_eq!(read.0, OpId(2));
        assert!(formed.1 > 0 && read.1 > formed.1);
        assert_eq!(formed.2, read.2);
        assert_eq!(&source[formed.3.range()], "n");
        assert_eq!(&source[read.3.range()], "b");
        assert_eq!(formed.4.len(), 1);
        assert_eq!(&source[formed.4[0].range()], "n");
    }

    #[test]
    fn side_effecting_bound_orders_read_write_then_capture() {
        let source = "int f(int n) { int a[n++]; return n + sizeof(a); }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        assert!(matches!(
            operations[0],
            EvaluationOp {
                id: OpId(0),
                kind: TypeValueOp::ReadScalar { .. },
                ..
            }
        ));
        assert!(matches!(
            operations[1],
            EvaluationOp {
                id: OpId(1),
                kind: TypeValueOp::WriteScalar {
                    kind: ScalarWriteKind::Increment,
                    ..
                },
                ..
            }
        ));
        assert!(matches!(
            operations[2],
            EvaluationOp {
                id: OpId(2),
                kind: TypeValueOp::FormBound { .. },
                ..
            }
        ));
        assert_eq!(operations[0].cfg_node, operations[1].cfg_node);
        assert_eq!(operations[1].cfg_node, operations[2].cfg_node);
        assert_eq!(operations[1].inputs, [operations[0].output]);
        assert_eq!(operations[2].inputs, [operations[0].output]);

        let prefix = AnalysisUnit::new("int f(int n) { int a[++n]; return sizeof(a); }");
        let prefix = prefix.evaluations()[0].operations();
        assert!(matches!(
            prefix[1].kind,
            TypeValueOp::WriteScalar {
                result: ScalarWriteResult::PostWrite,
                ..
            }
        ));
        assert_eq!(prefix[2].inputs, [prefix[1].output]);

        let compound = AnalysisUnit::new("int f(int n) { int a[n += 1]; return sizeof(a); }");
        let compound = compound.evaluations()[0].operations();
        let compound_write = compound
            .iter()
            .find(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::WriteScalar {
                        kind: ScalarWriteKind::CompoundAssign,
                        ..
                    }
                )
            })
            .expect("compound write");
        assert!(matches!(
            compound_write.kind,
            TypeValueOp::WriteScalar {
                kind: ScalarWriteKind::CompoundAssign,
                ..
            }
        ));
        assert_eq!(compound_write.inputs.len(), 2);
        let formation = compound
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("compound bound formation");
        assert_eq!(formation.inputs, [compound_write.output]);

        let assigned = AnalysisUnit::new("int f(int n) { int a[n = 4]; return sizeof(a); }");
        let assigned = assigned.evaluations()[0].operations();
        let assigned_write = assigned
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
            .expect("assignment write");
        assert_eq!(assigned_write.inputs.len(), 1);
        assert!(matches!(
            assigned_write.kind,
            TypeValueOp::WriteScalar {
                kind: ScalarWriteKind::Assign,
                assigned: Some(ScalarOperand::Constant { .. }),
                result: ScalarWriteResult::PostWrite,
                ..
            }
        ));
        let formation = assigned
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("assignment bound formation");
        assert_eq!(formation.inputs, [assigned_write.output]);
    }

    #[test]
    fn comma_bound_separates_executed_effects_from_result_inputs() {
        let source = "int f(int n, int m) { int a[(n++, m)]; return sizeof(a); }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        assert_eq!(operations.len(), 6);
        assert!(matches!(operations[0].kind, TypeValueOp::ReadScalar { .. }));
        assert!(matches!(
            operations[1].kind,
            TypeValueOp::WriteScalar { .. }
        ));
        assert!(matches!(operations[2].kind, TypeValueOp::ReadScalar { .. }));
        assert!(matches!(
            operations[3].kind,
            TypeValueOp::SequenceScalar { .. }
        ));
        let TypeValueOp::FormBound { inputs, .. } = &operations[4].kind else {
            panic!("fifth operation captures the comma result")
        };
        assert_eq!(inputs.len(), 1);
        assert_eq!(&source[inputs[0].range()], "m");
        assert!(matches!(operations[5].kind, TypeValueOp::ReadBound { .. }));
    }

    #[test]
    fn conditional_bound_is_one_selection_with_explicit_alternatives() {
        let source = "int f(int c, int n, int m) { int a[c ? n : m]; return sizeof(a); }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let select = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::SelectScalar { .. }))
            .expect("conditional selection");
        let TypeValueOp::SelectScalar {
            condition,
            when_true,
            when_false,
            ..
        } = &select.kind
        else {
            unreachable!("selected operation is a conditional")
        };
        assert_eq!(&source[condition.occurrence().range()], "c");
        assert_eq!(&source[when_true.occurrence().range()], "n");
        assert_eq!(&source[when_false.occurrence().range()], "m");
        assert_eq!(select.inputs.len(), 3);
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("selection feeds bound formation");
        let TypeValueOp::FormBound { .. } = &formation.kind else {
            panic!("selection feeds bound formation")
        };
        assert_eq!(formation.inputs, [select.output]);
    }

    #[test]
    fn conditional_arms_are_value_subgraphs_not_unconditional_effects() {
        let source = concat!(
            "int f(int c, int n, int m) { int a[c ? (n + 1) : +(m * 2)]; ",
            "return sizeof(a); }",
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let operations = plan.operations();
        let computes = operations
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .collect::<Vec<_>>();
        let unary = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::UnaryScalar { .. }))
            .expect("false-arm unary result");
        let select = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::SelectScalar { .. }))
            .expect("selection");
        assert_eq!(computes.len(), 2);
        assert_eq!(unary.inputs, [computes[1].output]);
        assert!(select.inputs.contains(&computes[0].output));
        assert!(select.inputs.contains(&unary.output));
        assert_eq!(plan.guards.len(), 2);
        assert_eq!(plan.guards[0].owner, select.id);
        assert_eq!(plan.guards[0].condition, select.inputs[0]);
        assert_eq!(plan.guards[0].polarity, GuardPolarity::WhenTrue);
        assert_eq!(plan.guards[1].polarity, GuardPolarity::WhenFalse);
        let GuardControl::CfgEdge {
            branch_node: true_branch,
            entry_node: true_entry,
        } = plan.guards[0].control
        else {
            panic!("true arm must resolve to its CFG edge")
        };
        let GuardControl::CfgEdge {
            branch_node: false_branch,
            entry_node: false_entry,
        } = plan.guards[1].control
        else {
            panic!("false arm must resolve to its CFG edge")
        };
        assert_eq!(true_branch, false_branch);
        assert_ne!(true_entry, false_entry);
        assert_eq!(
            computes[0].execution,
            ExecutionCondition::Guarded(vec![plan.guards[0].id])
        );
        assert_eq!(
            computes[1].execution,
            ExecutionCondition::Guarded(vec![plan.guards[1].id])
        );
        assert_eq!(
            unary.execution,
            ExecutionCondition::Guarded(vec![plan.guards[1].id])
        );
        assert_eq!(select.execution, ExecutionCondition::Unconditional);
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        assert_eq!(formation.inputs, [select.output]);

        let effecting = AnalysisUnit::new(
            "int f(int c, int n, int m) { int a[c ? n++ : m]; return sizeof(a); }",
        );
        let effecting_plan = &effecting.evaluations()[0];
        assert!(effecting_plan
            .operations()
            .iter()
            .any(|operation| matches!(operation.kind, TypeValueOp::SelectScalar { .. })));
        let guarded_write = effecting_plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
            .expect("true-arm increment");
        let ExecutionCondition::Guarded(write_guards) = &guarded_write.execution else {
            panic!("conditional write must not become unconditional")
        };
        assert_eq!(write_guards.len(), 1);
        assert!(matches!(
            effecting_plan.guards[write_guards[0].0 as usize].control,
            GuardControl::CfgEdge { .. }
        ));
    }

    #[test]
    fn guarded_compound_assignment_names_prior_and_rhs_values_explicitly() {
        let source = concat!(
            "int f(int c, int n, int m) { int a[c ? (n += m + 1) : n]; ",
            "return sizeof(a); }",
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let compute = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .expect("computed right-hand side");
        let write = plan
            .operations()
            .iter()
            .find(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::WriteScalar {
                        kind: ScalarWriteKind::CompoundAssign,
                        ..
                    }
                )
            })
            .expect("compound write");
        let read = plan
            .operations()
            .iter()
            .find(|operation| {
                matches!(operation.kind, TypeValueOp::ReadScalar { .. })
                    && write.inputs.contains(&operation.output)
            })
            .expect("compound assignment reads its old target");
        assert_eq!(write.inputs, [read.output, compute.output]);
        assert!(read.id.0 < compute.id.0 && compute.id.0 < write.id.0);
        let ExecutionCondition::Guarded(guards) = &write.execution else {
            panic!("true-arm write must be guarded")
        };
        assert_eq!(guards.len(), 1);
        assert_eq!(
            plan.guards[guards[0].0 as usize].polarity,
            GuardPolarity::WhenTrue
        );
        assert!(matches!(
            plan.guards[guards[0].0 as usize].control,
            GuardControl::CfgEdge { .. }
        ));
        let select = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::SelectScalar { .. }))
            .expect("conditional join");
        assert!(select.inputs.contains(&write.output));
    }

    #[test]
    fn call_bound_retains_argument_dependencies_before_capture() {
        let source = concat!(
            "int opaque(int, int); int f(int n) { int values[opaque(n, 4)]; ",
            "return sizeof(values); }",
        );
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        assert_eq!(operations.len(), 4);
        let call = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
            .expect("call operation");
        let TypeValueOp::CallScalar {
            evaluation,
            callee,
            arguments,
            ..
        } = &call.kind
        else {
            panic!("first operation evaluates the call")
        };
        assert_eq!(callee, "opaque");
        assert_eq!(arguments.len(), 2);
        assert_eq!(&source[arguments[0].occurrence().range()], "n");
        assert_eq!(&source[arguments[1].occurrence().range()], "4");
        assert!(matches!(arguments[1], ScalarOperand::Constant { .. }));
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        let TypeValueOp::FormBound { slot: formed, .. } = &formation.kind else {
            panic!("second operation captures the call result")
        };
        assert_eq!(*evaluation, EvaluationId(formed.0));
        assert_eq!(formation.inputs, [call.output]);
        assert_eq!(call.inputs.len(), 2);
        assert!(matches!(
            operations[3].kind,
            TypeValueOp::ReadBound { slot: read, .. } if read == *formed
        ));
    }

    #[test]
    fn nested_call_uses_producer_values_but_two_calls_remain_unlowered() {
        let source = concat!(
            "int opaque(int); int f(int n) { int values[opaque(n + 1) * 2]; ",
            "return sizeof(values); }",
        );
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let computes = operations
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .collect::<Vec<_>>();
        let call = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
            .expect("nested call");
        assert_eq!(computes.len(), 2);
        assert_eq!(call.inputs, [computes[0].output]);
        assert!(computes[1].inputs.contains(&call.output));
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        assert_eq!(formation.inputs, [computes[1].output]);

        let two_calls = AnalysisUnit::new(concat!(
            "int a(int); int b(int); int f(int n) { ",
            "int values[a(n) + b(n)]; return sizeof(values); }",
        ));
        assert!(!two_calls.evaluations()[0]
            .operations()
            .iter()
            .any(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. })));
    }

    #[test]
    fn ordinary_effects_record_language_order_without_inventing_source_order() {
        let siblings =
            AnalysisUnit::new("int a(int); int b(int); int f(int n) { return a(n) + b(n); }");
        let plan = &siblings.evaluations()[0];
        let calls = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
            .map(|operation| operation.id)
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert!(plan.order().iter().any(|order| {
            order.first == calls[0]
                && order.second == calls[1]
                && order.kind == EvaluationOrderKind::IndeterminatelySequenced
        }));

        let nested = AnalysisUnit::new("int a(int); int b(int); int f(int n) { return a(b(n)); }");
        let plan = &nested.evaluations()[0];
        let calls = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
            .map(|operation| operation.id)
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert!(plan.order().iter().any(|order| {
            order.first == calls[0]
                && order.second == calls[1]
                && order.kind == EvaluationOrderKind::SequencedBefore
        }));

        let alternatives = AnalysisUnit::new(
            "int a(int); int b(int); int f(int c, int n) { return c ? a(n) : b(n); }",
        );
        let plan = &alternatives.evaluations()[0];
        assert!(plan
            .order()
            .iter()
            .any(|order| order.kind == EvaluationOrderKind::MutuallyExclusive));

        for (source, call_before_write) in [
            ("int a(int); int f(int n) { return n = a(n); }", true),
            ("int a(int); int f(int n) { return a(n = 1); }", false),
        ] {
            let ordered = AnalysisUnit::new(source);
            let plan = &ordered.evaluations()[0];
            let call = plan
                .operations()
                .iter()
                .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
                .expect("ordered call");
            let write = plan
                .operations()
                .iter()
                .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
                .expect("ordered write");
            assert!(plan.order().iter().any(|order| {
                order.kind == EvaluationOrderKind::SequencedBefore
                    && if call_before_write {
                        order.first == call.id && order.second == write.id
                    } else {
                        order.first == write.id && order.second == call.id
                    }
            }));
        }

        for source in [
            "int a(int); int f(int c, int n) { return c ? a(n) : (n = 1); }",
            "int a(int); int f(int c, int n) { return c ? (n = 1) : a(n); }",
        ] {
            let alternative = AnalysisUnit::new(source);
            let plan = &alternative.evaluations()[0];
            let call = plan
                .operations()
                .iter()
                .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
                .expect("alternative call");
            let write = plan
                .operations()
                .iter()
                .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
                .expect("alternative write");
            assert!(plan.order().iter().any(|order| {
                order.kind == EvaluationOrderKind::MutuallyExclusive
                    && ((order.first == call.id && order.second == write.id)
                        || (order.first == write.id && order.second == call.id))
            }));
        }

        let mixed = AnalysisUnit::new("int a(int); int f(int n) { return a(n) + (n = 1); }");
        assert!(!mixed.evaluations()[0]
            .operations()
            .iter()
            .any(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. })));

        for source in [
            "int f(int n) { return n++ + n; }",
            "int f(int n) { return n++ + n++; }",
        ] {
            let unsequenced = AnalysisUnit::new(source);
            assert!(!unsequenced.evaluations()[0]
                .operations()
                .iter()
                .any(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. })));
        }
        for source in [
            "int f(int n) { return n++ && n; }",
            "int f(int c, int n) { return c ? n++ : n; }",
        ] {
            let sequenced = AnalysisUnit::new(source);
            assert!(sequenced.evaluations()[0]
                .operations()
                .iter()
                .any(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. })));
        }
    }

    #[test]
    fn ordinary_comma_executes_left_but_returns_only_the_right_value() {
        let unit = AnalysisUnit::new("int f(int n) { return (n++, n); }");
        let plan = &unit.evaluations()[0];
        let sequence = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::SequenceScalar { .. }))
            .expect("comma sequence");
        let write = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
            .expect("discarded increment");
        let right = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::ReadScalar { .. }))
            .find(|operation| operation.id.0 > write.id.0)
            .expect("right comma value");
        assert_eq!(sequence.inputs, [right.output]);
        assert!(plan.order().iter().any(|order| {
            order.first == write.id
                && order.second == right.id
                && order.kind == EvaluationOrderKind::SequencedBefore
        }));
        let finish = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
            .expect("return consumer");
        assert_eq!(finish.inputs, [sequence.output]);
    }

    #[test]
    fn binary_bound_is_one_explicit_computation() {
        let source = "int f(int n, int m) { int values[n + m]; return sizeof(values); }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        assert_eq!(operations.len(), 5);
        let compute = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .expect("binary computation");
        let TypeValueOp::ComputeScalar {
            evaluation,
            operator,
            operands,
            ..
        } = &compute.kind
        else {
            panic!("first operation computes the binary bound")
        };
        assert_eq!(operator, "+");
        assert_eq!(operands.len(), 2);
        assert_eq!(&source[operands[0].occurrence().range()], "n");
        assert_eq!(&source[operands[1].occurrence().range()], "m");
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        let TypeValueOp::FormBound { slot: formed, .. } = &formation.kind else {
            panic!("second operation captures the computed result")
        };
        assert_eq!(*evaluation, EvaluationId(formed.0));
        assert_eq!(formation.inputs, [compute.output]);
        assert!(matches!(
            operations[4].kind,
            TypeValueOp::ReadBound { slot: read, .. } if read == *formed
        ));
        assert_eq!(compute.inputs, [operations[0].output, operations[1].output]);
        assert_eq!(formation.inputs, [compute.output]);
        assert_eq!(operations[4].inputs, [formation.output]);
        assert_eq!(unit.evaluations()[0].values.len(), 5);
        for (index, value) in unit.evaluations()[0].values.iter().enumerate() {
            assert_eq!(value.id, ValueId(index as u32));
        }
    }

    #[test]
    fn operation_inputs_preserve_operand_order_and_multiplicity() {
        let source =
            "int f(int n, int m) { int a[m - n]; int b[n - n]; return sizeof(a) + sizeof(b); }";
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let computes = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .collect::<Vec<_>>();
        assert_eq!(computes.len(), 2);

        let first = plan.scalar_inputs_of(computes[0]);
        assert_eq!(first.len(), 2);
        assert_eq!(&source[first[0].occurrence().range()], "m");
        assert_eq!(&source[first[1].occurrence().range()], "n");

        let repeated = plan.scalar_inputs_of(computes[1]);
        assert_eq!(repeated.len(), 2);
        assert_eq!(&source[repeated[0].occurrence().range()], "n");
        assert_eq!(&source[repeated[1].occurrence().range()], "n");
        assert_ne!(repeated[0].occurrence(), repeated[1].occurrence());
        assert_ne!(computes[1].inputs[0], computes[1].inputs[1]);
    }

    #[test]
    fn nested_binary_bound_is_a_topological_value_graph() {
        let source = concat!(
            "int f(int n, int m) { int values[((n + 3) * (m - 1)) / 2]; ",
            "return sizeof(values); }",
        );
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let computes = operations
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .collect::<Vec<_>>();
        assert_eq!(computes.len(), 4);
        assert_eq!(computes[2].inputs, [computes[0].output, computes[1].output]);
        assert!(computes[3].inputs.contains(&computes[2].output));
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        assert_eq!(formation.inputs, [computes[3].output]);
        assert!(operations.iter().all(|operation| {
            operation
                .inputs
                .iter()
                .all(|input| input.0 < operation.output.0)
        }));
    }

    #[test]
    fn unary_bound_consumes_the_nested_operand_result() {
        let source = "int f(int n) { int values[+(n + 1)]; return sizeof(values); }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let compute = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .expect("nested computation");
        let unary = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::UnaryScalar { .. }))
            .expect("unary computation");
        assert_eq!(unary.inputs, [compute.output]);
        let TypeValueOp::UnaryScalar { operator, .. } = &unary.kind else {
            unreachable!()
        };
        assert_eq!(operator, "+");
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        assert_eq!(formation.inputs, [unary.output]);
    }

    #[test]
    fn short_circuit_bound_has_a_conditional_right_input() {
        let source = "int f(int n, int m) { int values[n && (m + 1)]; return sizeof(values); }";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let compute = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
            .expect("right-hand computation");
        let short = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ShortCircuitScalar { .. }))
            .expect("short-circuit operation");
        let TypeValueOp::ShortCircuitScalar { operator, .. } = &short.kind else {
            unreachable!()
        };
        assert_eq!(operator, "&&");
        assert_eq!(short.inputs[1], compute.output);
        let TypeValueOp::ShortCircuitScalar { bypass, .. } = &short.kind else {
            unreachable!()
        };
        let ScalarOperand::Produced { expression } = bypass else {
            panic!("short circuit bypass must be an explicit produced value")
        };
        let bypass_op = operations
            .iter()
            .find(|operation| {
                matches!(&operation.kind, TypeValueOp::ConstantScalar { value, .. }
                    if value.occurrence() == *expression)
            })
            .expect("language-defined false bypass");
        assert!(matches!(
            &bypass_op.kind,
            TypeValueOp::ConstantScalar {
                value: ScalarOperand::Constant { spelling, .. },
                ..
            } if spelling == "0"
        ));
        assert!(short.inputs.contains(&bypass_op.output));
        assert_eq!(unit.evaluations()[0].guards.len(), 1);
        let guard = &unit.evaluations()[0].guards[0];
        assert_eq!(guard.owner, short.id);
        assert_eq!(guard.condition, short.inputs[0]);
        assert_eq!(guard.polarity, GuardPolarity::WhenTrue);
        assert!(matches!(guard.control, GuardControl::CfgEdge { .. }));
        assert_eq!(
            compute.execution,
            ExecutionCondition::Guarded(vec![guard.id])
        );
        assert_eq!(short.execution, ExecutionCondition::Unconditional);
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        assert_eq!(formation.inputs, [short.output]);

        let effecting = AnalysisUnit::new(concat!(
            "int opaque(int); int f(int n, int m) { ",
            "int values[n && opaque(m)]; return sizeof(values); }",
        ));
        let effecting_plan = &effecting.evaluations()[0];
        assert!(effecting_plan
            .operations()
            .iter()
            .any(|operation| matches!(operation.kind, TypeValueOp::ShortCircuitScalar { .. })));
        let guarded_call = effecting_plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::CallScalar { .. }))
            .expect("right-arm call");
        let ExecutionCondition::Guarded(call_guards) = &guarded_call.execution else {
            panic!("short-circuit call must not become unconditional")
        };
        assert_eq!(call_guards.len(), 1);
        assert!(matches!(
            effecting_plan.guards[call_guards[0].0 as usize].control,
            GuardControl::CfgEdge { .. }
        ));

        let or = AnalysisUnit::new(
            "int f(int n, int m) { int values[n || (m + 1)]; return sizeof(values); }",
        );
        assert_eq!(
            or.evaluations()[0].guards[0].polarity,
            GuardPolarity::WhenFalse
        );
        assert!(or.evaluations()[0].operations().iter().any(|operation| {
            matches!(
                &operation.kind,
                TypeValueOp::ConstantScalar {
                    value: ScalarOperand::Constant { spelling, .. },
                    ..
                } if spelling == "1"
            )
        }));
    }

    #[test]
    fn scalar_places_distinguish_shadowed_objects_and_reuse_one_object_identity() {
        let source = "int f(int x){x++; {int x=0; x++;} return x;}";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let first_increment = source.find("x++").expect("outer increment") as u32;
        let second_increment = source.rfind("x++").expect("inner increment") as u32;
        let returned = source.rfind('x').expect("returned x") as u32;
        let place_at = |offset| {
            operations
                .iter()
                .find_map(|operation| match operation.kind {
                    TypeValueOp::ReadScalar {
                        place, occurrence, ..
                    } if occurrence.lo == offset => Some(place),
                    _ => None,
                })
                .expect("operation-owned scalar read")
        };

        let outer = place_at(first_increment);
        let inner = place_at(second_increment);
        assert_ne!(outer, inner);
        assert_eq!(outer, place_at(returned));
        for operation in operations {
            if let TypeValueOp::WriteScalar {
                place, occurrence, ..
            } = operation.kind
            {
                assert_eq!(place, place_at(occurrence.lo));
            }
        }
    }

    #[test]
    fn address_of_a_scalar_place_is_not_a_value_read() {
        let source = "int f(void){int x=1; int *p=&x; return x;}";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let addressed = source.find("&x").expect("address expression") as u32;
        let address = operations
            .iter()
            .find(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::AddressOf { span, .. } if span.lo == addressed
                )
            })
            .expect("operation-owned address");
        let TypeValueOp::AddressOf {
            place,
            declaration,
            occurrence: address_occurrence,
            ..
        } = address.kind
        else {
            unreachable!()
        };
        assert_eq!(&source[declaration.range()], "x");
        assert_eq!(&source[address_occurrence.range()], "x");
        assert!(
            address.inputs.is_empty(),
            "taking an address reads no value"
        );

        let returned = source.rfind('x').expect("returned x") as u32;
        let return_place = operations
            .iter()
            .find_map(|operation| match operation.kind {
                TypeValueOp::ReadScalar {
                    place, occurrence, ..
                } if occurrence.lo == returned => Some(place),
                _ => None,
            })
            .expect("return read");
        assert_eq!(place, return_place);
        assert!(operations.iter().all(|operation| {
            !matches!(
                operation.kind,
                TypeValueOp::ReadScalar { occurrence, .. }
                    if occurrence == address_occurrence
            )
        }));
    }

    #[test]
    fn dereference_is_an_explicit_load_from_the_pointer_value() {
        let source = "int f(int *p){return *p;}";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let pointer = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ReadScalar { .. }))
            .expect("pointer value read");
        let load = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .expect("indirect scalar load");
        assert_eq!(load.inputs, [pointer.output]);
        let TypeValueOp::LoadScalar {
            base, access, span, ..
        } = &load.kind
        else {
            unreachable!()
        };
        assert_eq!(access, &ProjectedAccess::Dereference);
        assert_eq!(&source[span.range()], "*p");
        let Some(SemanticPlace::Projected(projected)) = load.place else {
            panic!("load must name its projected dereference place")
        };
        let projected = &unit.evaluations()[0].projected_places()[projected.0 as usize];
        assert_eq!(projected.id.0, 0);
        assert_eq!(projected.span, *span);
        assert_eq!(
            projected.kind,
            ProjectedPlaceKind::Dereference {
                address: pointer.output
            }
        );
        let pointer_occurrence = source.rfind('p').expect("dereferenced pointer") as u32;
        assert_eq!(
            base.occurrence(),
            Span::new(pointer_occurrence, pointer_occurrence + 1)
        );
        let finish = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
            .expect("return consumer");
        assert_eq!(finish.inputs, [load.output]);
    }

    #[test]
    fn pointer_fields_and_elements_have_explicit_projected_places() {
        let source = "struct S{int x;};int f(struct S *p,int *a,int i){return p->x+a[i];}";
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let loads = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .collect::<Vec<_>>();
        assert_eq!(loads.len(), 2);

        let field = loads
            .iter()
            .find(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::LoadScalar {
                        access: ProjectedAccess::Field { .. },
                        ..
                    }
                )
            })
            .expect("pointer-member load");
        assert_eq!(field.inputs.len(), 1);
        let Some(SemanticPlace::Projected(field_place)) = field.place else {
            panic!("field load must own a projected place")
        };
        assert_eq!(
            plan.projected_places()[field_place.0 as usize].kind,
            ProjectedPlaceKind::Field {
                base: ProjectedBase::Value(field.inputs[0]),
                member: "x".to_owned(),
                overlapping_members: false,
            }
        );

        let element = loads
            .iter()
            .find(|operation| {
                matches!(
                    operation.kind,
                    TypeValueOp::LoadScalar {
                        access: ProjectedAccess::Element { .. },
                        ..
                    }
                )
            })
            .expect("indexed load");
        assert_eq!(element.inputs.len(), 2);
        let Some(SemanticPlace::Projected(element_place)) = element.place else {
            panic!("element load must own a projected place")
        };
        assert_eq!(
            plan.projected_places()[element_place.0 as usize].kind,
            ProjectedPlaceKind::Element {
                base: ProjectedBase::Value(element.inputs[0]),
                index: element.inputs[1],
            }
        );
    }

    #[test]
    fn array_objects_and_adjusted_array_parameters_have_distinct_bases() {
        let source = concat!(
            "int f(int local_i,int parameter_i,int parameter[4]){",
            "int local[4];return local[local_i]+parameter[parameter_i];}"
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let element_places = plan
            .operations()
            .iter()
            .filter_map(|operation| {
                let SemanticPlace::Projected(id) = operation.place? else {
                    return None;
                };
                matches!(
                    plan.projected_places()[id.0 as usize].kind,
                    ProjectedPlaceKind::Element { .. }
                )
                .then_some((operation, &plan.projected_places()[id.0 as usize].kind))
            })
            .collect::<Vec<_>>();
        assert_eq!(element_places.len(), 2);

        let local = element_places
            .iter()
            .find(|(operation, _)| {
                &source[operation_span(&operation.kind).range()] == "local[local_i]"
            })
            .expect("local array element");
        assert!(matches!(
            local.1,
            ProjectedPlaceKind::Element {
                base: ProjectedBase::Place(_),
                ..
            }
        ));
        assert_eq!(local.0.inputs.len(), 1, "only the index is a scalar input");

        let parameter = element_places
            .iter()
            .find(|(operation, _)| {
                &source[operation_span(&operation.kind).range()] == "parameter[parameter_i]"
            })
            .expect("adjusted array parameter element");
        assert!(matches!(
            parameter.1,
            ProjectedPlaceKind::Element {
                base: ProjectedBase::Value(_),
                ..
            }
        ));
        assert_eq!(parameter.0.inputs.len(), 2, "pointer value then index");
    }

    #[test]
    fn pointer_field_and_element_writes_are_explicit_stores() {
        let source = concat!(
            "struct S{int x;};void f(struct S *p,int *a,int i,int v){",
            "p->x=v;a[i]=v;}"
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let stores = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .collect::<Vec<_>>();
        assert_eq!(stores.len(), 2);

        for store in stores {
            let TypeValueOp::StoreScalar {
                access,
                target,
                span,
                ..
            } = &store.kind
            else {
                unreachable!()
            };
            assert_eq!(span.hi, target.hi + 2, "`=v` follows the target");
            let Some(SemanticPlace::Projected(place)) = store.place else {
                panic!("projected store place")
            };
            match (access, &plan.projected_places()[place.0 as usize].kind) {
                (
                    ProjectedAccess::Field { member, .. },
                    ProjectedPlaceKind::Field {
                        base,
                        member: projected_member,
                        ..
                    },
                ) => {
                    assert_eq!(member, "x");
                    assert_eq!(projected_member, member);
                    assert_eq!(*base, ProjectedBase::Value(store.inputs[0]));
                    assert_eq!(store.inputs.len(), 2);
                }
                (ProjectedAccess::Element { .. }, ProjectedPlaceKind::Element { base, index }) => {
                    assert_eq!(*base, ProjectedBase::Value(store.inputs[0]));
                    assert_eq!(*index, store.inputs[1]);
                    assert_eq!(store.inputs.len(), 3);
                }
                pair => panic!("access/place mismatch: {pair:?}"),
            }
        }
    }

    #[test]
    fn an_array_object_store_projects_from_storage_without_reading_the_array() {
        let source = "void f(int i,int value){int array[4];array[i]=value;}";
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let store = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("array-element store");
        assert_eq!(store.inputs.len(), 2, "index then assigned value");
        let Some(SemanticPlace::Projected(place)) = store.place else {
            panic!("array-element place")
        };
        let ProjectedPlaceKind::Element {
            base: ProjectedBase::Place(array),
            ..
        } = plan.projected_places()[place.0 as usize].kind
        else {
            panic!("local array must be a storage-place base")
        };
        assert!(plan.operations().iter().all(|operation| {
            !matches!(
                operation.kind,
                TypeValueOp::ReadScalar { place, .. } if place == array
            )
        }));
    }

    #[test]
    fn record_array_members_decay_from_the_field_place_without_aggregate_loads() {
        let source = concat!(
            "struct S{int values[4];};",
            "int f(struct S object,struct S *pointer,int i){",
            "object.values[i]=1;return pointer->values[i];}"
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let elements = plan
            .operations()
            .iter()
            .filter_map(|operation| {
                let SemanticPlace::Projected(place) = operation.place? else {
                    return None;
                };
                matches!(
                    plan.projected_places()[place.0 as usize].kind,
                    ProjectedPlaceKind::Element { .. }
                )
                .then_some((operation, place))
            })
            .collect::<Vec<_>>();
        assert_eq!(elements.len(), 2);
        for (operation, place) in elements {
            let ProjectedPlaceKind::Element {
                base: ProjectedBase::Projected(field),
                ..
            } = plan.projected_places()[place.0 as usize].kind
            else {
                panic!("array element must project from its field place")
            };
            assert!(matches!(
                plan.projected_places()[field.0 as usize].kind,
                ProjectedPlaceKind::Field { ref member, .. } if member == "values"
            ));
            assert!(operation.inputs.len() <= 2, "only pointer and index values");
        }
        assert!(plan.operations().iter().all(|operation| {
            !matches!(
                &operation.kind,
                TypeValueOp::LoadScalar {
                    access: ProjectedAccess::Field { member, .. },
                    ..
                } if member == "values"
            )
        }));
    }

    #[test]
    fn multidimensional_arrays_keep_intermediate_rows_as_places() {
        let source = concat!(
            "struct S{int matrix[2][3];};",
            "int f(struct S object,struct S *pointer,int i,int j){int local[2][3];",
            "return local[i][j]+object.matrix[i][j]+pointer->matrix[i][j];}"
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let loads = plan
            .operations()
            .iter()
            .filter(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .collect::<Vec<_>>();
        assert_eq!(loads.len(), 3, "only final scalar elements are loaded");
        for load in loads {
            let Some(SemanticPlace::Projected(outer)) = load.place else {
                panic!("scalar element load must have a projected place")
            };
            let ProjectedPlaceKind::Element {
                base: ProjectedBase::Projected(row),
                ..
            } = plan.projected_places()[outer.0 as usize].kind
            else {
                panic!("outer element must project from an array row place")
            };
            assert!(matches!(
                plan.projected_places()[row.0 as usize].kind,
                ProjectedPlaceKind::Element { .. }
            ));
        }
    }

    #[test]
    fn direct_aggregate_fields_project_from_the_object_place() {
        let source = concat!(
            "struct S{int x;};int f(struct S object,int value){",
            "object.x=value;return object.x;}"
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let load = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .expect("direct-field load");
        let store = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("direct-field store");

        assert!(
            load.inputs.is_empty(),
            "the aggregate is a place, not a scalar input"
        );
        assert_eq!(
            store.inputs.len(),
            1,
            "only the assigned value is scalar input"
        );
        let Some(SemanticPlace::Projected(load_place)) = load.place else {
            panic!("direct-field load place")
        };
        let Some(SemanticPlace::Projected(store_place)) = store.place else {
            panic!("direct-field store place")
        };
        let load_kind = &plan.projected_places()[load_place.0 as usize].kind;
        let store_kind = &plan.projected_places()[store_place.0 as usize].kind;
        assert_eq!(load_kind, store_kind);
        let ProjectedPlaceKind::Field {
            base: ProjectedBase::Place(object),
            member,
            ..
        } = load_kind
        else {
            panic!("field must project from the direct aggregate place: {load_kind:?}")
        };
        assert_eq!(member, "x");
        assert!(plan.operations().iter().all(|operation| {
            !matches!(
                operation.kind,
                TypeValueOp::ReadScalar { place, .. } if place == *object
            )
        }));
    }

    #[test]
    fn nested_fields_form_inside_out_place_chains() {
        let source = concat!(
            "struct Inner{int x;};struct Outer{struct Inner inner;};",
            "int f(struct Outer object,struct Outer *p,int value){",
            "object.inner.x=value;return p->inner.x;}"
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        let store = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("nested direct-field store");
        let load = plan
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .expect("nested pointer-field load");

        let nested_base = |operation: &EvaluationOp| {
            let Some(SemanticPlace::Projected(outer)) = operation.place else {
                panic!("outer projected place")
            };
            let ProjectedPlaceKind::Field {
                base: ProjectedBase::Projected(inner),
                member,
                ..
            } = &plan.projected_places()[outer.0 as usize].kind
            else {
                panic!("outer field must be based on an inner projected place")
            };
            assert_eq!(member, "x");
            &plan.projected_places()[inner.0 as usize].kind
        };

        assert!(matches!(
            nested_base(store),
            ProjectedPlaceKind::Field {
                base: ProjectedBase::Place(_),
                member,
                ..
            } if member == "inner"
        ));
        assert!(matches!(
            nested_base(load),
            ProjectedPlaceKind::Field {
                base: ProjectedBase::Value(_),
                member,
                ..
            } if member == "inner"
        ));
        assert_eq!(store.inputs.len(), 1, "only the assigned value is scalar");
        assert_eq!(load.inputs.len(), 1, "only the pointer value is scalar");
    }

    #[test]
    fn indirect_assignment_is_an_explicit_store_with_conservative_ordering() {
        let source = "int f(int *p,int x){return (*p=x);}";
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        let store = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("indirect scalar store");
        assert_eq!(&source[operation_span(&store.kind).range()], "*p=x");
        let TypeValueOp::StoreScalar { target, span, .. } = store.kind else {
            unreachable!()
        };
        assert_eq!(&source[target.range()], "*p");
        let Some(SemanticPlace::Projected(projected)) = store.place else {
            panic!("store must name its projected dereference place")
        };
        let projected = &unit.evaluations()[0].projected_places()[projected.0 as usize];
        assert_eq!(projected.span, target);
        assert_eq!(
            projected.kind,
            ProjectedPlaceKind::Dereference {
                address: store.inputs[0]
            }
        );
        let pointer_occurrence = source.find("p=x").expect("pointer occurrence") as u32;
        assert_eq!(
            unit.evaluations()[0].pointer_store_constraints(),
            vec![PointerStoreConstraint {
                span,
                target,
                address_sources: vec![PointerValueSource::Copy {
                    place: PlaceId(0),
                    occurrence: Span::new(pointer_occurrence, pointer_occurrence + 1),
                }],
                assigned: store.inputs[1],
            }]
        );
        assert_eq!(store.inputs.len(), 2);
        assert!(store.inputs.iter().all(|input| {
            operations.iter().any(|operation| {
                operation.output == *input
                    && matches!(operation.kind, TypeValueOp::ReadScalar { .. })
            })
        }));
        let finish = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
            .expect("return consumer");
        assert_eq!(finish.inputs, [store.output]);

        let unsequenced = AnalysisUnit::new("int f(int *p,int x){return (*p=x)+x;}");
        let unsequenced = &unsequenced.evaluations()[0];
        assert!(unsequenced
            .operations()
            .iter()
            .all(|operation| !matches!(operation.kind, TypeValueOp::StoreScalar { .. })));
        assert_eq!(unsequenced.unsequenced_roots().len(), 1);

        let sequenced = AnalysisUnit::new("int f(int *p,int x){return (*p=x,x);}");
        let sequenced = &sequenced.evaluations()[0];
        assert!(sequenced
            .operations()
            .iter()
            .any(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. })));
        assert!(sequenced.unsequenced_roots().is_empty());
    }

    #[test]
    fn projected_updates_load_once_store_once_and_preserve_the_result_value() {
        let postfix = AnalysisUnit::new("int f(int *p){return (*p)++;}");
        let postfix = &postfix.evaluations()[0];
        let postfix_load = postfix
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .expect("postfix prior load");
        let postfix_store = postfix
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("postfix store");
        let TypeValueOp::StoreScalar { result, .. } = postfix_store.kind else {
            unreachable!()
        };
        assert_eq!(result, ScalarWriteResult::PreWrite);
        let postfix_finish = postfix
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
            .expect("postfix return");
        assert_eq!(postfix_finish.inputs, [postfix_load.output]);

        let prefix = AnalysisUnit::new("int f(int *p){return ++*p;}");
        let prefix = &prefix.evaluations()[0];
        let prefix_store = prefix
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("prefix store");
        let TypeValueOp::StoreScalar { result, .. } = prefix_store.kind else {
            unreachable!()
        };
        assert_eq!(result, ScalarWriteResult::PostWrite);
        let prefix_finish = prefix
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
            .expect("prefix return");
        assert_eq!(prefix_finish.inputs, [prefix_store.output]);

        let indexed = AnalysisUnit::new("int f(int *a,int i,int x){return a[i++] += x;}");
        let indexed = &indexed.evaluations()[0];
        let load = indexed
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::LoadScalar { .. }))
            .expect("compound prior load");
        let store = indexed
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
            .expect("compound store");
        assert_eq!(&store.inputs[..2], load.inputs.as_slice());
        assert_eq!(
            indexed
                .operations()
                .iter()
                .filter(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
                .count(),
            1,
            "the index increment is evaluated exactly once"
        );
        let TypeValueOp::StoreScalar { prior, result, .. } = &store.kind else {
            unreachable!()
        };
        assert!(prior.is_some());
        assert_eq!(*result, ScalarWriteResult::PostWrite);

        let nested = AnalysisUnit::new("int f(int *a,int i){return 1+a[i]++;}");
        let nested = &nested.evaluations()[0];
        assert!(nested.unsequenced_roots().is_empty());
        assert_eq!(
            nested
                .operations()
                .iter()
                .filter(|operation| matches!(operation.kind, TypeValueOp::StoreScalar { .. }))
                .count(),
            1,
            "a postfix update on a binary RHS must not capture the outer expression"
        );
        assert_eq!(
            nested
                .operations()
                .iter()
                .filter(|operation| matches!(operation.kind, TypeValueOp::ComputeScalar { .. }))
                .count(),
            2,
            "one increment computation and one enclosing addition"
        );
    }

    #[test]
    fn transparent_dereference_updates_keep_the_direct_object_place() {
        for (source, expected_result) in [
            (
                "int f(int x,int y){return (*&x += y);}",
                ScalarWriteResult::PostWrite,
            ),
            ("int f(int x){return (*&x)++;}", ScalarWriteResult::PreWrite),
            ("int f(int x){return ++*&x;}", ScalarWriteResult::PostWrite),
        ] {
            let unit = AnalysisUnit::new(source);
            let evaluation = &unit.evaluations()[0];
            assert!(evaluation.projected_places().is_empty(), "{source}");
            assert!(
                evaluation.operations().iter().all(|operation| {
                    !matches!(
                        operation.kind,
                        TypeValueOp::AddressOf { .. }
                            | TypeValueOp::LoadScalar { .. }
                            | TypeValueOp::StoreScalar { .. }
                    )
                }),
                "{source}"
            );

            let read = evaluation
                .operations()
                .iter()
                .find(|operation| {
                    matches!(
                        operation.kind,
                        TypeValueOp::ReadScalar { occurrence, .. }
                            if &source[occurrence.range()] == "x"
                    )
                })
                .expect("direct prior read");
            let write = evaluation
                .operations()
                .iter()
                .find(|operation| matches!(operation.kind, TypeValueOp::WriteScalar { .. }))
                .expect("direct update");
            let TypeValueOp::ReadScalar {
                place: read_place, ..
            } = read.kind
            else {
                unreachable!()
            };
            let TypeValueOp::WriteScalar {
                place: write_place,
                result,
                ..
            } = write.kind
            else {
                unreachable!()
            };
            assert_eq!(write_place, read_place, "{source}");
            assert_eq!(result, expected_result, "{source}");

            let finish = evaluation
                .operations()
                .iter()
                .find(|operation| matches!(operation.kind, TypeValueOp::FinishExpression { .. }))
                .expect("return consumer");
            assert_eq!(
                finish.inputs,
                [if expected_result == ScalarWriteResult::PreWrite {
                    read.output
                } else {
                    write.output
                }],
                "{source}"
            );
        }
    }

    #[test]
    fn pointer_constraints_follow_values_without_rescanning_rhs_spans() {
        let source = concat!(
            "int f(int c){int x=0,y=0;int *p=&x;int *q=p;",
            "p=c?&x:&y;int **pp=&p;int *r=*pp;return *q+*r;}"
        );
        let unit = AnalysisUnit::new(source);
        let constraints = unit.evaluations()[0].pointer_value_constraints();
        let sources_for = |name: &str| {
            constraints
                .iter()
                .find(|constraint| &source[constraint.occurrence.range()] == name)
                .map(|constraint| constraint.sources.clone())
                .expect("pointer destination constraint")
        };

        assert!(matches!(
            sources_for("q").as_slice(),
            [PointerValueSource::Copy { .. }]
        ));
        assert_eq!(
            sources_for("p")
                .iter()
                .filter(|source| matches!(source, PointerValueSource::Address(_)))
                .count(),
            1,
            "the first p constraint is its &x initializer"
        );
        let reassignment = constraints
            .iter()
            .find(|constraint| &source[constraint.effect.range()] == "p=c?&x:&y")
            .expect("conditional pointer assignment");
        assert_eq!(
            reassignment
                .sources
                .iter()
                .filter(|source| matches!(source, PointerValueSource::Address(_)))
                .count(),
            2
        );
        assert!(matches!(
            sources_for("r").as_slice(),
            [PointerValueSource::Load { .. }]
        ));
        assert!(!sources_for("q").contains(&PointerValueSource::Unknown));
        assert!(!sources_for("pp").contains(&PointerValueSource::Unknown));
        assert!(!sources_for("r").contains(&PointerValueSource::Unknown));
        assert!(!reassignment.sources.contains(&PointerValueSource::Unknown));
    }

    #[test]
    fn computed_pointer_load_constraints_follow_the_address_value() {
        let source = concat!(
            "int f(int c,int x,int y){int *p=&x;int *q=&y;",
            "return *(c?p:q);}",
        );
        let unit = AnalysisUnit::new(source);
        let constraints = unit.evaluations()[0].pointer_load_constraints();
        assert_eq!(
            constraints.len(),
            1,
            "constraints={constraints:#?}\noperations={:#?}",
            unit.evaluations()[0].operations()
        );
        assert_eq!(constraints[0].sources.len(), 2, "{constraints:#?}");
        assert!(
            constraints[0]
                .sources
                .iter()
                .all(|source| matches!(source, PointerValueSource::Copy { .. })),
            "{constraints:#?}"
        );
    }

    #[test]
    fn computed_pointer_store_constraints_follow_the_address_value() {
        let source = concat!(
            "int f(int c,int x){int a=0,b=0;int *p=&a;int *q=&b;",
            "*(c?p:q)=x;return a+b;}",
        );
        let unit = AnalysisUnit::new(source);
        let constraints = unit.evaluations()[0].pointer_store_constraints();
        assert_eq!(constraints.len(), 1, "{constraints:#?}");
        assert_eq!(constraints[0].address_sources.len(), 2, "{constraints:#?}");
        assert!(
            constraints[0]
                .address_sources
                .iter()
                .all(|source| matches!(source, PointerValueSource::Copy { .. })),
            "{constraints:#?}"
        );
        assert_eq!(
            unit.evaluations()[0]
                .pointer_sources(constraints[0].assigned)
                .len(),
            1
        );
    }

    #[test]
    fn nested_conditions_accumulate_guard_predicates() {
        let source = concat!(
            "int f(int c, int d, int n, int m, int k) { ",
            "int values[c ? (d ? n + 1 : m + 1) : k + 1]; return sizeof(values); }",
        );
        let unit = AnalysisUnit::new(source);
        let plan = &unit.evaluations()[0];
        assert_eq!(plan.guards.len(), 4);
        let n_compute = plan
            .operations()
            .iter()
            .find(|operation| {
                matches!(&operation.kind, TypeValueOp::ComputeScalar { span, .. }
                    if &source[span.range()] == "n + 1")
            })
            .expect("nested true-arm computation");
        let ExecutionCondition::Guarded(guards) = &n_compute.execution else {
            panic!("nested arm must be guarded")
        };
        assert_eq!(guards.len(), 2);
        assert!(guards.iter().any(|id| {
            let guard = &plan.guards[id.0 as usize];
            guard.polarity == GuardPolarity::WhenTrue && &source[guard.span.range()] == "n + 1"
        }));
        assert!(guards.iter().any(|id| {
            let guard = &plan.guards[id.0 as usize];
            guard.polarity == GuardPolarity::WhenTrue
                && &source[guard.span.range()] == "d ? n + 1 : m + 1"
        }));
    }

    #[test]
    fn discarded_constant_result_does_not_capture_an_earlier_value_id() {
        let unit =
            AnalysisUnit::new("int f(int n) { int values[(n++, 4)]; return sizeof(values); }");
        let operations = unit.evaluations()[0].operations();
        let formation = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        let sequence = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::SequenceScalar { .. }))
            .expect("comma sequence");
        assert!(matches!(
            unit.evaluations()[0].values[sequence.inputs[0].0 as usize].scalar,
            Some(ScalarOperand::Constant { .. })
        ));
        assert_eq!(formation.inputs, [sequence.output]);
        let read = operations
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::ReadBound { .. }))
            .expect("bound read");
        assert_eq!(read.inputs, [formation.output]);
    }

    #[test]
    fn bound_formation_inside_loop_is_owned_by_the_repeated_cfg_cycle() {
        let source = concat!(
            "int f(int n) { while (n > 0) { int values[n]; ",
            "consume(sizeof(values)); } return 0; }",
        );
        let unit = AnalysisUnit::new(source);
        let operation = unit.evaluations()[0]
            .operations()
            .iter()
            .find(|operation| matches!(operation.kind, TypeValueOp::FormBound { .. }))
            .expect("bound formation");
        let owner = NodeId::new(operation.cfg_node);
        assert_ne!(owner, unit.functions()[0].cfg.entry());

        let cfg = &unit.functions()[0].cfg;
        let mut seen = vec![false; cfg.node_count()];
        let mut stack = cfg.successors(owner).collect::<Vec<_>>();
        let mut returns_to_owner = false;
        while let Some(node) = stack.pop() {
            if node == owner {
                returns_to_owner = true;
                break;
            }
            if seen.get(node.index()).copied().unwrap_or(true) {
                continue;
            }
            seen[node.index()] = true;
            stack.extend(cfg.successors(node));
        }
        assert!(returns_to_owner, "formation must execute on each loop trip");
    }

    #[test]
    fn unreachable_bound_formation_is_not_relocated_to_entry() {
        let source = concat!(
            "int f(int n) { int live[n]; return sizeof(live); ",
            "int unreachable[n]; }",
        );
        let unit = AnalysisUnit::new(source);
        let operations = unit.evaluations()[0].operations();
        assert_eq!(
            operations.len(),
            3,
            "one scalar read, one formation, and one captured read"
        );
        assert!(operations.iter().all(|operation| operation.cfg_node > 0));
        assert!(operations.iter().all(|operation| {
            let span = operation_span(&operation.kind);
            &source[span.range()] != "n" || span.lo < source.find("return").unwrap() as u32
        }));
    }
}
