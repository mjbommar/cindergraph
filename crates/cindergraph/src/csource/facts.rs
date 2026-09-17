//! External facts: what a consumer knows about a function that its source
//! does not say.
//!
//! A solver front end lifting a C function needs a pointer parameter's
//! capacity in bytes, a string parameter's length, and a bound for a bounded
//! unroller, and no C text states any of them. This module is the contract
//! decided in `docs/design/external-facts-2026-09-17.md`: one grammar with two
//! front doors, resolved once against the function's parameter list and
//! written on the function's exported nodes.
//!
//! * The **comment form** is a `//` line comment in the region above a
//!   function definition: `// @cindergraph capacity(dst) = dst_len`,
//!   `// @cindergraph strlen(s) = n`, `// @cindergraph unroll = 8`. The
//!   consumer's original `// axeyum:` marker is an alias.
//! * The **API form** is an [`ExternalFacts`] value handed to
//!   [`crate::csource::semantic::AnalysisUnit::with_facts`], keyed by function
//!   name.
//!
//! Both resolve to the same [`FunctionFacts`]. A fact that cannot be attached
//! --- a parameter the function does not have, a value that is neither a
//! parameter nor a literal, a comment that no function follows --- is a
//! [`Diagnostic`], never a silent drop: a capacity the consumer believes was
//! attached and was not is a wrong verdict waiting to happen. When the API and
//! a comment give the same key the API wins, without a diagnostic, because
//! overriding a comment from a more authoritative source is what the API is
//! for.

use std::collections::BTreeMap;

use crate::csource::lex::C_SCAN;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::syntax::diag::{Diagnostic, Diagnostics};
use crate::syntax::ids::{NodeId, Span};
use crate::syntax::scan::{scan_block_comment, scan_line_comment, scan_whitespace};

/// The tool-neutral comment marker.
pub const MARKER: &str = "@cindergraph";
/// The consumer's original marker, accepted as an alias of [`MARKER`].
pub const ALIAS_MARKER: &str = "axeyum:";

/// What kind of fact this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FactKind {
    /// How many bytes a pointer parameter points at.
    Capacity,
    /// The length of the NUL-terminated string a pointer parameter points at,
    /// not counting the terminator.
    Strlen,
    /// How far a bounded unroller may unroll the function's loops.
    Unroll,
}

impl FactKind {
    /// Every kind, in the order facts are written on a node.
    pub const ALL: [FactKind; 3] = [FactKind::Capacity, FactKind::Strlen, FactKind::Unroll];

    /// The kind's spelling in the grammar and in the export.
    pub const fn name(self) -> &'static str {
        match self {
            FactKind::Capacity => "capacity",
            FactKind::Strlen => "strlen",
            FactKind::Unroll => "unroll",
        }
    }

    /// The kind spelled `name`, if any.
    pub fn parse(name: &str) -> Option<FactKind> {
        FactKind::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// Whether the fact names a parameter (`capacity`, `strlen`) rather than
    /// the function (`unroll`).
    pub const fn targets_parameter(self) -> bool {
        !matches!(self, FactKind::Unroll)
    }
}

/// The value a fact gives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FactValue {
    /// A scalar parameter of the same function.
    Parameter(String),
    /// A decimal integer literal, as written.
    Literal(String),
}

impl FactValue {
    /// The value as the export writes it: the parameter name or the digits.
    pub fn render(&self) -> &str {
        match self {
            FactValue::Parameter(name) | FactValue::Literal(name) => name,
        }
    }
}

/// Which front door a fact came through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FactSource {
    /// A `// @cindergraph` (or `// axeyum:`) comment above the function.
    Comment,
    /// An [`ExternalFacts`] argument.
    Api,
}

impl FactSource {
    /// The source's spelling in the export's `facts_source`.
    pub const fn name(self) -> &'static str {
        match self {
            FactSource::Comment => "comment",
            FactSource::Api => "api",
        }
    }
}

