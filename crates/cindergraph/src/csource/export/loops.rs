//! Loop metadata for a bounded unroller: what a `for`, `while` or `do`
//! header says about its own trip count.
//!
//! A consumer that refuses loops needs only the back edge, which the CFG
//! export already flags. A consumer that *unrolls* them needs to know whether
//! the header decides the trip count by itself --- `for (i = 0; i < 16; i++)`
//! runs sixteen times whatever the function's inputs are, `for (i = 0;
//! i <= n; i++)` runs as a function of the parameter `n`, and `while (u > 0)
//! { u--; }` runs as a function of a value the loop text does not fix. This
//! module reads exactly that off the syntax tree.
//!
//! # Contract
//!
//! Every fact here is decided from the loop's own text plus one lexical
//! question (is a name a function parameter). Where a rule does not apply
//! exactly, the answer is the conservative one: `bound_kind` is `runtime` and
//! the derived attributes are absent. Nothing is guessed from a name, a
//! comment or a habit; a loop the module cannot classify is reported as a
//! loop it cannot classify, which is the answer an unroller must refuse on.
//!
//! * `bound_kind = constant` says: the induction variable starts at an
//!   integer literal (the `for` initializer), the condition is one relational
//!   comparison of that variable against an integer literal, the step is
//!   `++`, `--`, `+= literal`, `-= literal` or `v = v ± literal`, and nothing
//!   else in the loop assigns the variable or takes its address anywhere in
//!   the function. A `while` or `do` has no initializer, so it is never
//!   `constant`: the trip count of `while (u > 0) { u--; }` is whatever `u`
//!   was on entry.
//! * `bound_kind = parameter` is the same shape with the literal bound
//!   replaced by a function parameter that the loop (initializer, condition,
//!   step, body) never assigns and whose address is never taken.
//! * `induction` and `step` are read from the `for` step clause, or for a
//!   `while`/`do` from the one top-level statement of the body that steps a
//!   variable the condition reads (`while (u > 0) { u--; steps++; }` names
//!   `u`). Two such statements name nothing.
//! * `bound_kind = none` is a loop with no condition (`for (;;)`).
//! * `bound_kind = runtime` is everything else, including a `for` whose
//!   initializer is not a literal, a condition with `&&`, a compound step, a
//!   body that writes the induction variable, or a comparison that is not a
//!   bound (`==`).

use std::collections::BTreeMap;

use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::syntax::ids::{NodeId, Span};

/// The exported classification of one loop's bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundKind {
    /// Trip count fixed by the loop text alone.
    Constant,
    /// Trip count a function of one unassigned function parameter.
    Parameter,
    /// Not decided by the loop text; an unroller must refuse or ask elsewhere.
    Runtime,
    /// No condition at all.
    None,
}

impl BoundKind {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            BoundKind::Constant => "constant",
            BoundKind::Parameter => "parameter",
            BoundKind::Runtime => "runtime",
            BoundKind::None => "none",
        }
    }
}

/// What the export publishes about one loop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoopFacts {
    /// `for`, `while` or `do_while`.
    pub(crate) kind: &'static str,
    /// The span the CFG gives this loop's `loop_header`: the condition's, or
    /// the whole statement's when there is no condition.
    pub(crate) header_span: Span,
    /// The condition's source text, trimmed; absent when there is none.
    pub(crate) bound_expr: Option<String>,
    /// The variable the recognized step advances.
    pub(crate) induction: Option<String>,
    /// `+1`, `-1`, `+N`, `-N` for the recognized step.
    pub(crate) step: Option<String>,
    /// The literal the `for` initializer assigns the induction variable.
    pub(crate) init_value: Option<String>,
    pub(crate) bound_kind: BoundKind,
    /// The literal the induction variable is compared against, when
    /// `bound_kind` is `constant`.
    pub(crate) bound_value: Option<String>,
}

