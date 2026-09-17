//! The typed-operations export: one function's evaluation plan as a list of
//! operations with C types and explicit conversions.
//!
//! [`crate::csource::eval::EvaluationPlan`] is the evaluation model --- what
//! executes, in what order, reading and writing which place, guarded by which
//! condition, in which CFG node. It carries no types. The AST typer
//! ([`crate::csource::semantic::expr_types::ExpressionTyper`]) is the type
//! model --- the C type of every expression after lvalue conversion, integer
//! promotion and the usual arithmetic conversions. This module joins the two:
//! each plan operation is matched to the AST expression node with the same
//! byte span, and the conversions C performs between an operand's own type and
//! the type the typer says the operator works in are written out as `convert`
//! operations. Nothing here decides a promotion or a common type on its own;
//! where the typer says `unknown`, the export says `unknown`.
//!
//! # Contract
//!
//! Every operation the plan lowered appears exactly once, in plan order, and
//! every expression root the plan declined appears as an `unknown` operation
//! carrying the reason, so silence in this export means "no expression here",
//! never "an expression the lowering could not handle". The one deliberate
//! omission is the completion of a discarded value (an expression statement or
//! a `for` clause): the expression's effects are already operations, and its
//! value has no language-level consumer to stand for.

use std::collections::BTreeMap;

use crate::csource::cfg::FunctionCfg;
use crate::csource::eval::{
    ControlConditionKind, DeclinedRoot, EvaluationId, EvaluationOp, EvaluationPlan,
    EvaluationPurpose, ExecutionCondition, GuardPolarity, ProjectedAccess, ProjectionBaseOperand,
    ScalarOperand, ScalarWriteKind, TypeValueOp, ValueId,
};
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::csource::semantic::expr_types::{
    integer_literal, CType, ExpressionTyper, ExpressionTypes, IntKind,
};
use crate::syntax::graph_export::{ExportEdge, ExportNode, GraphView};
use crate::syntax::ids::{NodeId, Span};

use super::{operators_of, snippet, LineIndex};

/// The conversion kinds this export distinguishes, C17 §6.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Conversion {
    /// §6.3.1.1p2: a type of lower rank than `int` becomes `int`.
    Promotion,
    /// §6.3.1.8: the common type two operands of an arithmetic or relational
    /// operator are brought to.
    UsualArithmetic,
    /// §6.3.2.1/§6.5.16.1p2: the value assigned, initialized or returned takes
    /// the type of its destination.
    Assignment,
}

impl Conversion {
    const fn name(self) -> &'static str {
        match self {
            Self::Promotion => "promotion",
            Self::UsualArithmetic => "usual_arithmetic",
            Self::Assignment => "assignment",
        }
    }
}

/// What every operation emitted while handling one plan operation inherits.
#[derive(Debug, Clone)]
struct Context {
    block: String,
    root: String,
    guarded_by: String,
}

/// One function's typed operations as a view.
///
/// `typer` is `None` when the unit has no semantic tables for the function;
/// every type is then `unknown`, which is the documented meaning of that
/// value, and the operations and conversions that do not depend on a type
/// (loads, stores, calls, branches, the `unknown` roots) are still exported.
pub(super) fn ops_view(
    name: &str,
    text: &str,
    tree: &Tree,
    token_spans: &[Span],
    function: &FunctionCfg,
    plan: &EvaluationPlan,
    typer: Option<&ExpressionTyper<'_>>,
) -> GraphView {
    let lines = LineIndex::new(text);
    let mut builder = Builder {
        text,
        tree,
        token_spans,
        lines: &lines,
        plan,
        typer,
        types: typer.map(|typer| typer.compute(function.node)),
        node_at_span: expression_nodes_by_span(tree, token_spans, function.node),
        return_type: typer.map_or(CType::UNKNOWN, |typer| {
            typer.return_type_of_function(function.node)
        }),
        view: GraphView::new(name),
        result_type: Vec::new(),
        value_export: BTreeMap::new(),
        span_export: BTreeMap::new(),
    };
    let mut declined = plan.declined_roots().iter().peekable();
    for operation in plan.operations() {
        let at = operation.span().lo;
        while let Some(root) = declined.next_if(|root| root.expression.lo < at) {
            builder.unknown_root(root);
        }
        builder.operation(operation);
    }
    for root in declined {
        builder.unknown_root(root);
    }
    builder.view
}

/// Every expression node under `root` by its span; when two nest with the
/// same span, the inner one wins, since the typer gives them the same type.
fn expression_nodes_by_span(
    tree: &Tree,
    token_spans: &[Span],
    root: NodeId,
) -> BTreeMap<Span, NodeId> {
    let arena = tree.arena();
    let mut out = BTreeMap::new();
    for node in arena.preorder(root) {
        let is_expression = arena
            .tag(node)
            .and_then(NodeTag::from_u16)
            .is_some_and(NodeTag::is_expression);
        if !is_expression {
            continue;
        }
        if let Some(span) = arena.span(node, token_spans) {
            out.insert(span, node);
        }
    }
    out
}

struct Builder<'a> {
    text: &'a str,
    tree: &'a Tree,
    token_spans: &'a [Span],
    lines: &'a LineIndex,
    plan: &'a EvaluationPlan,
    typer: Option<&'a ExpressionTyper<'a>>,
    types: Option<ExpressionTypes>,
    node_at_span: BTreeMap<Span, NodeId>,
    return_type: CType,
    view: GraphView,
    /// The type of every exported operation's value, by exported id.
    result_type: Vec<CType>,
    /// Exported operation whose value stands for a plan value.
    value_export: BTreeMap<ValueId, u32>,
    /// Exported operation whose value stands for a plan operation's span.
    span_export: BTreeMap<Span, u32>,
}