/// Facts a caller attaches through the API, keyed by function name.
///
/// Values are kept as the caller wrote them and validated against the parsed
/// function when the analysis unit is built, so a typo is a diagnostic on the
/// unit rather than a panic or a silently absent attribute.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExternalFacts {
    functions: BTreeMap<String, FunctionFactsInput>,
}

impl ExternalFacts {
    /// No facts.
    pub fn new() -> Self {
        Self::default()
    }

    /// The facts for the function named `name`, creating an empty entry.
    pub fn function(&mut self, name: impl Into<String>) -> &mut FunctionFactsInput {
        self.functions.entry(name.into()).or_default()
    }

    /// Whether no function has an entry.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// Every entry, by function name.
    pub fn functions(&self) -> impl Iterator<Item = (&str, &FunctionFactsInput)> {
        self.functions
            .iter()
            .map(|(name, input)| (name.as_str(), input))
    }
}

/// The facts the API gives one function, as written.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunctionFactsInput {
    capacity: BTreeMap<String, String>,
    strlen: BTreeMap<String, String>,
    unroll: Option<String>,
}

impl FunctionFactsInput {
    /// `capacity(parameter) = value`.
    pub fn capacity(
        &mut self,
        parameter: impl Into<String>,
        value: impl Into<String>,
    ) -> &mut Self {
        self.capacity.insert(parameter.into(), value.into());
        self
    }

    /// `strlen(parameter) = value`.
    pub fn strlen(&mut self, parameter: impl Into<String>, value: impl Into<String>) -> &mut Self {
        self.strlen.insert(parameter.into(), value.into());
        self
    }

    /// `unroll = value`.
    pub fn unroll(&mut self, value: impl Into<String>) -> &mut Self {
        self.unroll = Some(value.into());
        self
    }

    /// Every fact, as `(kind, target, value)`, in kind order.
    fn entries(&self) -> Vec<(FactKind, Option<&str>, &str)> {
        let mut out = Vec::new();
        for (parameter, value) in &self.capacity {
            out.push((FactKind::Capacity, Some(parameter.as_str()), value.as_str()));
        }
        for (parameter, value) in &self.strlen {
            out.push((FactKind::Strlen, Some(parameter.as_str()), value.as_str()));
        }
        if let Some(value) = &self.unroll {
            out.push((FactKind::Unroll, None, value.as_str()));
        }
        out
    }
}

/// One fact after resolution, ready to be written on a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachedFact {
    /// What the fact says.
    pub kind: FactKind,
    /// What it says it.
    pub value: FactValue,
    /// Where it came from.
    pub source: FactSource,
}

/// The facts attached to one parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParameterFactSet {
    /// The parameter's name.
    pub name: String,
    /// The span of the parameter's declared name, which is what the `ops`
    /// export's `declared` attribute carries for a load or store of it.
    pub declaration: Span,
    /// The `param_decl` node in the tree.
    pub node: NodeId,
    /// The facts, in [`FactKind::ALL`] order, at most one per kind.
    pub facts: Vec<AttachedFact>,
}

/// The facts attached to one function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FunctionFacts {
    /// Parameters that carry at least one fact, in parameter order.
    pub parameters: Vec<ParameterFactSet>,
    /// Function-level facts (`unroll`), in [`FactKind::ALL`] order.
    pub function: Vec<AttachedFact>,
}

impl FunctionFacts {
    /// Whether nothing is attached.
    pub fn is_empty(&self) -> bool {
        self.parameters.is_empty() && self.function.is_empty()
    }

    /// The facts on the parameter whose `param_decl` node is `node`.
    pub fn parameter(&self, node: NodeId) -> Option<&ParameterFactSet> {
        self.parameters.iter().find(|set| set.node == node)
    }

    /// The facts on the parameter whose declared name has span `declaration`.
    pub fn parameter_declared(&self, declaration: Span) -> Option<&ParameterFactSet> {
        self.parameters
            .iter()
            .find(|set| set.declaration == declaration)
    }
}