impl LoopFacts {
    /// The attributes as the export writes them, in a fixed order.
    pub(crate) fn attributes(&self) -> Vec<(&'static str, String)> {
        let mut out = vec![
            ("loop_kind", self.kind.to_owned()),
            ("bound_kind", self.bound_kind.name().to_owned()),
        ];
        if let Some(expr) = &self.bound_expr {
            out.push(("bound_expr", expr.clone()));
        }
        if let Some(name) = &self.induction {
            out.push(("induction", name.clone()));
        }
        if let Some(step) = &self.step {
            out.push(("step", step.clone()));
        }
        if let Some(init) = &self.init_value {
            out.push(("init_value", init.clone()));
        }
        if let Some(value) = &self.bound_value {
            out.push(("bound_value", value.clone()));
        }
        out
    }
}

/// Answers "is `name`, referenced at byte `offset`, a function parameter?".
///
/// The tree-only export has no declaration resolution and passes `None`; a
/// name bound is then `runtime`, never a guess.
pub(crate) type IsParameter<'a> = &'a dyn Fn(&str, u32) -> bool;

/// The loop statements under `root`, keyed by the span their CFG
/// `loop_header` carries, so a CFG node joins its metadata by span.
pub(crate) fn loops_by_header_span(
    tree: &Tree,
    root: NodeId,
    token_spans: &[Span],
    text: &str,
    is_parameter: Option<IsParameter<'_>>,
) -> BTreeMap<Span, LoopFacts> {
    let arena = tree.arena();
    let mut out = BTreeMap::new();
    for node in arena.preorder(root) {
        let Some(tag) = arena.tag(node).and_then(NodeTag::from_u16) else {
            continue;
        };
        if let Some(facts) = loop_facts(tree, root, node, tag, token_spans, text, is_parameter) {
            out.insert(facts.header_span, facts);
        }
    }
    out
}