impl Builder<'_> {
    // --- types --------------------------------------------------------------

    fn joined(&self, span: Span) -> Option<(NodeId, NodeTag)> {
        let node = *self.node_at_span.get(&span)?;
        let tag = self.tree.arena().tag(node).and_then(NodeTag::from_u16)?;
        Some((node, tag))
    }

    /// The typer's type for the expression at `span`, or unknown.
    fn type_at(&self, span: Span) -> CType {
        let Some(types) = &self.types else {
            return CType::UNKNOWN;
        };
        self.node_at_span
            .get(&span)
            .and_then(|node| types.type_of(*node))
            .cloned()
            .unwrap_or(CType::UNKNOWN)
    }

    fn declared_type_at(&self, declaration: Span) -> CType {
        self.typer
            .map_or(CType::UNKNOWN, |typer| typer.declared_type_at(declaration))
    }

    fn type_of(&self, exported: u32) -> CType {
        self.result_type
            .get(exported as usize)
            .cloned()
            .unwrap_or(CType::UNKNOWN)
    }

    /// The typer's rule for `op` on two operand types: `(result, operand)`.
    fn binary_rule(&self, op: &str, left: &CType, right: &CType, offset: u32) -> (CType, CType) {
        match self.typer {
            Some(typer) => {
                let (result, operand) = typer.binary(op, left, right, offset);
                (result, operand.unwrap_or(CType::UNKNOWN))
            }
            None => (CType::UNKNOWN, CType::UNKNOWN),
        }
    }

    // --- emission -----------------------------------------------------------

    fn context(&self, operation: &EvaluationOp) -> Context {
        let guarded_by = match &operation.execution {
            ExecutionCondition::Unconditional => String::new(),
            ExecutionCondition::Guarded(guards) => guards
                .iter()
                .map(|guard| {
                    let Some(guard) = self.plan.guards().get(guard.0 as usize) else {
                        return "?".to_owned();
                    };
                    let condition = self
                        .value_export
                        .get(&guard.condition)
                        .map_or_else(|| "?".to_owned(), u32::to_string);
                    let polarity = match guard.polarity {
                        GuardPolarity::WhenTrue => "true",
                        GuardPolarity::WhenFalse => "false",
                    };
                    format!("{condition}:{polarity}")
                })
                .collect::<Vec<_>>()
                .join(","),
        };
        Context {
            block: operation.cfg_node.to_string(),
            root: operation
                .evaluation
                .map_or_else(String::new, |id: EvaluationId| id.0.to_string()),
            guarded_by,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        context: &Context,
        kind: &str,
        op: &str,
        ty: &CType,
        inputs: &[u32],
        span: Span,
        extra: &[(&str, String)],
    ) -> u32 {
        let id = self.view.nodes.len() as u32;
        let rendered = ty.render();
        let (line, column) = self.lines.position(span.lo);
        let source = snippet(self.text, span);
        let label = if source.is_empty() {
            format!("{kind} {op}: {rendered}")
        } else {
            format!("{kind} {op}: {rendered}\n{source}")
        };
        let mut node = ExportNode::new(id, label)
            .with("kind", kind)
            .with("op", op)
            .with("type", rendered)
            .with(
                "inputs",
                inputs
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .with("block", context.block.clone())
            .with("root", context.root.clone())
            .with("guarded_by", context.guarded_by.clone())
            .with("span", format!("{}:{}", span.lo, span.hi))
            .with("line", line.to_string())
            .with("column", column.to_string());
        for (key, value) in extra {
            node = node.with(*key, value.clone());
        }
        self.view.nodes.push(node);
        for (index, input) in inputs.iter().enumerate() {
            self.view.edges.push(
                ExportEdge::new(*input, id, index.to_string()).with("index", index.to_string()),
            );
        }
        self.result_type.push(ty.clone());
        id
    }

    /// `input` converted to `target` by `kind`, or `input` itself when no
    /// conversion applies: the types agree, or the target is unknown, or the
    /// target is not arithmetic for an arithmetic conversion (pointer
    /// arithmetic converts nothing).
    fn convert(
        &mut self,
        context: &Context,
        input: u32,
        target: &CType,
        kind: Conversion,
        span: Span,
    ) -> u32 {
        let from = self.type_of(input);
        if target.is_unknown() || from == *target {
            return input;
        }
        if kind != Conversion::Assignment && !target.is_arithmetic() {
            return input;
        }
        let mut current = input;
        let mut from = from;
        let mut kind = kind;
        if kind == Conversion::UsualArithmetic && from.is_arithmetic() {
            let promoted = from.promoted();
            if promoted == *target {
                // §6.3.1.8 begins with the integer promotions; when they
                // already reach the common type, that is the whole conversion.
                kind = Conversion::Promotion;
            } else if promoted != from {
                current = self.convert_step(
                    context,
                    current,
                    &from,
                    &promoted,
                    Conversion::Promotion,
                    span,
                );
                from = promoted;
            }
        }
        self.convert_step(context, current, &from, target, kind, span)
    }

    fn convert_step(
        &mut self,
        context: &Context,
        input: u32,
        from: &CType,
        to: &CType,
        kind: Conversion,
        span: Span,
    ) -> u32 {
        self.emit(
            context,
            "convert",
            kind.name(),
            to,
            &[input],
            span,
            &[("from", from.render()), ("to", to.render())],
        )
    }

    /// The exported operation standing for a plan value, materializing a
    /// `const` or a leaf `load` for a value the plan left producerless.
    fn input(&mut self, context: &Context, value: ValueId) -> Option<u32> {
        if let Some(id) = self.value_export.get(&value) {
            return Some(*id);
        }
        let scalar = self.plan.value_scalar(value)?.clone();
        let id = match scalar {
            ScalarOperand::Constant {
                spelling,
                occurrence,
            } => self.constant(context, &spelling, occurrence),
            ScalarOperand::Declaration {
                declaration,
                occurrence,
                ..
            } => self.scalar_load(context, declaration, occurrence),
            ScalarOperand::Produced { expression } => *self.span_export.get(&expression)?,
        };
        self.value_export.insert(value, id);
        Some(id)
    }

    fn inputs(&mut self, context: &Context, operation: &EvaluationOp) -> (Vec<u32>, usize) {
        let mut resolved = Vec::with_capacity(operation.inputs.len());
        let mut missing = 0usize;
        for value in &operation.inputs {
            match self.input(context, *value) {
                Some(id) => resolved.push(id),
                None => missing += 1,
            }
        }
        (resolved, missing)
    }

    fn constant(&mut self, context: &Context, spelling: &str, occurrence: Span) -> u32 {
        let mut ty = self.type_at(occurrence);
        if ty.is_unknown() {
            ty = integer_literal(spelling);
        }
        self.emit(context, "const", spelling, &ty, &[], occurrence, &[])
    }

    fn scalar_load(&mut self, context: &Context, declaration: Span, occurrence: Span) -> u32 {
        let mut ty = self.type_at(occurrence);
        if ty.is_unknown() {
            ty = self.declared_type_at(declaration);
        }
        let name = self.name_of(declaration);
        self.emit(
            context,
            "load",
            &name,
            &ty,
            &[],
            occurrence,
            &[
                ("access", "scalar".to_owned()),
                ("name", name.clone()),
                ("declared", format!("{}:{}", declaration.lo, declaration.hi)),
            ],
        )
    }

    fn name_of(&self, declaration: Span) -> String {
        self.text
            .get(declaration.lo as usize..declaration.hi as usize)
            .unwrap_or_default()
            .to_owned()
    }

    fn missing(missing: usize) -> Vec<(&'static str, String)> {
        if missing == 0 {
            Vec::new()
        } else {
            vec![("missing_inputs", missing.to_string())]
        }
    }

    // --- plan operations ----------------------------------------------------

    fn operation(&mut self, operation: &EvaluationOp) {
        let context = self.context(operation);
        let span = operation.span();
        let (inputs, missing) = self.inputs(&context, operation);
        let result: Option<u32> = match &operation.kind {
            TypeValueOp::FinishExpression { purpose, .. } => {
                self.finish(&context, operation, *purpose, &inputs, missing)
            }
            other => Some(self.plain(&context, other, &inputs, missing, span)),
        };
        if let Some(result) = result {
            self.value_export.insert(operation.output, result);
            self.span_export.insert(span, result);
        }
    }

    /// Every plan operation but `FinishExpression`, which may stand for a
    /// discarded value and export nothing.
    fn plain(
        &mut self,
        context: &Context,
        kind: &TypeValueOp,
        inputs: &[u32],
        missing: usize,
        span: Span,
    ) -> u32 {
        let inputs = inputs.to_vec();
        let context = context.clone();
        match kind {
            // The plan's constant operation consumes the leaf constant value
            // as its one input; the `const` materialized for that input is
            // the operation, not a second copy.
            TypeValueOp::ConstantScalar { value, .. } => match (inputs.first(), value) {
                (Some(input), _) => *input,
                (
                    None,
                    ScalarOperand::Constant {
                        spelling,
                        occurrence,
                    },
                ) => self.constant(&context, spelling, *occurrence),
                (None, _) => self.emit(&context, "const", "?", &CType::UNKNOWN, &[], span, &[]),
            },
            TypeValueOp::ReadScalar {
                declaration,
                occurrence,
                ..
            } => self.scalar_load(&context, *declaration, *occurrence),
            TypeValueOp::AddressOf { declaration, .. } => {
                let ty = self.type_at(span);
                let name = self.name_of(*declaration);
                self.emit(
                    &context,
                    "unary",
                    "&",
                    &ty,
                    &[],
                    span,
                    &[
                        ("name", name),
                        ("declared", format!("{}:{}", declaration.lo, declaration.hi)),
                    ],
                )
            }
            TypeValueOp::DecayArray {
                declaration,
                occurrence,
                ..
            } => {
                let mut ty = self.type_at(*occurrence);
                if ty.is_unknown() {
                    ty = self.declared_type_at(*declaration);
                }
                let name = self.name_of(*declaration);
                self.emit(
                    &context,
                    "unary",
                    "decay",
                    &ty.decayed(),
                    &[],
                    span,
                    &[
                        ("name", name),
                        ("declared", format!("{}:{}", declaration.lo, declaration.hi)),
                    ],
                )
            }
            TypeValueOp::LoadScalar { base, access, .. } => {
                let ty = self.type_at(span);
                let mut extra = projection_attributes(self.text, base, access);
                extra.extend(Self::missing(missing));
                self.emit(
                    &context,
                    "load",
                    access_operator(access),
                    &ty,
                    &inputs,
                    span,
                    &extra,
                )
            }
            TypeValueOp::StoreScalar {
                base,
                access,
                target,
                ..
            } => {
                let mut ty = self.type_at(span);
                if ty.is_unknown() {
                    ty = self.type_at(*target);
                }
                let address_count = base_input_count(base, access);
                let address = inputs.get(..address_count).unwrap_or(&[]).to_vec();
                // Plan inputs are: address operands, the prior value for a
                // compound store, then the assigned value last.
                let assigned = inputs.last().copied();
                let mut store_inputs = address;
                if let Some(assigned) = assigned {
                    let converted =
                        self.convert(&context, assigned, &ty, Conversion::Assignment, span);
                    store_inputs.push(converted);
                }
                let mut extra = projection_attributes(self.text, base, access);
                extra.extend(Self::missing(missing));
                self.emit(&context, "store", "=", &ty, &store_inputs, span, &extra)
            }
            TypeValueOp::WriteScalar {
                declaration,
                occurrence,
                kind,
                ..
            } => self.scalar_write(
                &context,
                span,
                *declaration,
                *occurrence,
                *kind,
                &inputs,
                missing,
            ),
            TypeValueOp::SelectScalar { .. } => {
                let ty = self.type_at(span);
                let mut select_inputs = Vec::with_capacity(3);
                for (index, input) in inputs.iter().enumerate() {
                    if index == 0 {
                        select_inputs.push(*input);
                    } else {
                        select_inputs.push(self.convert(
                            &context,
                            *input,
                            &ty,
                            Conversion::UsualArithmetic,
                            span,
                        ));
                    }
                }
                let extra = Self::missing(missing);
                self.emit(&context, "select", "?:", &ty, &select_inputs, span, &extra)
            }
            TypeValueOp::ComputeScalar { operator, .. } => {
                self.compute(&context, operator, &inputs, missing, span)
            }
            TypeValueOp::UnaryScalar { operator, .. } => {
                let mut ty = self.type_at(span);
                let operand = inputs.first().copied();
                if ty.is_unknown() {
                    ty = match (operator.as_str(), operand) {
                        ("!", _) => CType::int(IntKind::Int),
                        (_, Some(operand)) => self.type_of(operand).promoted(),
                        _ => CType::UNKNOWN,
                    };
                }
                let converted = match operand {
                    Some(operand) if operator != "!" => {
                        vec![self.convert(&context, operand, &ty, Conversion::Promotion, span)]
                    }
                    Some(operand) => vec![operand],
                    None => Vec::new(),
                };
                let extra = Self::missing(missing);
                self.emit(&context, "unary", operator, &ty, &converted, span, &extra)
            }
            TypeValueOp::ShortCircuitScalar { operator, .. } => {
                let extra = Self::missing(missing);
                self.emit(
                    &context,
                    "binary",
                    operator,
                    &CType::int(IntKind::Int),
                    &inputs,
                    span,
                    &extra,
                )
            }
            TypeValueOp::SequenceScalar { before, .. } => {
                let ty = inputs
                    .first()
                    .map_or(CType::UNKNOWN, |input| self.type_of(*input));
                let mut extra = Self::missing(missing);
                if let Some(before) = self.span_export.get(&before.occurrence()) {
                    extra.push(("sequenced_after", before.to_string()));
                }
                self.emit(&context, "sequence", ",", &ty, &inputs, span, &extra)
            }
            TypeValueOp::CallScalar { callee, .. } => {
                let ty = self.type_at(span);
                let mut extra = vec![("argument_conversions", "unknown".to_owned())];
                extra.extend(Self::missing(missing));
                self.emit(&context, "call", callee, &ty, &inputs, span, &extra)
            }
            TypeValueOp::FormBound {
                declaration,
                expression,
                ..
            } => {
                let ty = self.type_at(*expression);
                let name = self.name_of(*declaration);
                let mut extra = vec![
                    ("name", name),
                    ("declared", format!("{}:{}", declaration.lo, declaration.hi)),
                ];
                extra.extend(Self::missing(missing));
                self.emit(&context, "bound", "capture", &ty, &inputs, span, &extra)
            }
            TypeValueOp::ReadBound { .. } => {
                let ty = inputs
                    .first()
                    .map_or(CType::UNKNOWN, |input| self.type_of(*input));
                let extra = Self::missing(missing);
                self.emit(&context, "bound", "read", &ty, &inputs, span, &extra)
            }
            TypeValueOp::FinishExpression { .. } => unreachable!("handled by the caller"),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scalar_write(
        &mut self,
        context: &Context,
        span: Span,
        declaration: Span,
        occurrence: Span,
        kind: ScalarWriteKind,
        inputs: &[u32],
        missing: usize,
    ) -> u32 {
        let mut target = self.declared_type_at(declaration);
        if target.is_unknown() {
            target = self.type_at(occurrence);
        }
        let value = match kind {
            ScalarWriteKind::Assign => inputs.first().copied(),
            ScalarWriteKind::CompoundAssign => {
                let (prior, assigned) = (inputs.first().copied(), inputs.get(1).copied());
                match (prior, assigned) {
                    (Some(prior), Some(assigned)) => {
                        let operator = self
                            .assignment_operator(span, occurrence)
                            .unwrap_or_else(|| "?".to_owned());
                        let operator = operator.trim_end_matches('=').to_owned();
                        Some(self.arithmetic(context, &operator, prior, assigned, span))
                    }
                    (Some(only), None) | (None, Some(only)) => Some(only),
                    (None, None) => None,
                }
            }
            ScalarWriteKind::Increment => {
                let prior = inputs.first().copied();
                let (operator, operator_span) = self.increment_operator(span, occurrence);
                let one = self.constant(context, "1", operator_span);
                prior.map(|prior| self.arithmetic(context, &operator, prior, one, span))
            }
        };
        let mut store_inputs = Vec::with_capacity(1);
        if let Some(value) = value {
            store_inputs.push(self.convert(context, value, &target, Conversion::Assignment, span));
        }
        let name = self.name_of(declaration);
        let mut extra = vec![
            ("access", "scalar".to_owned()),
            ("name", name),
            ("declared", format!("{}:{}", declaration.lo, declaration.hi)),
        ];
        extra.extend(Self::missing(missing));
        self.emit(context, "store", "=", &target, &store_inputs, span, &extra)
    }

    /// The operator token of the assignment at `span`, from the AST when the
    /// span is an `assign_expr`, else the first token after the target.
    fn assignment_operator(&self, span: Span, occurrence: Span) -> Option<String> {
        if let Some((node, tag @ NodeTag::AssignExpr)) = self.joined(span) {
            let ops = operators_of(self.tree, node, tag, self.token_spans, self.text);
            if let Some(first) = ops.first() {
                return Some(first.clone());
            }
        }
        let rest = self.text.get(occurrence.hi as usize..span.hi as usize)?;
        rest.split_whitespace().next().map(str::to_owned)
    }

    /// `("+", span of "++")` or `("-", span of "--")` for the increment at
    /// `span` whose target occurrence is `occurrence`.
    fn increment_operator(&self, span: Span, occurrence: Span) -> (String, Span) {
        let operator_span = if span.lo < occurrence.lo {
            Span::new(span.lo, occurrence.lo)
        } else {
            Span::new(occurrence.hi, span.hi)
        };
        let token = self
            .text
            .get(operator_span.lo as usize..operator_span.hi as usize)
            .unwrap_or_default()
            .trim();
        let operator = if token == "--" { "-" } else { "+" };
        (operator.to_owned(), operator_span)
    }

    /// A `binary` operation on two already-exported operands, typed by the
    /// typer's rule for `operator`, with the operand conversions written out.
    fn arithmetic(
        &mut self,
        context: &Context,
        operator: &str,
        left: u32,
        right: u32,
        span: Span,
    ) -> u32 {
        let left_type = self.type_of(left);
        let right_type = self.type_of(right);
        let (result, operand) = self.binary_rule(operator, &left_type, &right_type, span.lo);
        self.binary(context, operator, left, right, &result, &operand, span, &[])
    }

    #[allow(clippy::too_many_arguments)]
    fn binary(
        &mut self,
        context: &Context,
        operator: &str,
        left: u32,
        right: u32,
        result: &CType,
        operand: &CType,
        span: Span,
        extra: &[(&str, String)],
    ) -> u32 {
        let shift = matches!(operator, "<<" | ">>");
        let left = self.convert(context, left, operand, Conversion::UsualArithmetic, span);
        let right = if shift {
            let promoted = self.type_of(right).promoted();
            self.convert(context, right, &promoted, Conversion::Promotion, span)
        } else {
            self.convert(context, right, operand, Conversion::UsualArithmetic, span)
        };
        let kind = if matches!(operator, "<" | "<=" | ">" | ">=" | "==" | "!=") {
            "compare"
        } else {
            "binary"
        };
        let mut attributes = vec![("operand_type", operand.render())];
        attributes.extend(extra.iter().map(|(key, value)| (*key, value.clone())));
        self.emit(
            context,
            kind,
            operator,
            result,
            &[left, right],
            span,
            &attributes,
        )
    }

    /// A plan `ComputeScalar`: typed from the AST node at its span when there
    /// is one (a chain operator, a compound assignment, an increment), else
    /// from the typer's rule on its operand types.
    fn compute(
        &mut self,
        context: &Context,
        operator: &str,
        inputs: &[u32],
        missing: usize,
        span: Span,
    ) -> u32 {
        let (Some(left), Some(right)) = (inputs.first().copied(), inputs.get(1).copied()) else {
            let extra = Self::missing(missing);
            return self.emit(
                context,
                "binary",
                operator,
                &CType::UNKNOWN,
                inputs,
                span,
                &extra,
            );
        };
        let joined = self.joined(span).and_then(|(node, tag)| {
            let types = self.types.as_ref()?;
            match tag {
                NodeTag::BinaryExpr => {
                    let arena = self.tree.arena();
                    let children: Vec<NodeId> = arena.children_iter(node).collect();
                    let position = children.iter().position(|child| {
                        arena
                            .span(*child, self.token_spans)
                            .is_some_and(|child| child.hi == span.hi)
                    })?;
                    let index = position.checked_sub(1)?;
                    let operand = types.operand_types_of(node)?.get(index)?.clone();
                    let result = types.chain_results_of(node)?.get(index)?.clone();
                    Some((result, operand))
                }
                NodeTag::AssignExpr => {
                    let operand = types.operand_types_of(node)?.first()?.clone();
                    Some((operand.clone(), operand))
                }
                _ => None,
            }
        });
        let (result, operand) = joined.unwrap_or_else(|| {
            let left_type = self.type_of(left);
            let right_type = self.type_of(right);
            self.binary_rule(operator, &left_type, &right_type, span.lo)
        });
        let extra = Self::missing(missing);
        self.binary(
            context, operator, left, right, &result, &operand, span, &extra,
        )
    }

    fn finish(
        &mut self,
        context: &Context,
        operation: &EvaluationOp,
        purpose: EvaluationPurpose,
        inputs: &[u32],
        missing: usize,
    ) -> Option<u32> {
        let span = operation.span();
        let value = inputs.first().copied();
        Some(match purpose {
            EvaluationPurpose::Initialize { declaration, .. } => {
                let target = self.declared_type_at(declaration);
                let mut store_inputs = Vec::with_capacity(1);
                if let Some(value) = value {
                    store_inputs.push(self.convert(
                        context,
                        value,
                        &target,
                        Conversion::Assignment,
                        span,
                    ));
                }
                let name = self.name_of(declaration);
                let mut extra = vec![
                    ("access", "scalar".to_owned()),
                    ("name", name),
                    ("declared", format!("{}:{}", declaration.lo, declaration.hi)),
                ];
                extra.extend(Self::missing(missing));
                self.emit(context, "store", "=", &target, &store_inputs, span, &extra)
            }
            EvaluationPurpose::Return { statement } => {
                let target = self.return_type.clone();
                let mut return_inputs = Vec::with_capacity(1);
                if let Some(value) = value {
                    return_inputs.push(self.convert(
                        context,
                        value,
                        &target,
                        Conversion::Assignment,
                        span,
                    ));
                }
                let mut extra = Vec::new();
                if target.is_unknown() {
                    extra.push(("return_conversion", "unknown".to_owned()));
                }
                extra.extend(Self::missing(missing));
                self.emit(
                    context,
                    "return",
                    "return",
                    &CType::VOID,
                    &return_inputs,
                    statement,
                    &extra,
                )
            }
            EvaluationPurpose::Control { kind, .. } => {
                let op = match kind {
                    ControlConditionKind::If => "if",
                    ControlConditionKind::While => "while",
                    ControlConditionKind::DoWhile => "do_while",
                    ControlConditionKind::For => "for",
                    ControlConditionKind::Switch => "switch",
                };
                let extra = Self::missing(missing);
                self.emit(context, "branch", op, &CType::VOID, inputs, span, &extra)
            }
            EvaluationPurpose::IndirectDispatch { .. } => {
                let extra = Self::missing(missing);
                self.emit(
                    context,
                    "branch",
                    "goto",
                    &CType::VOID,
                    inputs,
                    span,
                    &extra,
                )
            }
            EvaluationPurpose::Discard { .. } | EvaluationPurpose::ForClause { .. } => {
                // The value has no language-level consumer; its effects are
                // already operations. Stand for it by its producer.
                return value;
            }
        })
    }

    fn unknown_root(&mut self, root: &DeclinedRoot) {
        let context = Context {
            block: root
                .cfg_node
                .map_or_else(String::new, |node| node.to_string()),
            root: String::new(),
            guarded_by: String::new(),
        };
        self.emit(
            &context,
            "unknown",
            purpose_name(&root.purpose),
            &CType::UNKNOWN,
            &[],
            root.expression,
            &[("reason", root.reason.name().to_owned())],
        );
    }
}

fn purpose_name(purpose: &EvaluationPurpose) -> &'static str {
    match purpose {
        EvaluationPurpose::Initialize { .. } => "initialize",
        EvaluationPurpose::Return { .. } => "return",
        EvaluationPurpose::Discard { .. } => "discard",
        EvaluationPurpose::Control { .. } => "control",
        EvaluationPurpose::ForClause { .. } => "for_clause",
        EvaluationPurpose::IndirectDispatch { .. } => "indirect_dispatch",
    }
}

/// How many of a projected load's or store's plan inputs are its address
/// operands: the base's evaluated values plus an element index.
fn base_input_count(base: &ProjectionBaseOperand, access: &ProjectedAccess) -> usize {
    let mut count = base.scalar_count();
    if matches!(access, ProjectedAccess::Element { .. }) {
        count += 1;
    }
    count
}

/// The operator a projected access is spelled with.
fn access_operator(access: &ProjectedAccess) -> &'static str {
    match access {
        ProjectedAccess::Dereference => "*",
        ProjectedAccess::Field { .. } => ".",
        ProjectedAccess::Element { .. } => "[]",
    }
}

/// `access`, `member`, and the named object a projection bottoms out in.
fn projection_attributes(
    text: &str,
    base: &ProjectionBaseOperand,
    access: &ProjectedAccess,
) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    match access {
        ProjectedAccess::Dereference => out.push(("access", "deref".to_owned())),
        ProjectedAccess::Field { member, .. } => {
            out.push(("access", "member".to_owned()));
            out.push(("member", member.clone()));
        }
        ProjectedAccess::Element { .. } => out.push(("access", "element".to_owned())),
    }
    let mut current = base;
    loop {
        match current {
            ProjectionBaseOperand::Value(_) => break,
            ProjectionBaseOperand::Place { declaration, .. } => {
                let name = text
                    .get(declaration.lo as usize..declaration.hi as usize)
                    .unwrap_or_default()
                    .to_owned();
                out.push(("name", name));
                out.push(("declared", format!("{}:{}", declaration.lo, declaration.hi)));
                break;
            }
            ProjectionBaseOperand::Projected { base, .. } => current = base,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::csource::export::{export, Repr};
    use crate::syntax::graph_export::{to_json, ExportNode, GraphView};

    fn ops(source: &str) -> GraphView {
        let views = export(source, Repr::Ops).into_parts().0;
        assert_eq!(views.len(), 1, "one function in {source:?}");
        views.into_iter().next().expect("one view")
    }

    fn attr<'a>(node: &'a ExportNode, key: &str) -> &'a str {
        node.attrs
            .iter()
            .find(|(candidate, _)| candidate == key)
            .map(|(_, value)| value.as_str())
            .unwrap_or_else(|| panic!("node {} has no {key}", node.id))
    }

    /// `(kind, op, type, inputs)` per node, the shape most assertions read.
    fn shape(view: &GraphView) -> Vec<(String, String, String, String)> {
        view.nodes
            .iter()
            .map(|node| {
                (
                    attr(node, "kind").to_owned(),
                    attr(node, "op").to_owned(),
                    attr(node, "type").to_owned(),
                    attr(node, "inputs").to_owned(),
                )
            })
            .collect()
    }

    fn row(kind: &str, op: &str, ty: &str, inputs: &str) -> (String, String, String, String) {
        (
            kind.to_owned(),
            op.to_owned(),
            ty.to_owned(),
            inputs.to_owned(),
        )
    }

    #[test]
    fn compound_assignment_shows_promotion_arithmetic_and_assignment_conversion() {
        // The example the export exists for: the add happens in `int`, and
        // the value goes back to `unsigned short` by assignment.
        let view = ops("void f(void) { unsigned short s = 3; s += 2; }");
        assert_eq!(
            shape(&view),
            [
                row("const", "3", "int", ""),
                row("convert", "assignment", "unsigned short", "0"),
                row("store", "=", "unsigned short", "1"),
                row("load", "s", "unsigned short", ""),
                row("const", "2", "int", ""),
                row("convert", "promotion", "int", "3"),
                row("binary", "+", "int", "5,4"),
                row("convert", "assignment", "unsigned short", "6"),
                row("store", "=", "unsigned short", "7"),
            ]
        );
        let promotion = &view.nodes[5];
        assert_eq!(attr(promotion, "from"), "unsigned short");
        assert_eq!(attr(promotion, "to"), "int");
        assert_eq!(attr(&view.nodes[6], "operand_type"), "int");
        assert_eq!(attr(&view.nodes[8], "name"), "s");
        assert_eq!(attr(&view.nodes[8], "access"), "scalar");
        assert_eq!(
            attr(&view.nodes[3], "declared"),
            attr(&view.nodes[8], "declared")
        );
    }

    #[test]
    fn usual_arithmetic_conversion_widens_the_signed_operand_of_a_comparison() {
        // `int len` against `unsigned long n`: §6.3.1.8 converts `len`.
        let view = ops("int g(int len, unsigned long n) { if (len < n) { return 1; } return 0; }");
        let rows = shape(&view);
        assert_eq!(
            &rows[..4],
            [
                row("load", "len", "int", ""),
                row("load", "n", "unsigned long", ""),
                row("convert", "usual_arithmetic", "unsigned long", "0"),
                row("compare", "<", "int", "2,1"),
            ]
        );
        assert_eq!(attr(&view.nodes[3], "operand_type"), "unsigned long");
        assert_eq!(rows[4], row("branch", "if", "void", "3"));
        // The `return 1` converts `int` to nothing: the function returns int.
        assert!(rows
            .iter()
            .all(|(kind, op, _, _)| !(kind == "convert" && op == "assignment")));
    }

    #[test]
    fn promotion_then_usual_arithmetic_are_two_conversions_when_both_apply() {
        // `unsigned short` meeting `long`: promote to `int`, then convert to
        // the common `long`. Both steps are written because C names both.
        let view = ops("long f(unsigned short a, long b) { return a + b; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "a", "unsigned short", ""),
                row("load", "b", "long", ""),
                row("convert", "promotion", "int", "0"),
                row("convert", "usual_arithmetic", "long", "2"),
                row("binary", "+", "long", "3,1"),
                row("return", "return", "void", "4"),
            ]
        );
    }

    #[test]
    fn assignment_conversion_narrows_an_initializer_and_widens_a_return() {
        let view = ops("unsigned short h(unsigned int n) { unsigned short len = n; return len; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "n", "unsigned int", ""),
                row("convert", "assignment", "unsigned short", "0"),
                row("store", "=", "unsigned short", "1"),
                row("load", "len", "unsigned short", ""),
                row("return", "return", "void", "3"),
            ]
        );
        let widened = ops("unsigned int f(void) { return 1; }");
        assert_eq!(
            shape(&widened),
            [
                row("const", "1", "int", ""),
                row("convert", "assignment", "unsigned int", "0"),
                row("return", "return", "void", "1"),
            ]
        );
        let pointer = ops("static const char *name(char *p) { return p; }");
        assert_eq!(
            shape(&pointer),
            [
                row("load", "p", "char *", ""),
                row("convert", "assignment", "const char *", "0"),
                row("return", "return", "void", "1"),
            ]
        );
    }

    #[test]
    fn a_cast_is_an_unknown_root_with_its_reason_never_a_silent_gap() {
        // The lowering has no cast rule, so a `cast` conversion cannot be
        // exported yet; what is exported is that the root was declined.
        let view = ops("int c(int n) { return (unsigned char) n; }");
        assert_eq!(shape(&view), [row("unknown", "return", "unknown", "")]);
        assert_eq!(attr(&view.nodes[0], "reason"), "unsupported_form");
        assert_eq!(attr(&view.nodes[0], "block"), "2");
        assert_eq!(attr(&view.nodes[0], "span"), "22:39");
    }

    #[test]
    fn every_declined_root_kind_is_an_unknown_operation_in_source_position() {
        let view = ops(
            "int glob; int f(int n) { int a[2] = {1, 2}; int s = sizeof(int) + n; \
             int u = n++ + n; return glob + a[0]; }",
        );
        let unknowns: Vec<(&str, &str)> = view
            .nodes
            .iter()
            .filter(|node| attr(node, "kind") == "unknown")
            .map(|node| (attr(node, "op"), attr(node, "reason")))
            .collect();
        assert_eq!(
            unknowns,
            [
                ("initialize", "braced_initializer"),
                ("initialize", "unsupported_form"),
                ("initialize", "unsequenced_effects"),
                ("return", "unsupported_form"),
            ]
        );
        // In source order, so a reader walking the list sees each unknown
        // where the expression sits.
        let spans: Vec<u32> = view
            .nodes
            .iter()
            .map(|node| {
                attr(node, "span")
                    .split(':')
                    .next()
                    .unwrap()
                    .parse()
                    .unwrap()
            })
            .collect();
        let mut sorted = spans.clone();
        sorted.sort_unstable();
        assert_eq!(spans, sorted);
    }

    #[test]
    fn consumer_shape_length_overflow_check() {
        let view = ops(
            "unsigned int f(unsigned int hdr, unsigned int body, unsigned int dst_len) \
             { unsigned int total = hdr + body; if (total > dst_len) { return 1; } return 0; }",
        );
        let rows = shape(&view);
        assert_eq!(
            &rows[..8],
            [
                row("load", "hdr", "unsigned int", ""),
                row("load", "body", "unsigned int", ""),
                row("binary", "+", "unsigned int", "0,1"),
                row("store", "=", "unsigned int", "2"),
                row("load", "total", "unsigned int", ""),
                row("load", "dst_len", "unsigned int", ""),
                row("compare", ">", "int", "4,5"),
                row("branch", "if", "void", "6"),
            ]
        );
        assert_eq!(attr(&view.nodes[3], "name"), "total");
        assert_eq!(attr(&view.nodes[3], "block"), "2");
        assert_eq!(attr(&view.nodes[7], "block"), "3");
        assert_eq!(attr(&view.nodes[7], "root"), "1");
        assert_eq!(attr(&view.nodes[7], "line"), "1");
        assert_eq!(attr(&view.nodes[7], "column"), "114");
    }

    #[test]
    fn consumer_shape_shift_of_an_unsigned_literal_converts_nothing() {
        let view = ops("unsigned int k(unsigned int bits) { return 1u << bits; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "bits", "unsigned int", ""),
                row("const", "1u", "unsigned int", ""),
                row("binary", "<<", "unsigned int", "1,0"),
                row("return", "return", "void", "2"),
            ]
        );
        // A narrow shift count is promoted on its own, not to the left type.
        let narrow =
            ops("unsigned long k(unsigned long v, unsigned char bits) { return v << bits; }");
        assert_eq!(
            shape(&narrow),
            [
                row("load", "v", "unsigned long", ""),
                row("load", "bits", "unsigned char", ""),
                row("convert", "promotion", "int", "1"),
                row("binary", "<<", "unsigned long", "0,2"),
                row("return", "return", "void", "3"),
            ]
        );
    }

    #[test]
    fn consumer_shape_conditional_abs_guards_each_arm_on_the_compare() {
        let view = ops("int m(int n) { return n < 0 ? -n : n; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "n", "int", ""),
                row("const", "0", "int", ""),
                row("compare", "<", "int", "0,1"),
                row("load", "n", "int", ""),
                row("unary", "-", "int", "3"),
                row("load", "n", "int", ""),
                row("select", "?:", "int", "2,4,5"),
                row("return", "return", "void", "6"),
            ]
        );
        assert_eq!(attr(&view.nodes[3], "guarded_by"), "2:true");
        assert_eq!(attr(&view.nodes[4], "guarded_by"), "2:true");
        assert_eq!(attr(&view.nodes[5], "guarded_by"), "2:false");
        assert_eq!(attr(&view.nodes[6], "guarded_by"), "");
        // The arms sit in their own CFG nodes; the select in the join.
        assert_ne!(attr(&view.nodes[3], "block"), attr(&view.nodes[5], "block"));
        assert_ne!(attr(&view.nodes[6], "block"), attr(&view.nodes[3], "block"));
    }

    #[test]
    fn short_circuit_carries_its_bypass_constant_once_and_guards_the_right() {
        let view = ops("int sc(int a, int b) { int r = a && b; return r; }");
        assert_eq!(
            shape(&view)[..5],
            [
                row("load", "a", "int", ""),
                row("load", "b", "int", ""),
                row("const", "0", "int", ""),
                row("binary", "&&", "int", "0,1,2"),
                row("store", "=", "int", "3"),
            ]
        );
        assert_eq!(attr(&view.nodes[1], "guarded_by"), "0:true");
        assert_eq!(attr(&view.nodes[2], "guarded_by"), "");
    }

    #[test]
    fn a_flat_chain_types_each_prefix_from_the_typer() {
        let view = ops("long ch(unsigned char a, short b, long c) { return a + b - c; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "a", "unsigned char", ""),
                row("load", "b", "short", ""),
                row("convert", "promotion", "int", "0"),
                row("convert", "promotion", "int", "1"),
                row("binary", "+", "int", "2,3"),
                row("load", "c", "long", ""),
                row("convert", "usual_arithmetic", "long", "4"),
                row("binary", "-", "long", "6,5"),
                row("return", "return", "void", "7"),
            ]
        );
    }

    #[test]
    fn increments_expand_to_their_arithmetic_and_postfix_yields_the_prior_value() {
        let view = ops("int neg(int n) { int x = -n; x++; return x--; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "n", "int", ""),
                row("unary", "-", "int", "0"),
                row("store", "=", "int", "1"),
                row("load", "x", "int", ""),
                row("const", "1", "int", ""),
                row("binary", "+", "int", "3,4"),
                row("store", "=", "int", "5"),
                row("load", "x", "int", ""),
                row("const", "1", "int", ""),
                row("binary", "-", "int", "7,8"),
                row("store", "=", "int", "9"),
                row("return", "return", "void", "7"),
            ]
        );
        assert_eq!(attr(&view.nodes[4], "span"), "30:32");
        let narrow = ops("void f(void) { unsigned char c = 0; c++; }");
        assert_eq!(
            shape(&narrow)[3..],
            [
                row("load", "c", "unsigned char", ""),
                row("const", "1", "int", ""),
                row("convert", "promotion", "int", "3"),
                row("binary", "+", "int", "5,4"),
                row("convert", "assignment", "unsigned char", "6"),
                row("store", "=", "unsigned char", "7"),
            ]
        );
    }

    #[test]
    fn projected_stores_and_loads_carry_the_access_and_the_element_conversion() {
        let view =
            ops("int arr(unsigned char *dst, unsigned int i) { dst[i] = 7; return dst[i]; }");
        assert_eq!(
            shape(&view),
            [
                row("load", "dst", "unsigned char *", ""),
                row("load", "i", "unsigned int", ""),
                row("const", "7", "int", ""),
                row("convert", "assignment", "unsigned char", "2"),
                row("store", "=", "unsigned char", "0,1,3"),
                row("load", "dst", "unsigned char *", ""),
                row("load", "i", "unsigned int", ""),
                row("load", "[]", "unsigned char", "5,6"),
                row("convert", "assignment", "int", "7"),
                row("return", "return", "void", "8"),
            ]
        );
        assert_eq!(attr(&view.nodes[4], "access"), "element");
        assert_eq!(attr(&view.nodes[7], "access"), "element");
        let member =
            ops("struct s { int f; }; int g(struct s *p, struct s v) { p->f = v.f; return p->f; }");
        let rows = shape(&member);
        assert_eq!(rows[1], row("load", ".", "int", ""));
        assert_eq!(attr(&member.nodes[1], "member"), "f");
        assert_eq!(attr(&member.nodes[1], "name"), "v");
        assert_eq!(rows[2], row("store", "=", "int", "0,1"));
        assert_eq!(attr(&member.nodes[2], "member"), "f");
    }

    #[test]
    fn calls_and_header_typedefs_are_unknown_where_the_typer_is() {
        let view = ops("int f(size_t n) { unsigned short s = g(n); return s; }");
        let rows = shape(&view);
        assert_eq!(rows[0], row("load", "n", "unknown", ""));
        assert_eq!(rows[1], row("call", "g", "unknown", "0"));
        assert_eq!(attr(&view.nodes[1], "argument_conversions"), "unknown");
        // The assignment conversion is real even when its source is not known.
        assert_eq!(rows[2], row("convert", "assignment", "unsigned short", "1"));
        assert_eq!(attr(&view.nodes[2], "from"), "unknown");
    }

    #[test]
    fn value_edges_mirror_the_inputs_attribute() {
        let view = ops("int f(int a, int b) { return a * b + 1; }");
        let mut from_attrs = Vec::new();
        for node in &view.nodes {
            for (index, input) in attr(node, "inputs")
                .split(',')
                .filter(|input| !input.is_empty())
                .enumerate()
            {
                from_attrs.push((input.parse::<u32>().unwrap(), node.id, index.to_string()));
            }
        }
        let from_edges: Vec<(u32, u32, String)> = view
            .edges
            .iter()
            .map(|edge| (edge.src, edge.dst, edge.label.clone()))
            .collect();
        assert_eq!(from_edges, from_attrs);
        assert!(
            view.edges.iter().all(|edge| edge.src < edge.dst),
            "inputs precede consumers"
        );
    }

    #[test]
    fn the_export_is_byte_identical_across_runs() {
        let source = "unsigned short h(unsigned int n, int c) { unsigned short len = n; \
                      len += c ? 1 : 2; if (len && n) { return len; } return 0; }";
        let first = to_json(&ops(source));
        let second = to_json(&ops(source));
        assert_eq!(first, second);
        assert!(first.contains("\"kind\": \"convert\""));
    }

    #[test]
    fn the_representation_parses_under_its_names() {
        for name in [
            "ops",
            "operations",
            "typed-operations",
            "typed_operations",
            "OPS",
        ] {
            assert_eq!(Repr::parse(name), Some(Repr::Ops), "{name}");
        }
        assert_eq!(Repr::Ops.name(), "ops");
        assert_eq!(Repr::Ops.graph_kind(), "typed_operations");
    }
}