/// The `facts` and `facts_source` attribute values for `facts`, or `None`
/// when there are none.
///
/// `facts` is `kind=value` pairs joined by commas; `facts_source` is one
/// entry per fact, in the same order, so a reader splitting both on `,` can
/// zip them.
pub fn attributes(facts: &[AttachedFact]) -> Option<(String, String)> {
    if facts.is_empty() {
        return None;
    }
    let values: Vec<String> = facts
        .iter()
        .map(|fact| format!("{}={}", fact.kind.name(), fact.value.render()))
        .collect();
    let sources: Vec<&str> = facts.iter().map(|fact| fact.source.name()).collect();
    Some((values.join(","), sources.join(",")))
}

/// One parameter as the resolver needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParameterShape {
    pub(crate) name: String,
    pub(crate) declaration: Span,
    pub(crate) node: NodeId,
    /// `Some(true)` for a pointer, `Some(false)` for a known non-pointer,
    /// `None` when the type is unknown (a typedef from a header this snapshot
    /// never saw may well be a pointer).
    pub(crate) pointer: Option<bool>,
}

/// One function as the resolver needs to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FunctionShape {
    pub(crate) name: String,
    pub(crate) node: NodeId,
    pub(crate) name_span: Span,
    pub(crate) parameters: Vec<ParameterShape>,
}

/// One fact as written, before it is checked against a function.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RawFact {
    kind: FactKind,
    target: Option<String>,
    value: String,
    source: FactSource,
    /// The comment for a comment fact, the function name for an API fact.
    span: Span,
}

/// What resolution produced: the facts per function and every refusal,
/// each tagged with the front door the refused fact came through.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FactResolution {
    pub(crate) functions: Vec<FunctionFacts>,
    pub(crate) refused: Vec<(FactSource, Diagnostic)>,
}

/// Where refusals go while resolving.
#[derive(Default)]
struct Refusals(Vec<(FactSource, Diagnostic)>);

impl Refusals {
    fn push(&mut self, source: FactSource, span: Span, message: impl Into<String>) {
        self.0.push((source, Diagnostic::error(span, message)));
    }
}

/// Resolve every comment fact in `text` and every API fact in `api` against
/// `functions`, aligned with it, and push every refusal into `diagnostics`.
pub(crate) fn resolve_facts(
    text: &str,
    tree: &Tree,
    token_spans: &[Span],
    functions: &[FunctionShape],
    api: &ExternalFacts,
    diagnostics: &mut Diagnostics,
) -> FactResolution {
    let mut raw: Vec<Vec<RawFact>> = vec![Vec::new(); functions.len()];
    let mut refusals = Refusals::default();
    collect_comment_facts(text, tree, token_spans, functions, &mut raw, &mut refusals);
    collect_api_facts(functions, api, &mut raw, &mut refusals);
    let attached = functions
        .iter()
        .zip(raw)
        .map(|(function, facts)| attach(function, facts, &mut refusals))
        .collect();
    for (_, diagnostic) in &refusals.0 {
        diagnostics.push(diagnostic.clone());
    }
    FactResolution {
        functions: attached,
        refused: refusals.0,
    }
}

/// Every marked line comment in the trivia region above each top-level node,
/// attached to the function definition that follows it.
fn collect_comment_facts(
    text: &str,
    tree: &Tree,
    token_spans: &[Span],
    functions: &[FunctionShape],
    raw: &mut [Vec<RawFact>],
    refusals: &mut Refusals,
) {
    let arena = tree.arena();
    let mut previous_end = 0u32;
    for root in arena.roots() {
        let Some(span) = arena.span(*root, token_spans) else {
            continue;
        };
        let region = Span::new(previous_end.min(span.lo), span.lo);
        previous_end = span.hi;
        let is_definition = arena.tag(*root) == Some(NodeTag::FuncDef.as_u16());
        let index = if is_definition {
            functions.iter().position(|function| function.node == *root)
        } else {
            None
        };
        for (comment_span, body) in marked_comments(text, region) {
            match parse_fact(body) {
                Ok((kind, target, value)) => match index {
                    Some(index) => raw[index].push(RawFact {
                        kind,
                        target,
                        value,
                        source: FactSource::Comment,
                        span: comment_span,
                    }),
                    None => refusals.push(
                        FactSource::Comment,
                        comment_span,
                        "fact comment is not followed by a function definition, so it attaches to nothing",
                    ),
                },
                Err(problem) => refusals.push(
                    FactSource::Comment,
                    comment_span,
                    format!("malformed fact comment: {problem}"),
                ),
            }
        }
    }
    // Marked comments after the last top-level node follow no function.
    let tail = Span::new(previous_end.min(text.len() as u32), text.len() as u32);
    for (comment_span, _) in marked_comments(text, tail) {
        refusals.push(
            FactSource::Comment,
            comment_span,
            "fact comment is not followed by a function definition, so it attaches to nothing",
        );
    }
}