/// The facts for the loop statement `node` (tagged `tag`) inside the function
/// rooted at `root`, or `None` when `node` is not a loop.
pub(crate) fn loop_facts(
    tree: &Tree,
    root: NodeId,
    node: NodeId,
    tag: NodeTag,
    token_spans: &[Span],
    text: &str,
    is_parameter: Option<IsParameter<'_>>,
) -> Option<LoopFacts> {
    let reader = Reader {
        tree,
        token_spans,
        text,
    };
    let kind = match tag {
        NodeTag::ForStmt => "for",
        NodeTag::WhileStmt => "while",
        NodeTag::DoWhileStmt => "do_while",
        _ => return None,
    };
    let statement_span = reader.span(node).unwrap_or_default();

    // The clauses. A `for` keeps its three header clauses as nodes; `while`
    // and `do` have the condition as their one expression child and the body
    // as their one statement child.
    let (init, cond, step, body) = if tag == NodeTag::ForStmt {
        let clause = |wanted: NodeTag| {
            reader
                .children(node)
                .into_iter()
                .find(|child| reader.tag(*child) == Some(wanted))
        };
        let single_child = |clause: Option<NodeId>| {
            clause.and_then(|clause| {
                let children = reader.children(clause);
                (children.len() == 1).then(|| children[0])
            })
        };
        (
            clause(NodeTag::ForInit),
            single_child(clause(NodeTag::ForCond)),
            single_child(clause(NodeTag::ForStep)),
            reader
                .children(node)
                .into_iter()
                .find(|child| reader.tag(*child).is_some_and(is_body)),
        )
    } else {
        let children = reader.children(node);
        (
            None,
            children
                .iter()
                .copied()
                .find(|child| reader.tag(*child).is_some_and(NodeTag::is_expression)),
            None,
            children
                .iter()
                .copied()
                .find(|child| reader.tag(*child).is_some_and(is_body)),
        )
    };

    let header_span = cond
        .and_then(|cond| reader.span(cond))
        .unwrap_or(statement_span);
    let bound_expr = cond.map(|cond| reader.text_of(cond).trim().to_owned());

    // The step: the `for` step clause, or for `while`/`do` exactly one
    // top-level statement of the body that has the step shape.
    let (induction, step, step_statement) = match tag {
        NodeTag::ForStmt => match step.and_then(|step| reader.step_of(step)) {
            Some((name, step)) => (Some(name), Some(step), None),
            None => (None, None, None),
        },
        _ => {
            let statements: Vec<NodeId> = match body {
                Some(body) if reader.tag(body) == Some(NodeTag::CompoundStmt) => {
                    reader.children(body)
                }
                Some(body) => vec![body],
                None => Vec::new(),
            };
            let mut found: Vec<(String, String, NodeId)> = Vec::new();
            for statement in statements {
                if reader.tag(statement) != Some(NodeTag::ExprStmt) {
                    continue;
                }
                let children = reader.children(statement);
                if children.len() != 1 {
                    continue;
                }
                if let Some((name, step)) = reader.step_of(children[0]) {
                    found.push((name, step, statement));
                }
            }
            // The induction variable is the stepped one the condition reads;
            // `while (u > 0) { u--; steps++; }` steps two variables and only
            // `u` is in the test. Two steps of the same variable, or two
            // stepped variables both in the test, decide nothing.
            let in_condition: Vec<String> =
                cond.map(|cond| reader.names_in(cond)).unwrap_or_default();
            found.retain(|(name, _, _)| in_condition.contains(name));
            match found.as_slice() {
                [(name, step, statement)] => {
                    (Some(name.clone()), Some(step.clone()), Some(*statement))
                }
                _ => (None, None, None),
            }
        }
    };

    let init_value = match (tag, init, &induction) {
        (NodeTag::ForStmt, Some(init), Some(name)) => reader.init_literal(init, name),
        _ => None,
    };

    let mut facts = LoopFacts {
        kind,
        header_span,
        bound_expr,
        induction,
        step,
        init_value,
        bound_kind: BoundKind::Runtime,
        bound_value: None,
    };
    let Some(cond) = cond else {
        facts.bound_kind = BoundKind::None;
        return Some(facts);
    };
    let Some(name) = facts.induction.clone() else {
        return Some(facts);
    };
    // A `while`/`do` never fixes the start value; a `for` must.
    if tag != NodeTag::ForStmt || facts.init_value.is_none() {
        return Some(facts);
    }
    let Some(bound) = reader.bound_of(cond, &name) else {
        return Some(facts);
    };

    // Writes to the induction variable other than the initializer and the
    // recognized step, or its address taken anywhere in the function, make
    // the trip count something the header does not decide.
    let mut writes = Writes::default();
    if let Some(body) = body {
        reader.collect_writes(body, &mut writes, step_statement);
    }
    reader.collect_writes(cond, &mut writes, None);
    if writes.assigned.contains(&name) || reader.address_taken(root, &name) {
        return Some(facts);
    }

    match bound {
        Bound::Literal(value) => {
            facts.bound_kind = BoundKind::Constant;
            facts.bound_value = Some(value);
        }
        Bound::Name(other, offset) => {
            let parameter = is_parameter.is_some_and(|is_parameter| is_parameter(&other, offset));
            // The parameter must be untouched by the whole loop: initializer
            // and step included, since `for (i = 0; i < n; i++, n--)` is not
            // bounded by `n`.
            let mut loop_writes = Writes::default();
            reader.collect_writes(node, &mut loop_writes, None);
            if parameter
                && !loop_writes.assigned.contains(&other)
                && !reader.address_taken(root, &other)
            {
                facts.bound_kind = BoundKind::Parameter;
            }
        }
    }
    Some(facts)
}

/// Whether `tag` is a loop body rather than a header clause or condition;
/// mirrors the CFG builder's rule so the two agree on which child is the body.
fn is_body(tag: NodeTag) -> bool {
    match tag {
        NodeTag::ForInit | NodeTag::ForCond | NodeTag::ForStep => false,
        NodeTag::Error => true,
        other => other.is_statement(),
    }
}

/// The other side of the induction variable's comparison.
enum Bound {
    Literal(String),
    /// A name and the byte offset it is referenced at.
    Name(String, u32),
}

/// Names written inside a subtree.
#[derive(Default)]
struct Writes {
    assigned: Vec<String>,
}

/// Tree access with the text and token spans in hand.
struct Reader<'a> {
    tree: &'a Tree,
    token_spans: &'a [Span],
    text: &'a str,
}

impl Reader<'_> {
    fn tag(&self, node: NodeId) -> Option<NodeTag> {
        self.tree.arena().tag(node).and_then(NodeTag::from_u16)
    }

    fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.tree.arena().children_iter(node).collect()
    }

    fn span(&self, node: NodeId) -> Option<Span> {
        self.tree.arena().span(node, self.token_spans)
    }

    fn text_of(&self, node: NodeId) -> &str {
        self.span(node)
            .and_then(|span| self.text.get(span.lo as usize..span.hi as usize))
            .unwrap_or("")
    }

    /// The operator token of a unary, postfix-suffix or assignment node.
    fn operator(&self, node: NodeId) -> Option<&str> {
        let arena = self.tree.arena();
        match self.tag(node)? {
            NodeTag::UnaryExpr | NodeTag::IncDecSuffix => {
                let (first, _) = arena.token_extent(node)?;
                let span = self.token_spans.get(first as usize)?;
                self.text.get(span.lo as usize..span.hi as usize)
            }
            NodeTag::AssignExpr | NodeTag::BinaryExpr => {
                let children = self.children(node);
                if children.len() != 2 {
                    return None;
                }
                let (_, left_end) = arena.token_extent(children[0])?;
                let (right_start, _) = arena.token_extent(children[1])?;
                (left_end..right_start).find_map(|raw| {
                    let span = self.token_spans.get(raw as usize)?;
                    self.text.get(span.lo as usize..span.hi as usize)
                })
            }
            _ => None,
        }
    }

    /// `node` as a bare name reference, through parentheses.
    fn name_ref(&self, node: NodeId) -> Option<String> {
        match self.tag(node)? {
            NodeTag::NameRef => Some(self.text_of(node).to_owned()),
            NodeTag::ParenExpr => {
                let children = self.children(node);
                (children.len() == 1)
                    .then(|| self.name_ref(children[0]))
                    .flatten()
            }
            _ => None,
        }
    }

    /// `node` as an integer literal, with any suffix removed.
    fn int_literal(&self, node: NodeId) -> Option<String> {
        match self.tag(node)? {
            NodeTag::Literal => integer_text(self.text_of(node)),
            NodeTag::ParenExpr => {
                let children = self.children(node);
                (children.len() == 1)
                    .then(|| self.int_literal(children[0]))
                    .flatten()
            }
            _ => None,
        }
    }

    /// `(variable, step)` when `node` is `v++`, `v--`, `++v`, `--v`,
    /// `v += N`, `v -= N`, `v = v + N` or `v = v - N`.
    fn step_of(&self, node: NodeId) -> Option<(String, String)> {
        let children = self.children(node);
        match self.tag(node)? {
            NodeTag::PostfixExpr => {
                if children.len() != 2 || self.tag(children[1]) != Some(NodeTag::IncDecSuffix) {
                    return None;
                }
                let name = self.name_ref(children[0])?;
                let step = match self.operator(children[1])? {
                    "++" => "+1",
                    "--" => "-1",
                    _ => return None,
                };
                Some((name, step.to_owned()))
            }
            NodeTag::UnaryExpr => {
                if children.len() != 1 {
                    return None;
                }
                let step = match self.operator(node)? {
                    "++" => "+1",
                    "--" => "-1",
                    _ => return None,
                };
                let name = self.name_ref(children[0])?;
                Some((name, step.to_owned()))
            }
            NodeTag::AssignExpr => {
                if children.len() != 2 {
                    return None;
                }
                let name = self.name_ref(children[0])?;
                match self.operator(node)? {
                    "+=" => Some((name, format!("+{}", self.int_literal(children[1])?))),
                    "-=" => Some((name, format!("-{}", self.int_literal(children[1])?))),
                    "=" => {
                        // `v = v + N` / `v = v - N`: one binary operator, the
                        // variable on the left, a literal on the right.
                        let rhs = children[1];
                        if self.tag(rhs) != Some(NodeTag::BinaryExpr) {
                            return None;
                        }
                        let operands = self.children(rhs);
                        if operands.len() != 2 || self.name_ref(operands[0])? != name {
                            return None;
                        }
                        let sign = match self.operator(rhs)? {
                            "+" => "+",
                            "-" => "-",
                            _ => return None,
                        };
                        Some((name, format!("{sign}{}", self.int_literal(operands[1])?)))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// The literal a `for` initializer gives `name`: `int i = 0` (one
    /// declarator with a literal initializer) or `i = 0`.
    fn init_literal(&self, init: NodeId, name: &str) -> Option<String> {
        let children = self.children(init);
        if children.len() != 1 {
            return None;
        }
        let child = children[0];
        match self.tag(child)? {
            NodeTag::AssignExpr => {
                let parts = self.children(child);
                if parts.len() != 2
                    || self.operator(child)? != "="
                    || self.name_ref(parts[0])? != name
                {
                    return None;
                }
                self.int_literal(parts[1])
            }
            NodeTag::Decl => {
                let declarators: Vec<NodeId> = self
                    .children(child)
                    .into_iter()
                    .filter(|part| self.tag(*part) == Some(NodeTag::Declarator))
                    .collect();
                if declarators.len() != 1 {
                    return None;
                }
                let declared = self.text_of(declarators[0]).trim();
                if declared != name {
                    return None;
                }
                let initializer = self
                    .children(child)
                    .into_iter()
                    .find(|part| self.tag(*part) == Some(NodeTag::Initializer))?;
                let values = self.children(initializer);
                if values.len() != 1 {
                    return None;
                }
                self.int_literal(values[0])
            }
            _ => None,
        }
    }

    /// The bound `cond` compares `name` against: one relational operator
    /// with `name` on one side and a literal or a name on the other.
    fn bound_of(&self, cond: NodeId, name: &str) -> Option<Bound> {
        let cond = self.unparen(cond);
        if self.tag(cond) != Some(NodeTag::BinaryExpr) {
            return None;
        }
        let operands = self.children(cond);
        if operands.len() != 2 {
            return None;
        }
        if !matches!(self.operator(cond)?, "<" | "<=" | ">" | ">=" | "!=") {
            return None;
        }
        let other = if self.name_ref(operands[0]).as_deref() == Some(name) {
            operands[1]
        } else if self.name_ref(operands[1]).as_deref() == Some(name) {
            operands[0]
        } else {
            return None;
        };
        if let Some(literal) = self.int_literal(other) {
            return Some(Bound::Literal(literal));
        }
        let bound_name = self.name_ref(other)?;
        if bound_name == name {
            return None;
        }
        let offset = self.span(self.unparen(other))?.lo;
        Some(Bound::Name(bound_name, offset))
    }

    fn unparen(&self, mut node: NodeId) -> NodeId {
        while self.tag(node) == Some(NodeTag::ParenExpr) {
            let children = self.children(node);
            if children.len() != 1 {
                break;
            }
            node = children[0];
        }
        node
    }

    /// Every name a subtree writes: the target of an assignment, an
    /// increment or a decrement, or a declarator (a redeclaration shadows).
    /// `skip` is a statement whose own write is expected and not counted.
    fn collect_writes(&self, node: NodeId, out: &mut Writes, skip: Option<NodeId>) {
        if Some(node) == skip {
            return;
        }
        let children = self.children(node);
        match self.tag(node) {
            Some(NodeTag::AssignExpr) => {
                if let Some(target) = children.first().and_then(|target| self.name_ref(*target)) {
                    out.assigned.push(target);
                }
            }
            Some(NodeTag::PostfixExpr) => {
                if children.len() == 2 && self.tag(children[1]) == Some(NodeTag::IncDecSuffix) {
                    if let Some(target) = self.name_ref(children[0]) {
                        out.assigned.push(target);
                    }
                }
            }
            Some(NodeTag::UnaryExpr) => {
                if matches!(self.operator(node), Some("++" | "--")) {
                    if let Some(target) = children.first().and_then(|target| self.name_ref(*target))
                    {
                        out.assigned.push(target);
                    }
                }
            }
            Some(NodeTag::Declarator) => {
                out.assigned.push(self.text_of(node).trim().to_owned());
            }
            _ => {}
        }
        for child in children {
            self.collect_writes(child, out, skip);
        }
    }

    /// Every name referenced under `node`, in preorder, duplicates kept.
    fn names_in(&self, node: NodeId) -> Vec<String> {
        self.tree
            .arena()
            .preorder(node)
            .filter(|node| self.tag(*node) == Some(NodeTag::NameRef))
            .map(|node| self.text_of(node).to_owned())
            .collect()
    }

    /// Whether `&name` appears anywhere under `root`.
    fn address_taken(&self, root: NodeId, name: &str) -> bool {
        self.tree.arena().preorder(root).any(|node| {
            self.tag(node) == Some(NodeTag::UnaryExpr)
                && self.operator(node) == Some("&")
                && self
                    .children(node)
                    .first()
                    .and_then(|operand| self.name_ref(*operand))
                    .as_deref()
                    == Some(name)
        })
    }
}

/// `text` as an integer literal with its suffix removed (`16u` is `16`,
/// `0x10UL` is `0x10`), or `None` for anything that is not one.
fn integer_text(text: &str) -> Option<String> {
    let body = text.trim_end_matches(['u', 'U', 'l', 'L']);
    if body.is_empty() {
        return None;
    }
    let digits = body
        .strip_prefix("0x")
        .or_else(|| body.strip_prefix("0X"))
        .map_or(body, |hex| hex);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if digits == body && !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some(body.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::parse::parse;

    /// The facts of every loop in the first function of `text`, in source
    /// order, with no parameter resolution.
    fn facts(text: &str) -> Vec<LoopFacts> {
        facts_with(text, None)
    }

    fn facts_with(text: &str, is_parameter: Option<IsParameter<'_>>) -> Vec<LoopFacts> {
        let tree = parse(text).into_parts().0;
        let spans = tree.token_spans(text);
        let function = tree.functions(text)[0].node;
        loops_by_header_span(&tree, function, &spans, text, is_parameter)
            .into_values()
            .collect()
    }

    #[test]
    fn a_literal_for_loop_is_constant() {
        let [f] = &facts("void f(void){ int s = 0; for (int i = 0; i < 16; i++) s += i; }")[..]
        else {
            panic!("one loop")
        };
        assert_eq!(f.bound_kind, BoundKind::Constant);
        assert_eq!(f.induction.as_deref(), Some("i"));
        assert_eq!(f.step.as_deref(), Some("+1"));
        assert_eq!(f.init_value.as_deref(), Some("0"));
        assert_eq!(f.bound_value.as_deref(), Some("16"));
        assert_eq!(f.bound_expr.as_deref(), Some("i < 16"));
    }

    #[test]
    fn a_parameter_bound_needs_the_resolver_and_an_untouched_parameter() {
        let text =
            "void f(int n){ int i; for (i = 0; i <= n; i++) {} for (i = 0; i <= n; i++) { n--; } }";
        let unresolved = facts(text);
        assert_eq!(unresolved[0].bound_kind, BoundKind::Runtime);
        let is_n = |name: &str, _: u32| name == "n";
        let resolved = facts_with(text, Some(&is_n));
        assert_eq!(resolved[0].bound_kind, BoundKind::Parameter);
        assert_eq!(resolved[0].bound_value, None);
        assert_eq!(
            resolved[1].bound_kind,
            BoundKind::Runtime,
            "the body writes n"
        );
    }

    #[test]
    fn a_while_loop_names_its_induction_but_stays_runtime() {
        let [f] = &facts("void f(unsigned u){ while (u > 0) { u--; } }")[..] else {
            panic!("one loop")
        };
        assert_eq!(f.kind, "while");
        assert_eq!(f.induction.as_deref(), Some("u"));
        assert_eq!(f.step.as_deref(), Some("-1"));
        assert_eq!(f.bound_kind, BoundKind::Runtime);
        assert_eq!(f.init_value, None);
    }

    #[test]
    fn the_induction_is_the_stepped_variable_the_condition_reads() {
        let all = facts(
            "void f(unsigned u, unsigned v){ while (u > 0) { u--; v++; } \
             while (u > 0) { u--; u++; } while (u > v) { u--; v++; } }",
        );
        assert_eq!(all[0].induction.as_deref(), Some("u"));
        assert_eq!(all[0].step.as_deref(), Some("-1"));
        assert_eq!(all[1].induction, None, "stepped twice");
        assert_eq!(
            all[2].induction, None,
            "both stepped variables are in the test"
        );
    }

    #[test]
    fn a_body_that_writes_the_induction_variable_is_runtime() {
        let [f] =
            &facts("void f(void){ for (int i = 0; i < 16; i++) { if (i == 3) i = 10; } }")[..]
        else {
            panic!("one loop")
        };
        assert_eq!(f.induction.as_deref(), Some("i"));
        assert_eq!(f.bound_kind, BoundKind::Runtime);
        assert_eq!(f.bound_value, None);
    }

    #[test]
    fn an_address_taken_anywhere_in_the_function_is_runtime() {
        let [f] =
            &facts("void g(int *); void f(void){ int i; g(&i); for (i = 0; i < 4; i++) {} }")[..]
        else {
            panic!("one loop")
        };
        assert_eq!(f.bound_kind, BoundKind::Runtime);
    }

    #[test]
    fn every_step_shape_is_read_and_a_compound_step_is_not() {
        let text = "void f(void){ int i; for (i = 0; i < 8; ++i) {} for (i = 0; i < 8; i += 2) {} \
                    for (i = 8; i > 0; i -= 3) {} for (i = 0; i < 8; i = i + 4) {} \
                    for (i = 0; i < 8; i++, i++) {} for (i = 0; i < 0x10u; i++) {} }";
        let all = facts(text);
        let steps: Vec<Option<&str>> = all.iter().map(|f| f.step.as_deref()).collect();
        assert_eq!(
            steps,
            vec![
                Some("+1"),
                Some("+2"),
                Some("-3"),
                Some("+4"),
                None,
                Some("+1")
            ]
        );
        assert_eq!(all[4].bound_kind, BoundKind::Runtime);
        assert_eq!(all[5].bound_value.as_deref(), Some("0x10"));
    }

    #[test]
    fn no_condition_is_none_and_a_compound_condition_is_runtime() {
        let all = facts(
            "void f(int ok){ for (;;) { break; } for (int i = 0; i < 4 && ok; i++) {} while (1) {} }",
        );
        assert_eq!(all[0].bound_kind, BoundKind::None);
        assert_eq!(all[0].bound_expr, None);
        assert_eq!(all[1].bound_kind, BoundKind::Runtime);
        assert_eq!(all[1].bound_expr.as_deref(), Some("i < 4 && ok"));
        assert_eq!(all[2].bound_kind, BoundKind::Runtime);
        assert_eq!(all[2].induction, None);
    }

    #[test]
    fn integer_text_strips_suffixes_and_rejects_non_integers() {
        assert_eq!(integer_text("16u").as_deref(), Some("16"));
        assert_eq!(integer_text("0x10UL").as_deref(), Some("0x10"));
        assert_eq!(integer_text("1.5"), None);
        assert_eq!(integer_text("'a'"), None);
        assert_eq!(integer_text("abc"), None);
        assert_eq!(integer_text("u"), None);
    }
}