/// The `//` comments in `region` that start with a fact marker, as
/// `(comment span, text after the marker)`.
///
/// Block comments are prose and skipped whole, so a mention of the grammar
/// inside one is never a fact. Anything that is not trivia --- which cannot
/// happen in the gap between two top-level nodes, but this is total on any
/// region --- is stepped over one byte at a time.
fn marked_comments(text: &str, region: Span) -> Vec<(Span, &str)> {
    let mut out = Vec::new();
    let end = (region.hi as usize).min(text.len());
    let mut at = (region.lo as usize).min(end);
    while at < end {
        let spaces = scan_whitespace(text, at);
        if spaces > 0 {
            at += spaces;
            continue;
        }
        if let Some(comment) = scan_line_comment(text, at, C_SCAN) {
            let len = comment.len.max(1).min(end - at);
            let body = text.get(at + 2..at + len).unwrap_or("");
            if let Some(rest) = after_marker(body) {
                out.push((Span::new(at as u32, (at + len) as u32), rest));
            }
            at += len;
            continue;
        }
        if let Some(comment) = scan_block_comment(text, at, C_SCAN) {
            at += comment.len.max(1);
            continue;
        }
        at += 1;
    }
    out
}

/// The text after the fact marker, if `body` (a line comment without its
/// `//`) starts with one.
fn after_marker(body: &str) -> Option<&str> {
    let body = body.trim_start();
    if let Some(rest) = body.strip_prefix(MARKER) {
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        // `@cindergraphx` is not the marker.
        if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
            return None;
        }
        return Some(rest);
    }
    body.strip_prefix(ALIAS_MARKER)
}

/// Parse `capacity(p) = v`, `strlen(p) = v` or `unroll = v`.
fn parse_fact(body: &str) -> Result<(FactKind, Option<String>, String), String> {
    let body = body.trim();
    let (head, value) = body.split_once('=').ok_or_else(|| {
        format!("expected `kind(parameter) = value` or `unroll = value`, found {body:?}")
    })?;
    let head = head.trim();
    let value = value.trim();
    if !is_word(value) {
        return Err(format!(
            "the value must be a parameter name or a decimal literal, found {value:?}"
        ));
    }
    let (kind_name, target) = match head.split_once('(') {
        Some((kind_name, rest)) => {
            let target = rest
                .strip_suffix(')')
                .ok_or_else(|| format!("expected `)` after the parameter name in {head:?}"))?
                .trim();
            if !is_word(target) {
                return Err(format!(
                    "expected a parameter name in parentheses, found {target:?}"
                ));
            }
            (kind_name.trim(), Some(target.to_owned()))
        }
        None => (head, None),
    };
    let kind = FactKind::parse(kind_name).ok_or_else(|| {
        format!(
            "unknown fact kind {kind_name:?}; expected one of {:?}",
            FactKind::ALL.map(FactKind::name)
        )
    })?;
    match (kind.targets_parameter(), &target) {
        (true, None) => Err(format!(
            "`{}` needs a parameter: `{}(p) = value`",
            kind.name(),
            kind.name()
        )),
        (false, Some(_)) => Err(format!(
            "`{}` names no parameter: `{} = value`",
            kind.name(),
            kind.name()
        )),
        _ => Ok((kind, target, value.to_owned())),
    }
}

fn is_word(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn is_literal(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Every API fact, attached to the one function of that name.
fn collect_api_facts(
    functions: &[FunctionShape],
    api: &ExternalFacts,
    raw: &mut [Vec<RawFact>],
    refusals: &mut Refusals,
) {
    for (name, input) in api.functions() {
        let matches: Vec<usize> = functions
            .iter()
            .enumerate()
            .filter(|(_, function)| function.name == name)
            .map(|(index, _)| index)
            .collect();
        let index = match matches.as_slice() {
            [index] => *index,
            [] => {
                refusals.push(
                    FactSource::Api,
                    Span::empty_at(0),
                    format!("facts name function `{name}`, which the source does not define"),
                );
                continue;
            }
            several => {
                refusals.push(
                    FactSource::Api,
                    functions[several[0]].name_span,
                    format!(
                        "facts name function `{name}`, which the source defines {} times; \
                         a fact needs a unique name",
                        several.len()
                    ),
                );
                continue;
            }
        };
        let span = functions[index].name_span;
        for (kind, target, value) in input.entries() {
            raw[index].push(RawFact {
                kind,
                target: target.map(str::to_owned),
                value: value.to_owned(),
                source: FactSource::Api,
                span,
            });
        }
    }
}

/// Check every raw fact of one function and build its [`FunctionFacts`].
///
/// Comment facts are attached first; a second comment for the same key is a
/// diagnostic and the first stays. API facts come after and replace whatever
/// a comment said for the same key.
fn attach(function: &FunctionShape, raw: Vec<RawFact>, refusals: &mut Refusals) -> FunctionFacts {
    let mut attached: BTreeMap<(FactKind, Option<String>), AttachedFact> = BTreeMap::new();
    let mut ordered = raw;
    ordered.sort_by_key(|fact| fact.source);
    for fact in ordered {
        let Some(value) = check(function, &fact, refusals) else {
            continue;
        };
        let key = (fact.kind, fact.target.clone());
        if fact.source == FactSource::Comment && attached.contains_key(&key) {
            refusals.push(
                fact.source,
                fact.span,
                format!(
                    "duplicate fact `{}` for `{}`; the first one stays",
                    describe(fact.kind, fact.target.as_deref()),
                    function.name
                ),
            );
            continue;
        }
        attached.insert(
            key,
            AttachedFact {
                kind: fact.kind,
                value,
                source: fact.source,
            },
        );
    }
    let mut out = FunctionFacts::default();
    for parameter in &function.parameters {
        let facts: Vec<AttachedFact> = FactKind::ALL
            .into_iter()
            .filter(|kind| kind.targets_parameter())
            .filter_map(|kind| attached.get(&(kind, Some(parameter.name.clone()))).cloned())
            .collect();
        if !facts.is_empty() {
            out.parameters.push(ParameterFactSet {
                name: parameter.name.clone(),
                declaration: parameter.declaration,
                node: parameter.node,
                facts,
            });
        }
    }
    out.function = FactKind::ALL
        .into_iter()
        .filter(|kind| !kind.targets_parameter())
        .filter_map(|kind| attached.get(&(kind, None)).cloned())
        .collect();
    out
}

fn describe(kind: FactKind, target: Option<&str>) -> String {
    match target {
        Some(target) => format!("{}({target})", kind.name()),
        None => kind.name().to_owned(),
    }
}

/// The resolved value of `fact` on `function`, or `None` after reporting why
/// it cannot be attached.
fn check(function: &FunctionShape, fact: &RawFact, refusals: &mut Refusals) -> Option<FactValue> {
    let mut refuse = |message: String| {
        refusals.push(fact.source, fact.span, message);
        None
    };
    let parameter = |name: &str| function.parameters.iter().find(|p| p.name == name);
    let what = describe(fact.kind, fact.target.as_deref());
    if let Some(target) = &fact.target {
        match parameter(target) {
            None => {
                return refuse(format!(
                    "{what}: `{target}` is not a parameter of `{}`",
                    function.name
                ))
            }
            Some(shape) if shape.pointer == Some(false) => {
                return refuse(format!(
                    "{what}: `{target}` is not a pointer parameter of `{}`",
                    function.name
                ))
            }
            Some(_) => {}
        }
    }
    if is_literal(&fact.value) {
        return Some(FactValue::Literal(fact.value.clone()));
    }
    if fact.kind == FactKind::Unroll {
        return refuse(format!(
            "{what}: the bound must be a decimal literal, found `{}`",
            fact.value
        ));
    }
    match parameter(&fact.value) {
        None => refuse(format!(
            "{what}: `{}` is neither a parameter of `{}` nor a decimal literal",
            fact.value, function.name
        )),
        Some(shape) if shape.pointer == Some(true) => refuse(format!(
            "{what}: `{}` is a pointer parameter, not a scalar",
            fact.value
        )),
        Some(_) => Some(FactValue::Parameter(fact.value.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grammar_accepts_the_three_kinds_and_both_value_forms() {
        assert_eq!(
            parse_fact(" capacity(dst) = dst_len "),
            Ok((
                FactKind::Capacity,
                Some("dst".to_owned()),
                "dst_len".to_owned()
            ))
        );
        assert_eq!(
            parse_fact("strlen( s )=n"),
            Ok((FactKind::Strlen, Some("s".to_owned()), "n".to_owned()))
        );
        assert_eq!(
            parse_fact("capacity(dst) = 256"),
            Ok((FactKind::Capacity, Some("dst".to_owned()), "256".to_owned()))
        );
        assert_eq!(
            parse_fact("unroll = 8"),
            Ok((FactKind::Unroll, None, "8".to_owned()))
        );
    }

    #[test]
    fn the_grammar_refuses_what_it_does_not_define() {
        for bad in [
            "capacity dst = 3",
            "capacity(dst) = ",
            "capacity(dst) = a b",
            "capacity(dst) = -1",
            "capacity(dst) == 3",
            "size(dst) = 3",
            "unroll(dst) = 3",
            "capacity = 3",
            "capacity(dst = 3",
            "capacity() = 3",
        ] {
            assert!(parse_fact(bad).is_err(), "{bad:?} parsed");
        }
    }

    #[test]
    fn both_markers_are_recognised_and_nothing_else_is() {
        assert_eq!(
            after_marker(" @cindergraph capacity(p) = n"),
            Some(" capacity(p) = n")
        );
        assert_eq!(
            after_marker("@cindergraph: unroll = 2"),
            Some(" unroll = 2")
        );
        assert_eq!(
            after_marker(" axeyum: capacity(p) = n"),
            Some(" capacity(p) = n")
        );
        assert_eq!(after_marker(" @cindergraphs capacity(p) = n"), None);
        assert_eq!(after_marker(" axeyum capacity(p) = n"), None);
        assert_eq!(after_marker(" see @cindergraph for the grammar"), None);
    }

    #[test]
    fn marked_comments_skip_block_comments_and_keep_line_comment_spans() {
        let text = "/* @cindergraph capacity(p) = n */\n// @cindergraph unroll = 4\n// plain\n";
        let found = marked_comments(text, Span::new(0, text.len() as u32));
        assert_eq!(found.len(), 1);
        let (span, body) = &found[0];
        assert_eq!(
            &text[span.lo as usize..span.hi as usize],
            "// @cindergraph unroll = 4"
        );
        assert_eq!(*body, " unroll = 4");
    }

    #[test]
    fn attributes_zip_facts_with_their_sources() {
        let facts = vec![
            AttachedFact {
                kind: FactKind::Capacity,
                value: FactValue::Parameter("n".to_owned()),
                source: FactSource::Comment,
            },
            AttachedFact {
                kind: FactKind::Strlen,
                value: FactValue::Literal("4".to_owned()),
                source: FactSource::Api,
            },
        ];
        assert_eq!(
            attributes(&facts),
            Some(("capacity=n,strlen=4".to_owned(), "comment,api".to_owned()))
        );
        assert_eq!(attributes(&[]), None);
    }
}
