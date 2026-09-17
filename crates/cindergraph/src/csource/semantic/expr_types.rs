//! C types of expressions, derived from the declared types this layer owns.
//!
//! [`super::types`] answers "what type was this name declared with". This
//! module answers the next question a semantic consumer asks: what type does
//! `hdr + body` or `len > 256` have once C17 §6.3.1.1 (integer promotion) and
//! §6.3.1.8 (the usual arithmetic conversions) have been applied. It exists so
//! that no consumer has to re-implement those rules --- and get one of them
//! wrong --- from the declared types alone.
//!
//! # Conservative by construction
//!
//! Every rule here either applies exactly or yields [`Base::Unknown`]. An
//! unresolved name, a call to a function with no visible declaration, a
//! `_Complex` operand, an enum-typed operand under promotion (whose compatible
//! integer type is implementation-defined), or a literal outside the ranges
//! C17 §6.4.4.1 assigns a type to, is `unknown` rather than a guess. A pointer
//! to an unresolved base keeps its shape: `unknown *` is a pointer whose
//! pointee could not be named, and a consumer that only needs to know it is a
//! pointer can still use it.
//!
//! # The one platform assumption
//!
//! Type *names* are platform-neutral, but §6.3.1.8's "if the signed type can
//! represent all values of the unsigned type" is a width question. The widths
//! used are the LP64 model (`int` 32, `long` 64, `long long` 64), the model of
//! every 64-bit Unix target. It changes exactly one family of answers: a
//! signed type of higher rank meeting a narrower unsigned type, such as
//! `long` with `unsigned int`, which is `long` under LP64 and `unsigned long`
//! under ILP32. Everything else --- promotion, same-rank mixing, literal
//! typing --- follows from rank alone.
//!
//! # Not recursion
//!
//! Expression trees can be deep. Types are computed bottom-up by walking the
//! subtree's preorder list backwards, which visits every child before its
//! parent without a call stack; type rendering walks declarator layers
//! iteratively.

use std::collections::BTreeMap;

use crate::csource::lex::kind::TokenKind;
use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::syntax::ids::{NodeId, Span, TokenId};

use super::declarations::{
    parameter_declarations, FunctionResolution, SymbolKind, TranslationUnitSymbols,
};
use super::types::{ArrayBound, FunctionTypes, Qualifiers, RecordId, RecordKind, TypeId, TypeNode};

/// A standard integer type, ordered by conversion rank within each signedness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IntKind {
    Bool,
    Char,
    SChar,
    UChar,
    Short,
    UShort,
    Int,
    UInt,
    Long,
    ULong,
    LongLong,
    ULongLong,
    Int128,
    UInt128,
}

impl IntKind {
    /// Conversion rank, C17 §6.3.1.1.
    const fn rank(self) -> u8 {
        match self {
            IntKind::Bool => 0,
            IntKind::Char | IntKind::SChar | IntKind::UChar => 1,
            IntKind::Short | IntKind::UShort => 2,
            IntKind::Int | IntKind::UInt => 3,
            IntKind::Long | IntKind::ULong => 4,
            IntKind::LongLong | IntKind::ULongLong => 5,
            IntKind::Int128 | IntKind::UInt128 => 6,
        }
    }

    /// Width in bits under LP64 (see the module documentation).
    const fn width(self) -> u32 {
        match self {
            IntKind::Bool => 1,
            IntKind::Char | IntKind::SChar | IntKind::UChar => 8,
            IntKind::Short | IntKind::UShort => 16,
            IntKind::Int | IntKind::UInt => 32,
            IntKind::Long | IntKind::ULong | IntKind::LongLong | IntKind::ULongLong => 64,
            IntKind::Int128 | IntKind::UInt128 => 128,
        }
    }

    const fn is_unsigned(self) -> bool {
        matches!(
            self,
            IntKind::Bool
                | IntKind::UChar
                | IntKind::UShort
                | IntKind::UInt
                | IntKind::ULong
                | IntKind::ULongLong
                | IntKind::UInt128
        )
    }

    /// The unsigned type of the same rank.
    const fn unsigned(self) -> IntKind {
        match self {
            IntKind::Bool => IntKind::Bool,
            IntKind::Char | IntKind::SChar | IntKind::UChar => IntKind::UChar,
            IntKind::Short | IntKind::UShort => IntKind::UShort,
            IntKind::Int | IntKind::UInt => IntKind::UInt,
            IntKind::Long | IntKind::ULong => IntKind::ULong,
            IntKind::LongLong | IntKind::ULongLong => IntKind::ULongLong,
            IntKind::Int128 | IntKind::UInt128 => IntKind::UInt128,
        }
    }

    /// Integer promotion, C17 §6.3.1.1p2: every type of lower rank than `int`
    /// fits in `int` under LP64, so it promotes to `int`.
    const fn promoted(self) -> IntKind {
        if self.rank() < IntKind::Int.rank() {
            IntKind::Int
        } else {
            self
        }
    }

    /// The common type of two promoted operands, C17 §6.3.1.8p1.
    fn usual(self, other: IntKind) -> IntKind {
        let a = self.promoted();
        let b = other.promoted();
        if a == b {
            return a;
        }
        if a.is_unsigned() == b.is_unsigned() {
            return if a.rank() >= b.rank() { a } else { b };
        }
        let (unsigned, signed) = if a.is_unsigned() { (a, b) } else { (b, a) };
        if unsigned.rank() >= signed.rank() {
            unsigned
        } else if signed.width() > unsigned.width() {
            signed
        } else {
            signed.unsigned()
        }
    }

    const fn spelling(self) -> &'static str {
        match self {
            IntKind::Bool => "_Bool",
            IntKind::Char => "char",
            IntKind::SChar => "signed char",
            IntKind::UChar => "unsigned char",
            IntKind::Short => "short",
            IntKind::UShort => "unsigned short",
            IntKind::Int => "int",
            IntKind::UInt => "unsigned int",
            IntKind::Long => "long",
            IntKind::ULong => "unsigned long",
            IntKind::LongLong => "long long",
            IntKind::ULongLong => "unsigned long long",
            IntKind::Int128 => "__int128",
            IntKind::UInt128 => "unsigned __int128",
        }
    }
}

/// A real floating type, ordered by §6.3.1.8's first three rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FloatKind {
    Float,
    Double,
    LongDouble,
}

impl FloatKind {
    const fn spelling(self) -> &'static str {
        match self {
            FloatKind::Float => "float",
            FloatKind::Double => "double",
            FloatKind::LongDouble => "long double",
        }
    }
}

/// The innermost layer of a type: what is left once every declarator
/// operator has been peeled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Base {
    /// Could not be established; see the module documentation for when.
    Unknown,
    Void,
    Int(IntKind),
    Float(FloatKind),
    /// An enumerated type by its spelling. Its compatible integer type is
    /// implementation-defined, so arithmetic on it is `Unknown`.
    Enum(String),
    /// A struct or union by its spelling, with its identity when known.
    Record {
        spelling: String,
        record: Option<RecordId>,
    },
}

/// One declarator operator, outermost first in [`CType::layers`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Layer {
    Pointer(Qualifiers),
    /// The bound as it will be rendered: digits, an expression's text, `*`,
    /// or empty for an incomplete array.
    Array(String),
    Function,
}

/// A C type as a flat list of declarator layers over a base.
///
/// `layers[0]` is the outermost operator: `unsigned char *` is one pointer
/// layer over an `unsigned char` base, and `int (*)[4]` is a pointer layer
/// over an array layer over `int`. The representation is a value, so taking an
/// address or dereferencing is a `Vec` edit rather than an arena allocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CType {
    pub(crate) layers: Vec<Layer>,
    pub(crate) base: Base,
    pub(crate) base_qualifiers: Qualifiers,
}

impl CType {
    pub(crate) const UNKNOWN: CType = CType {
        layers: Vec::new(),
        base: Base::Unknown,
        base_qualifiers: Qualifiers::NONE,
    };

    /// `void`: the type of an operation that produces no value.
    pub(crate) const VOID: CType = CType {
        layers: Vec::new(),
        base: Base::Void,
        base_qualifiers: Qualifiers::NONE,
    };

    fn scalar(base: Base) -> Self {
        CType {
            layers: Vec::new(),
            base,
            base_qualifiers: Qualifiers::default(),
        }
    }

    pub(crate) fn int(kind: IntKind) -> Self {
        Self::scalar(Base::Int(kind))
    }

    pub(crate) fn is_unknown(&self) -> bool {
        self.layers.is_empty() && self.base == Base::Unknown
    }

    pub(crate) fn as_int(&self) -> Option<IntKind> {
        match (&self.base, self.layers.is_empty()) {
            (Base::Int(kind), true) => Some(*kind),
            _ => None,
        }
    }

    fn as_float(&self) -> Option<FloatKind> {
        match (&self.base, self.layers.is_empty()) {
            (Base::Float(kind), true) => Some(*kind),
            _ => None,
        }
    }

    pub(crate) fn is_arithmetic(&self) -> bool {
        self.as_int().is_some() || self.as_float().is_some()
    }

    pub(crate) fn is_pointer(&self) -> bool {
        matches!(self.layers.first(), Some(Layer::Pointer(_)))
    }

    fn is_array(&self) -> bool {
        matches!(self.layers.first(), Some(Layer::Array(_)))
    }

    /// Whether this type could still be a pointer: a pointer, an array, or
    /// entirely unknown.
    fn may_be_pointer(&self) -> bool {
        self.is_pointer() || self.is_array() || self.is_unknown()
    }

    /// The result of lvalue conversion: the same type without its top-level
    /// qualifiers (C17 §6.3.2.1p2). Array element qualifiers are not
    /// top-level and stay.
    pub(crate) fn unqualified(mut self) -> Self {
        match self.layers.first_mut() {
            Some(Layer::Pointer(qualifiers)) => *qualifiers = Qualifiers::default(),
            Some(_) => {}
            None => self.base_qualifiers = Qualifiers::default(),
        }
        self
    }

    /// The array-to-pointer conversion, C17 §6.3.2.1p3; other types are
    /// returned unchanged.
    pub(crate) fn decayed(mut self) -> Self {
        if let Some(Layer::Array(_)) = self.layers.first() {
            self.layers[0] = Layer::Pointer(Qualifiers::default());
        }
        self
    }

    /// The pointee of a pointer or the element of an array.
    pub(crate) fn pointee(mut self) -> Option<Self> {
        match self.layers.first() {
            Some(Layer::Pointer(_) | Layer::Array(_)) => {
                self.layers.remove(0);
                Some(self)
            }
            _ => None,
        }
    }

    fn address_of(mut self) -> Self {
        self.layers.insert(0, Layer::Pointer(Qualifiers::default()));
        self
    }

    /// Integer promotion applied to an arithmetic type; anything else is
    /// unknown. An enum is unknown here on purpose (see [`Base::Enum`]).
    pub(crate) fn promoted(&self) -> CType {
        if let Some(kind) = self.as_int() {
            CType::int(kind.promoted())
        } else if let Some(kind) = self.as_float() {
            CType::scalar(Base::Float(kind))
        } else {
            CType::UNKNOWN
        }
    }

    /// Number of pointer layers before the first non-pointer layer.
    pub(crate) fn pointer_depth(&self) -> u32 {
        self.layers
            .iter()
            .take_while(|layer| matches!(layer, Layer::Pointer(_)))
            .count() as u32
    }

    /// The type as C would spell it, or `unknown`.
    ///
    /// Rendering is the standard abstract-declarator composition, done from
    /// the outermost layer inward with an explicit accumulator rather than
    /// recursion: `*` binds tighter than `[]` and `()`, so a pointer directly
    /// over an array or function is parenthesized.
    pub(crate) fn render(&self) -> String {
        let base = match &self.base {
            Base::Unknown => "unknown".to_owned(),
            Base::Void => "void".to_owned(),
            Base::Int(kind) => kind.spelling().to_owned(),
            Base::Float(kind) => kind.spelling().to_owned(),
            Base::Enum(spelling) | Base::Record { spelling, .. } => spelling.clone(),
        };
        let base = format!("{}{base}", qualifier_prefix(self.base_qualifiers));
        let mut inner = String::new();
        for (index, layer) in self.layers.iter().enumerate() {
            match layer {
                Layer::Pointer(qualifiers) => {
                    let suffix = qualifier_suffix(*qualifiers);
                    inner = format!("*{suffix}{inner}");
                    if matches!(
                        self.layers.get(index + 1),
                        Some(Layer::Array(_) | Layer::Function)
                    ) {
                        inner = format!("({inner})");
                    }
                }
                Layer::Array(bound) => inner.push_str(&format!("[{bound}]")),
                Layer::Function => inner.push_str("()"),
            }
        }
        if inner.is_empty() {
            base
        } else if inner.starts_with('*') || inner.starts_with('(') {
            format!("{base} {inner}")
        } else {
            format!("{base}{inner}")
        }
    }
}

fn qualifier_prefix(qualifiers: Qualifiers) -> String {
    let mut out = String::new();
    if qualifiers.is_const {
        out.push_str("const ");
    }
    if qualifiers.is_volatile {
        out.push_str("volatile ");
    }
    if qualifiers.is_atomic {
        out.push_str("_Atomic ");
    }
    if qualifiers.is_restrict {
        out.push_str("restrict ");
    }
    out
}

fn qualifier_suffix(qualifiers: Qualifiers) -> String {
    let mut out = String::new();
    if qualifiers.is_const {
        out.push_str("const ");
    }
    if qualifiers.is_volatile {
        out.push_str("volatile ");
    }
    if qualifiers.is_atomic {
        out.push_str("_Atomic ");
    }
    if qualifiers.is_restrict {
        out.push_str("restrict ");
    }
    // `char *const p`: no space after the star, one after each word.
    out.trim_end().to_owned()
}

/// The usual arithmetic conversions on two arithmetic types (C17 §6.3.1.8),
/// or `Unknown` when either operand is not arithmetic.
pub(crate) fn usual_arithmetic(left: &CType, right: &CType) -> CType {
    match (left.as_float(), right.as_float()) {
        (Some(a), Some(b)) => return CType::scalar(Base::Float(a.max(b))),
        (Some(a), None) if right.as_int().is_some() => return CType::scalar(Base::Float(a)),
        (None, Some(b)) if left.as_int().is_some() => return CType::scalar(Base::Float(b)),
        _ => {}
    }
    match (left.as_int(), right.as_int()) {
        (Some(a), Some(b)) => CType::int(a.usual(b)),
        _ => CType::UNKNOWN,
    }
}

/// The types computed for one function's expressions.
#[derive(Debug, Default)]
pub(crate) struct ExpressionTypes {
    /// The type of each expression node; absent for nodes that are not
    /// expressions or were never reached.
    types: BTreeMap<NodeId, CType>,
    /// For binary and assignment chains, the type the operands of each
    /// operator are converted to, one entry per operator in source order.
    operand_types: BTreeMap<NodeId, Vec<CType>>,
    /// For binary chains, the type of the value after each operator, one
    /// entry per operator in source order; the last is the node's type. A
    /// chain is one node, so the inner prefixes (`a + b` in `a + b - c`) have
    /// no node of their own to carry a type, and an evaluation consumer that
    /// lowers the chain one operator at a time reads them here.
    chain_results: BTreeMap<NodeId, Vec<CType>>,
}

impl ExpressionTypes {
    pub(crate) fn type_of(&self, node: NodeId) -> Option<&CType> {
        self.types.get(&node)
    }

    pub(crate) fn operand_types_of(&self, node: NodeId) -> Option<&[CType]> {
        self.operand_types.get(&node).map(Vec::as_slice)
    }

    pub(crate) fn chain_results_of(&self, node: NodeId) -> Option<&[CType]> {
        self.chain_results.get(&node).map(Vec::as_slice)
    }
}

/// Everything the typer reads; one per function.
pub(crate) struct ExpressionTyper<'a> {
    pub(crate) tree: &'a Tree,
    pub(crate) text: &'a str,
    pub(crate) token_spans: &'a [Span],
    pub(crate) resolution: &'a FunctionResolution,
    pub(crate) types: &'a FunctionTypes,
    pub(crate) symbols: &'a TranslationUnitSymbols,
}

impl ExpressionTyper<'_> {
    /// Types of every expression under `root`, children before parents.
    pub(crate) fn compute(&self, root: NodeId) -> ExpressionTypes {
        let arena = self.tree.arena();
        let order: Vec<NodeId> = arena.preorder(root).collect();
        let mut out = ExpressionTypes::default();
        for node in order.into_iter().rev() {
            let Some(tag) = arena.tag(node).and_then(NodeTag::from_u16) else {
                continue;
            };
            let Some(ty) = self.type_node(node, tag, &mut out) else {
                continue;
            };
            out.types.insert(node, ty);
        }
        out
    }

    /// The type of one expression node given its children's, or `None` for a
    /// node that has no value.
    fn type_node(&self, node: NodeId, tag: NodeTag, out: &mut ExpressionTypes) -> Option<CType> {
        let arena = self.tree.arena();
        let children: Vec<NodeId> = arena.children_iter(node).collect();
        let child_types: Vec<CType> = children
            .iter()
            .map(|child| out.types.get(child).cloned().unwrap_or(CType::UNKNOWN))
            .collect();
        let child =
            |index: usize| -> CType { child_types.get(index).cloned().unwrap_or(CType::UNKNOWN) };
        let offset = arena.span(node, self.token_spans).map_or(0, |span| span.lo);
        Some(match tag {
            NodeTag::ParenExpr => child(0),
            NodeTag::CommaExpr => child_types.last().cloned().unwrap_or(CType::UNKNOWN),
            NodeTag::NameRef => self.name_ref(node),
            NodeTag::Literal => self.literal(node),
            NodeTag::UnaryExpr => {
                let op = self.first_token_text(node).unwrap_or("");
                let operand = child(0);
                match op {
                    "-" | "+" | "~" => operand.promoted(),
                    "!" => CType::int(IntKind::Int),
                    "&" => operand.unqualified().address_of(),
                    "*" => operand
                        .decayed()
                        .pointee()
                        .map_or(CType::UNKNOWN, CType::unqualified),
                    "++" | "--" => operand.unqualified(),
                    "sizeof" => self.visible_typedef("size_t", node),
                    _ => CType::UNKNOWN,
                }
            }
            NodeTag::SizeofType | NodeTag::AlignofType => self.visible_typedef("size_t", node),
            NodeTag::CastExpr | NodeTag::CompoundLiteral => children
                .first()
                .filter(|first| arena.tag(**first) == Some(NodeTag::TypeName.as_u16()))
                .map_or(CType::UNKNOWN, |type_name| {
                    self.type_name(*type_name).unqualified()
                }),
            NodeTag::BinaryExpr => {
                let ops = self.gap_operators(&children);
                let mut acc = child(0);
                let mut operand_types = Vec::with_capacity(ops.len());
                let mut results = Vec::with_capacity(ops.len());
                for (index, op) in ops.iter().enumerate() {
                    let rhs = child(index + 1);
                    let (result, operand) = self.binary(op, &acc, &rhs, offset);
                    if let Some(operand) = operand {
                        operand_types.push(operand);
                    }
                    results.push(result.clone());
                    acc = result;
                }
                if !operand_types.is_empty() {
                    out.operand_types.insert(node, operand_types);
                }
                if !results.is_empty() {
                    out.chain_results.insert(node, results);
                }
                acc
            }
            NodeTag::AssignExpr => {
                let ops = self.gap_operators(&children);
                let mut operand_types = Vec::with_capacity(ops.len());
                for (index, op) in ops.iter().enumerate() {
                    let lhs = child(index).unqualified();
                    let rhs = child(index + 1);
                    let operand = if op == "=" {
                        lhs
                    } else {
                        let compound = op.strip_suffix('=').unwrap_or(op);
                        self.binary(compound, &lhs, &rhs, offset)
                            .1
                            .unwrap_or(CType::UNKNOWN)
                    };
                    operand_types.push(operand);
                }
                out.operand_types.insert(node, operand_types);
                child(0).unqualified()
            }
            NodeTag::CondExpr => {
                // `a ? b : c` has three children; GNU `a ?: c` has two, and its
                // omitted arm is the condition's value.
                let (then, otherwise) = match children.len() {
                    3 => (child(1), child(2)),
                    2 => (child(0), child(1)),
                    _ => return Some(CType::UNKNOWN),
                };
                self.conditional(then, otherwise)
            }
            NodeTag::PostfixExpr => {
                let mut acc = child(0);
                for suffix in children.iter().skip(1) {
                    let suffix_tag = arena.tag(*suffix).and_then(NodeTag::from_u16);
                    acc = match suffix_tag {
                        Some(NodeTag::IndexSuffix) => {
                            let index = arena
                                .child(*suffix, 0)
                                .and_then(|index| out.types.get(&index))
                                .cloned()
                                .unwrap_or(CType::UNKNOWN);
                            if acc.is_pointer() || acc.is_array() {
                                acc.pointee().map_or(CType::UNKNOWN, CType::unqualified)
                            } else if acc.as_int().is_some()
                                && (index.is_pointer() || index.is_array())
                            {
                                index.pointee().map_or(CType::UNKNOWN, CType::unqualified)
                            } else {
                                CType::UNKNOWN
                            }
                        }
                        Some(NodeTag::MemberSuffix) => self.member(&acc, *suffix),
                        Some(NodeTag::CallArgs) => match acc.layers.first() {
                            Some(Layer::Function) => acc.pointee_function(),
                            Some(Layer::Pointer(_))
                                if matches!(acc.layers.get(1), Some(Layer::Function)) =>
                            {
                                acc.pointee()
                                    .map(CType::pointee_function)
                                    .unwrap_or(CType::UNKNOWN)
                            }
                            _ => CType::UNKNOWN,
                        },
                        Some(NodeTag::IncDecSuffix) => acc.unqualified(),
                        _ => CType::UNKNOWN,
                    };
                }
                acc
            }
            NodeTag::LabelAddr => CType::scalar(Base::Void).address_of(),
            NodeTag::StmtExpr | NodeTag::BuiltinExpr => CType::UNKNOWN,
            _ => return None,
        })
    }

    /// The type of `op` applied to `left` and `right`, and the type the
    /// operands are converted to for it (`None` when the operator converts
    /// nothing: `&&` and `||` test each operand on its own).
    pub(crate) fn binary(
        &self,
        op: &str,
        left: &CType,
        right: &CType,
        offset: u32,
    ) -> (CType, Option<CType>) {
        let int = CType::int(IntKind::Int);
        match op {
            "*" | "/" => {
                let common = usual_arithmetic(left, right);
                (common.clone(), Some(common))
            }
            "%" | "&" | "|" | "^" => {
                let common = match (left.as_int(), right.as_int()) {
                    (Some(a), Some(b)) => CType::int(a.usual(b)),
                    _ => CType::UNKNOWN,
                };
                (common.clone(), Some(common))
            }
            "+" => {
                if left.is_arithmetic() && right.is_arithmetic() {
                    let common = usual_arithmetic(left, right);
                    (common.clone(), Some(common))
                } else if (left.is_pointer() || left.is_array()) && right.as_int().is_some() {
                    let pointer = left.clone().decayed().unqualified();
                    (pointer.clone(), Some(pointer))
                } else if (right.is_pointer() || right.is_array()) && left.as_int().is_some() {
                    let pointer = right.clone().decayed().unqualified();
                    (pointer.clone(), Some(pointer))
                } else {
                    (CType::UNKNOWN, Some(CType::UNKNOWN))
                }
            }
            "-" => {
                if left.is_arithmetic() && right.is_arithmetic() {
                    let common = usual_arithmetic(left, right);
                    (common.clone(), Some(common))
                } else if (left.is_pointer() || left.is_array()) && right.as_int().is_some() {
                    let pointer = left.clone().decayed().unqualified();
                    (pointer.clone(), Some(pointer))
                } else if (left.is_pointer() || left.is_array())
                    && (right.is_pointer() || right.is_array())
                {
                    let pointer = left.clone().decayed().unqualified();
                    (self.visible_typedef_at("ptrdiff_t", offset), Some(pointer))
                } else {
                    (CType::UNKNOWN, Some(CType::UNKNOWN))
                }
            }
            "<<" | ">>" => {
                let promoted = if left.as_int().is_some() && right.as_int().is_some() {
                    left.promoted()
                } else {
                    CType::UNKNOWN
                };
                (promoted.clone(), Some(promoted))
            }
            "<" | ">" | "<=" | ">=" | "==" | "!=" => {
                let operand = if left.is_arithmetic() && right.is_arithmetic() {
                    usual_arithmetic(left, right)
                } else if left.may_be_pointer() && right.may_be_pointer() {
                    let a = left.clone().decayed().unqualified();
                    let b = right.clone().decayed().unqualified();
                    if a == b {
                        a
                    } else if a.is_unknown() {
                        b
                    } else if b.is_unknown() {
                        a
                    } else {
                        CType::UNKNOWN
                    }
                } else if (left.is_pointer() || left.is_array()) && right.as_int().is_some() {
                    left.clone().decayed().unqualified()
                } else if (right.is_pointer() || right.is_array()) && left.as_int().is_some() {
                    right.clone().decayed().unqualified()
                } else {
                    CType::UNKNOWN
                };
                (int, Some(operand))
            }
            "&&" | "||" => (int, None),
            _ => (CType::UNKNOWN, Some(CType::UNKNOWN)),
        }
    }

    /// The type of `cond ? then : otherwise`, C17 §6.5.15p5--6.
    fn conditional(&self, then: CType, otherwise: CType) -> CType {
        if then.is_arithmetic() && otherwise.is_arithmetic() {
            return usual_arithmetic(&then, &otherwise);
        }
        let then = then.decayed().unqualified();
        let otherwise = otherwise.decayed().unqualified();
        if then.is_pointer() && otherwise.is_pointer() {
            return if then == otherwise {
                then
            } else {
                CType::UNKNOWN
            };
        }
        if then.is_pointer() && otherwise.as_int().is_some() {
            return then;
        }
        if otherwise.is_pointer() && then.as_int().is_some() {
            return otherwise;
        }
        if then == otherwise
            && matches!(then.base, Base::Void | Base::Record { .. } | Base::Enum(_))
            && then.layers.is_empty()
        {
            return then;
        }
        CType::UNKNOWN
    }

    /// The declared type of the name at `node`, after lvalue conversion.
    fn name_ref(&self, node: NodeId) -> CType {
        let Some(span) = self.tree.arena().span(node, self.token_spans) else {
            return CType::UNKNOWN;
        };
        let Some(name) = self.text.get(span.lo as usize..span.hi as usize) else {
            return CType::UNKNOWN;
        };
        match self.resolution.resolve_at(name, span.lo) {
            Some(declaration) => match declaration.kind {
                SymbolKind::Value => self
                    .types
                    .adjusted_type_of_declaration(declaration.span)
                    .map_or(CType::UNKNOWN, |id| self.declared_type(id).unqualified()),
                SymbolKind::Constant => CType::int(IntKind::Int),
                SymbolKind::Typedef => CType::UNKNOWN,
            },
            None if self.symbols.constant_is_visible(name, span.lo) => CType::int(IntKind::Int),
            None => CType::UNKNOWN,
        }
    }

    /// The type of a member access suffix (`.name` or `->name`) on `base`.
    fn member(&self, base: &CType, suffix: NodeId) -> CType {
        let Some((first, end)) = self.tree.arena().token_extent(suffix) else {
            return CType::UNKNOWN;
        };
        let through_pointer =
            match TokenKind::from_u16(self.tree.tokens().kind(TokenId::new(first))) {
                Some(TokenKind::Arrow) => true,
                Some(TokenKind::Dot) => false,
                _ => return CType::UNKNOWN,
            };
        let Some(name) = (first + 1..end)
            .filter_map(|raw| self.token_spans.get(raw as usize))
            .find_map(|span| self.text.get(span.lo as usize..span.hi as usize))
        else {
            return CType::UNKNOWN;
        };
        let object = if through_pointer {
            match base.clone().decayed().pointee() {
                Some(object) => object,
                None => return CType::UNKNOWN,
            }
        } else {
            base.clone()
        };
        let Base::Record {
            record: Some(record),
            ..
        } = object.base
        else {
            return CType::UNKNOWN;
        };
        if !object.layers.is_empty() {
            return CType::UNKNOWN;
        }
        self.types
            .record_member_type(record, name)
            .map_or(CType::UNKNOWN, |id| self.declared_type(id).unqualified())
    }

    /// The type an integer, floating, character or string literal has.
    fn literal(&self, node: NodeId) -> CType {
        let Some((first, end)) = self.tree.arena().token_extent(node) else {
            return CType::UNKNOWN;
        };
        let kinds: Vec<Option<TokenKind>> = (first..end)
            .map(|raw| TokenKind::from_u16(self.tree.tokens().kind(TokenId::new(raw))))
            .collect();
        let texts: Vec<&str> = (first..end)
            .filter_map(|raw| self.token_spans.get(raw as usize))
            .filter_map(|span| self.text.get(span.lo as usize..span.hi as usize))
            .collect();
        match (kinds.first(), texts.first()) {
            (Some(Some(TokenKind::IntLiteral)), Some(text)) => integer_literal(text),
            (Some(Some(TokenKind::FloatLiteral)), Some(text)) => float_literal(text),
            (Some(Some(TokenKind::CharLiteral)), Some(text)) => {
                let prefix = text.find('\'').map(|quote| &text[..quote]);
                match prefix {
                    Some("") => CType::int(IntKind::Int),
                    Some("L") => self.visible_typedef("wchar_t", node),
                    Some("u") => self.visible_typedef("char16_t", node),
                    Some("U") => self.visible_typedef("char32_t", node),
                    Some("u8") => CType::int(IntKind::UChar),
                    _ => CType::UNKNOWN,
                }
            }
            (Some(Some(TokenKind::StringLiteral)), _) => {
                if kinds
                    .iter()
                    .any(|kind| *kind != Some(TokenKind::StringLiteral))
                {
                    return CType::UNKNOWN;
                }
                let mut bytes = 0usize;
                for text in &texts {
                    let Some(length) = narrow_string_bytes(text) else {
                        return CType::UNKNOWN;
                    };
                    bytes += length;
                }
                CType {
                    layers: vec![Layer::Array((bytes + 1).to_string())],
                    base: Base::Int(IntKind::Char),
                    base_qualifiers: Qualifiers::default(),
                }
            }
            _ => CType::UNKNOWN,
        }
    }

    /// The type a typedef named `name` denotes where `node` sits, or unknown
    /// when no such typedef is visible. Standard names like `size_t` have no
    /// type without their header, and this does not invent one.
    fn visible_typedef(&self, name: &str, node: NodeId) -> CType {
        let offset = self
            .tree
            .arena()
            .span(node, self.token_spans)
            .map_or(0, |span| span.lo);
        self.visible_typedef_at(name, offset)
    }

    fn visible_typedef_at(&self, name: &str, offset: u32) -> CType {
        let declaration = match self.resolution.resolve_at(name, offset) {
            Some(declaration) if declaration.kind == SymbolKind::Typedef => Some(declaration.span),
            Some(_) => None,
            None => self.symbols.visible_typedef_declaration(name, offset),
        };
        declaration
            .and_then(|span| self.types.type_of_declaration(span))
            .map_or(CType::UNKNOWN, |id| self.declared_type(id).unqualified())
    }

    /// A `type_name` node (in a cast, `sizeof` or compound literal) as a type.
    ///
    /// Supports specifiers followed by any number of `*`, each with optional
    /// qualifiers --- the cast shapes C code actually writes. An abstract
    /// declarator with `[` or `(` is unknown rather than mis-parsed.
    fn type_name(&self, node: NodeId) -> CType {
        let Some((first, end)) = self.tree.arena().token_extent(node) else {
            return CType::UNKNOWN;
        };
        let offset = self
            .token_spans
            .get(first as usize)
            .map_or(0, |span| span.lo);
        let mut words: Vec<&str> = (first..end)
            .filter_map(|raw| self.token_spans.get(raw as usize))
            .filter_map(|span| self.text.get(span.lo as usize..span.hi as usize))
            .collect();
        // The parser keeps the parentheses inside the node.
        if words.first() == Some(&"(") && words.last() == Some(&")") {
            words = words[1..words.len() - 1].to_vec();
        }
        self.type_from_words(&words, offset)
    }

    /// Specifier words and trailing pointer operators as a type.
    pub(crate) fn type_from_words(&self, words: &[&str], offset: u32) -> CType {
        let star = words.iter().position(|word| *word == "*");
        let (specifiers, declarator) = match star {
            Some(index) => (&words[..index], &words[index..]),
            None => (words, &[][..]),
        };
        if specifiers.is_empty() {
            return CType::UNKNOWN;
        }
        let mut base = self.base_from_specifiers(specifiers, offset);
        if base.is_unknown() && !is_unknown_base_expected(specifiers) {
            return CType::UNKNOWN;
        }
        // Each `*` opens a pointer layer; qualifier words after it belong to
        // that layer. Anything else is a declarator shape not handled here.
        let mut layers: Vec<Layer> = Vec::new();
        for word in declarator {
            match *word {
                "*" => layers.push(Layer::Pointer(Qualifiers::default())),
                "const" | "volatile" | "restrict" | "__restrict" | "__restrict__" | "_Atomic" => {
                    let Some(Layer::Pointer(qualifiers)) = layers.last_mut() else {
                        return CType::UNKNOWN;
                    };
                    match *word {
                        "const" => qualifiers.is_const = true,
                        "volatile" => qualifiers.is_volatile = true,
                        "_Atomic" => qualifiers.is_atomic = true,
                        _ => qualifiers.is_restrict = true,
                    }
                }
                _ => return CType::UNKNOWN,
            }
        }
        // Outermost first: the last `*` written is the outermost layer.
        layers.reverse();
        layers.append(&mut base.layers);
        base.layers = layers;
        base
    }

    /// The base type named by specifier words, resolving typedef names
    /// through the declared types.
    fn base_from_specifiers(&self, specifiers: &[&str], offset: u32) -> CType {
        let mut qualifiers = Qualifiers::default();
        let mut words: Vec<&str> = Vec::new();
        for word in specifiers {
            match *word {
                "const" => qualifiers.is_const = true,
                "volatile" => qualifiers.is_volatile = true,
                "restrict" | "__restrict" | "__restrict__" => qualifiers.is_restrict = true,
                "_Atomic" => qualifiers.is_atomic = true,
                "typedef" | "extern" | "static" | "auto" | "register" | "inline" | "_Noreturn"
                | "__extension__" => {}
                _ => words.push(word),
            }
        }
        if words.is_empty() {
            return CType::UNKNOWN;
        }
        if let Some(kind) = words.first().copied().and_then(record_keyword) {
            let Some(name) = words.get(1) else {
                return CType::UNKNOWN;
            };
            if words.len() != 2 {
                return CType::UNKNOWN;
            }
            let spelling = format!("{kind} {name}");
            let base = match kind {
                "enum" => Base::Enum(spelling),
                _ => Base::Record {
                    spelling,
                    record: self.types.visible_record(
                        if kind == "union" {
                            RecordKind::Union
                        } else {
                            RecordKind::Struct
                        },
                        name,
                        offset,
                    ),
                },
            };
            return CType {
                layers: Vec::new(),
                base,
                base_qualifiers: qualifiers,
            };
        }
        if words.len() == 1 {
            let mut resolved = self.visible_typedef_at(words[0], offset);
            if !resolved.is_unknown() {
                if resolved.layers.is_empty() {
                    resolved.base_qualifiers = merge(resolved.base_qualifiers, qualifiers);
                } else if let Some(Layer::Pointer(existing)) = resolved.layers.first_mut() {
                    *existing = merge(*existing, qualifiers);
                }
                return resolved;
            }
        }
        match builtin_base(&words) {
            Some(base) => CType {
                layers: Vec::new(),
                base,
                base_qualifiers: qualifiers,
            },
            None => CType::UNKNOWN,
        }
    }

    /// A declared type from the structural type graph, or a layered
    /// `Unknown` when part of it could not be established.
    pub(crate) fn declared_type(&self, id: TypeId) -> CType {
        let mut layers: Vec<Layer> = Vec::new();
        let mut pending = Qualifiers::default();
        let mut current = id;
        let mut remaining = 4096u32;
        loop {
            remaining = remaining.saturating_sub(1);
            if remaining == 0 {
                return CType::UNKNOWN;
            }
            match self.types.node(current) {
                Some(TypeNode::Qualified {
                    unqualified,
                    qualifiers,
                }) => {
                    pending = merge(pending, *qualifiers);
                    current = *unqualified;
                }
                Some(TypeNode::Alias {
                    target: Some(target),
                    ..
                })
                | Some(TypeNode::Typeof {
                    captured: Some(target),
                    ..
                }) => current = *target,
                Some(TypeNode::Pointer { pointee }) => {
                    layers.push(Layer::Pointer(pending));
                    pending = Qualifiers::default();
                    current = *pointee;
                }
                Some(TypeNode::Array { element, bound }) => {
                    let bound = match bound {
                        ArrayBound::Constant(text) => text.clone(),
                        ArrayBound::Runtime(slot) => self
                            .types
                            .bound_slot(*slot)
                            .and_then(|slot| {
                                self.text
                                    .get(slot.expression.lo as usize..slot.expression.hi as usize)
                            })
                            .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
                            .unwrap_or_default(),
                        ArrayBound::Incomplete => String::new(),
                        ArrayBound::PrototypeStar => "*".to_owned(),
                    };
                    layers.push(Layer::Array(bound));
                    // Qualifiers on an array apply to its element type.
                    current = *element;
                }
                Some(TypeNode::Function { result }) => {
                    layers.push(Layer::Function);
                    pending = Qualifiers::default();
                    current = *result;
                }
                Some(TypeNode::SpelledBase { specifiers }) => {
                    let words: Vec<&str> = specifiers.split_whitespace().collect();
                    let mut base = self.base_from_specifiers(&words, 0);
                    if base.is_unknown() {
                        base = CType::UNKNOWN;
                    }
                    base.base_qualifiers = merge(base.base_qualifiers, pending);
                    base.layers.splice(0..0, layers);
                    return base;
                }
                Some(TypeNode::Record { record }) => {
                    let spelling = self.types.record_spelling(*record);
                    return CType {
                        layers,
                        base: Base::Record {
                            spelling,
                            record: Some(*record),
                        },
                        base_qualifiers: pending,
                    };
                }
                Some(
                    TypeNode::Unknown { .. }
                    | TypeNode::Alias { target: None, .. }
                    | TypeNode::Typeof { captured: None, .. },
                )
                | None => {
                    return CType {
                        layers,
                        base: Base::Unknown,
                        base_qualifiers: Qualifiers::default(),
                    };
                }
            }
        }
    }

    /// The operator token in each gap between consecutive children.
    fn gap_operators(&self, children: &[NodeId]) -> Vec<String> {
        let arena = self.tree.arena();
        children
            .windows(2)
            .filter_map(|pair| {
                let (_, left_end) = arena.token_extent(pair[0])?;
                let (right_start, _) = arena.token_extent(pair[1])?;
                (left_end..right_start).find_map(|raw| {
                    let span = self.token_spans.get(raw as usize)?;
                    self.text
                        .get(span.lo as usize..span.hi as usize)
                        .map(str::to_owned)
                })
            })
            .collect()
    }

    fn first_token_text(&self, node: NodeId) -> Option<&str> {
        let (first, _) = self.tree.arena().token_extent(node)?;
        let span = self.token_spans.get(first as usize)?;
        self.text.get(span.lo as usize..span.hi as usize)
    }
}

impl CType {
    /// The result type of a function type, or unknown.
    pub(crate) fn pointee_function(mut self) -> CType {
        match self.layers.first() {
            Some(Layer::Function) => {
                self.layers.remove(0);
                self
            }
            _ => CType::UNKNOWN,
        }
    }
}

/// The declared type, name and array shape of one declarator, for export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeclaratorFacts {
    pub(crate) name: Option<String>,
    pub(crate) declared: Option<CType>,
}

impl ExpressionTyper<'_> {
    /// What a `declarator` node declares, when the structural layer resolved
    /// it (locals, parameters and typedefs; a function's own declarator and
    /// record members are not resolved here).
    pub(crate) fn declarator(&self, node: NodeId) -> DeclaratorFacts {
        let arena = self.tree.arena();
        let name_span = arena
            .preorder(node)
            .find(|candidate| arena.tag(*candidate) == Some(NodeTag::DeclName.as_u16()))
            .and_then(|name| arena.span(name, self.token_spans));
        let name = name_span
            .and_then(|span| self.text.get(span.lo as usize..span.hi as usize))
            .map(str::to_owned);
        let declared = name_span
            .and_then(|span| self.types.type_of_declaration(span))
            .map(|id| self.declared_type(id));
        DeclaratorFacts { name, declared }
    }

    /// The adjusted, lvalue-converted type of the object declared at the
    /// declaration-name span `declaration` --- what a read of it yields and
    /// what a store to it converts to --- or unknown when the structural
    /// layer did not resolve it. This is the same lookup [`Self::name_ref`]
    /// makes for a resolved name.
    pub(crate) fn declared_type_at(&self, declaration: Span) -> CType {
        self.types
            .adjusted_type_of_declaration(declaration)
            .map_or(CType::UNKNOWN, |id| self.declared_type(id).unqualified())
    }

    /// The return type of the function definition at `root`, read from its
    /// declaration specifiers and the pointer operators before its name with
    /// the same word rule an unnamed parameter uses, after lvalue conversion.
    /// A shape that rule does not cover (a function returning a function or
    /// array pointer) is unknown.
    pub(crate) fn return_type_of_function(&self, root: NodeId) -> CType {
        let arena = self.tree.arena();
        let mut words: Vec<&str> = Vec::new();
        let mut offset = 0u32;
        let word_at = |raw: u32| -> Option<&str> {
            let span = self.token_spans.get(raw as usize)?;
            self.text
                .get(span.lo as usize..span.hi as usize)
                .map(str::trim)
                .filter(|word| !word.is_empty())
        };
        for child in arena.children_iter(root) {
            match arena.tag(child).and_then(NodeTag::from_u16) {
                Some(NodeTag::DeclSpecifiers) => {
                    let Some((start, end)) = arena.token_extent(child) else {
                        return CType::UNKNOWN;
                    };
                    offset = self
                        .token_spans
                        .get(start as usize)
                        .map_or(0, |span| span.lo);
                    words.extend((start..end).filter_map(word_at));
                }
                Some(NodeTag::Declarator) => {
                    let Some((start, _)) = arena.token_extent(child) else {
                        return CType::UNKNOWN;
                    };
                    let Some((name_start, _)) = arena
                        .preorder(child)
                        .find(|node| arena.tag(*node) == Some(NodeTag::DeclName.as_u16()))
                        .and_then(|name| arena.token_extent(name))
                    else {
                        return CType::UNKNOWN;
                    };
                    words.extend((start..name_start).filter_map(word_at));
                }
                _ => {}
            }
        }
        self.type_from_words(&words, offset).unqualified()
    }

    /// Name, adjusted type and pointer depth of every `param_decl` under
    /// `root`, keyed by the node.
    pub(crate) fn parameters(
        &self,
        root: NodeId,
        function_offset: u32,
    ) -> BTreeMap<NodeId, ParameterFacts> {
        let arena = self.tree.arena();
        let declarations = parameter_declarations(
            self.tree,
            self.text,
            self.token_spans,
            root,
            self.symbols,
            function_offset,
        );
        let Some(list) = arena
            .preorder(root)
            .find(|node| arena.tag(*node) == Some(NodeTag::ParamList.as_u16()))
        else {
            return BTreeMap::new();
        };
        let mut out = BTreeMap::new();
        for node in arena.children_iter(list) {
            if arena.tag(node) != Some(NodeTag::ParamDecl.as_u16()) {
                continue;
            }
            let Some((start, end)) = arena.token_extent(node) else {
                continue;
            };
            let named = declarations
                .named()
                .iter()
                .find(|parameter| parameter.group_start == start && parameter.group_end == end);
            let facts = match named {
                Some(parameter) => ParameterFacts {
                    name: Some(parameter.name.clone()),
                    adjusted: self
                        .types
                        .adjusted_type_of_declaration(parameter.span)
                        .map_or(CType::UNKNOWN, |id| self.declared_type(id)),
                },
                None => {
                    // Unnamed (`void`, `int`, `char *`): a type name.
                    let words: Vec<&str> = (start..end)
                        .filter_map(|raw| self.token_spans.get(raw as usize))
                        .filter_map(|span| self.text.get(span.lo as usize..span.hi as usize))
                        .collect();
                    let offset = self
                        .token_spans
                        .get(start as usize)
                        .map_or(0, |span| span.lo);
                    ParameterFacts {
                        name: None,
                        adjusted: self.type_from_words(&words, offset),
                    }
                }
            };
            out.insert(node, facts);
        }
        out
    }
}

/// One parameter as the export publishes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParameterFacts {
    pub(crate) name: Option<String>,
    /// The type after array and function adjustment (C17 §6.7.6.3p7--8).
    pub(crate) adjusted: CType,
}

fn merge(left: Qualifiers, right: Qualifiers) -> Qualifiers {
    Qualifiers {
        is_const: left.is_const || right.is_const,
        is_volatile: left.is_volatile || right.is_volatile,
        is_restrict: left.is_restrict || right.is_restrict,
        is_atomic: left.is_atomic || right.is_atomic,
    }
}

fn record_keyword(word: &str) -> Option<&'static str> {
    match word {
        "struct" => Some("struct"),
        "union" => Some("union"),
        "enum" => Some("enum"),
        _ => None,
    }
}

/// Whether a specifier list that resolved to no base is *expected* to (a
/// lone typedef name that is simply not declared here), as opposed to one
/// that is malformed. Both yield unknown; this only keeps the shape.
fn is_unknown_base_expected(specifiers: &[&str]) -> bool {
    specifiers.len() == 1
        && specifiers[0]
            .chars()
            .all(|character| character == '_' || character.is_ascii_alphanumeric())
}

/// The standard base type named by keyword words in any order, C17 §6.7.2p2.
fn builtin_base(words: &[&str]) -> Option<Base> {
    let mut signed = false;
    let mut unsigned = false;
    let mut short = false;
    let mut longs = 0u8;
    let mut kind: Option<&str> = None;
    for word in words {
        match *word {
            "signed" | "__signed" | "__signed__" => signed = true,
            "unsigned" => unsigned = true,
            "short" => short = true,
            "long" => longs += 1,
            "int" | "char" | "void" | "float" | "double" | "_Bool" | "__int128" | "bool" => {
                if kind.is_some() {
                    return None;
                }
                kind = Some(word);
            }
            _ => return None,
        }
    }
    if signed && unsigned {
        return None;
    }
    let base = match (kind, short, longs) {
        (Some("void"), false, 0) if !signed && !unsigned => Base::Void,
        (Some("_Bool") | Some("bool"), false, 0) if !signed && !unsigned => {
            Base::Int(IntKind::Bool)
        }
        (Some("float"), false, 0) if !signed && !unsigned => Base::Float(FloatKind::Float),
        (Some("double"), false, 0) if !signed && !unsigned => Base::Float(FloatKind::Double),
        (Some("double"), false, 1) if !signed && !unsigned => Base::Float(FloatKind::LongDouble),
        (Some("char"), false, 0) => Base::Int(if unsigned {
            IntKind::UChar
        } else if signed {
            IntKind::SChar
        } else {
            IntKind::Char
        }),
        (Some("__int128"), false, 0) => Base::Int(if unsigned {
            IntKind::UInt128
        } else {
            IntKind::Int128
        }),
        (Some("int") | None, true, 0) => Base::Int(if unsigned {
            IntKind::UShort
        } else {
            IntKind::Short
        }),
        (Some("int") | None, false, 0) => Base::Int(if unsigned {
            IntKind::UInt
        } else {
            IntKind::Int
        }),
        (Some("int") | None, false, 1) => Base::Int(if unsigned {
            IntKind::ULong
        } else {
            IntKind::Long
        }),
        (Some("int") | None, false, 2) => Base::Int(if unsigned {
            IntKind::ULongLong
        } else {
            IntKind::LongLong
        }),
        _ => return None,
    };
    if kind.is_none() && !signed && !unsigned && !short && longs == 0 {
        return None;
    }
    Some(base)
}

/// The type of an integer constant, C17 §6.4.4.1p5, under LP64 widths.
pub(crate) fn integer_literal(text: &str) -> CType {
    let lower = text.to_ascii_lowercase();
    // Split the digit run from the suffix by radix first: a hex literal's last
    // digit can be a letter that would otherwise read as a suffix.
    let split_at = |skip: usize, digit: fn(char) -> bool| -> usize {
        lower[skip..]
            .find(|character: char| !(digit(character) || character == '\''))
            .map_or(lower.len(), |offset| offset + skip)
    };
    let (radix, digits_start, end) = if lower.starts_with("0x") {
        (16, 2, split_at(2, |c| c.is_ascii_hexdigit()))
    } else if lower.starts_with("0b") {
        (2, 2, split_at(2, |c| c == '0' || c == '1'))
    } else if lower.len() > 1 && lower.starts_with('0') {
        (8, 1, split_at(1, |c| c.is_ascii_digit()))
    } else {
        (10, 0, split_at(0, |c| c.is_ascii_digit()))
    };
    let suffix = &lower[end..];
    let digits: String = lower[digits_start..end]
        .chars()
        .filter(|character| *character != '\'')
        .collect();
    let Ok(value) = u128::from_str_radix(if digits.is_empty() { "0" } else { &digits }, radix)
    else {
        return CType::UNKNOWN;
    };
    let (has_u, longs) = match suffix {
        "" => (false, 0),
        "u" => (true, 0),
        "l" => (false, 1),
        "ul" | "lu" => (true, 1),
        "ll" => (false, 2),
        "ull" | "llu" => (true, 2),
        _ => return CType::UNKNOWN,
    };
    // A 128-bit candidate never appears below, but a shift by the full width
    // would still be an overflow; guard it rather than rely on the table.
    let fits = |bits: u32| bits >= 128 || value < (1u128 << bits);
    let decimal = radix == 10;
    let candidates: &[IntKind] = match (has_u, longs, decimal) {
        (false, 0, true) => &[IntKind::Int, IntKind::Long, IntKind::LongLong],
        (false, 0, false) => &[
            IntKind::Int,
            IntKind::UInt,
            IntKind::Long,
            IntKind::ULong,
            IntKind::LongLong,
            IntKind::ULongLong,
        ],
        (true, 0, _) => &[IntKind::UInt, IntKind::ULong, IntKind::ULongLong],
        (false, 1, true) => &[IntKind::Long, IntKind::LongLong],
        (false, 1, false) => &[
            IntKind::Long,
            IntKind::ULong,
            IntKind::LongLong,
            IntKind::ULongLong,
        ],
        (true, 1, _) => &[IntKind::ULong, IntKind::ULongLong],
        (false, 2, true) => &[IntKind::LongLong],
        (false, 2, false) => &[IntKind::LongLong, IntKind::ULongLong],
        (true, 2, _) => &[IntKind::ULongLong],
        _ => return CType::UNKNOWN,
    };
    candidates
        .iter()
        .copied()
        .find(|kind| {
            if kind.is_unsigned() {
                fits(kind.width())
            } else {
                fits(kind.width() - 1)
            }
        })
        .map_or(CType::UNKNOWN, CType::int)
}

/// The type of a floating constant, C17 §6.4.4.2p4.
fn float_literal(text: &str) -> CType {
    let lower = text.to_ascii_lowercase();
    if lower.contains('i') || lower.contains('j') {
        // GNU imaginary constant.
        return CType::UNKNOWN;
    }
    let suffix = lower
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_ascii_alphabetic())
        .map(|(_, character)| character)
        .collect::<String>();
    let suffix: String = suffix.chars().rev().collect();
    // A hex float's exponent marker `p` and a decimal's `e` are not suffixes.
    let suffix = suffix.trim_start_matches(['e', 'p']);
    match suffix {
        "" => CType::scalar(Base::Float(FloatKind::Double)),
        "f" => CType::scalar(Base::Float(FloatKind::Float)),
        "l" => CType::scalar(Base::Float(FloatKind::LongDouble)),
        _ => CType::UNKNOWN,
    }
}

/// The number of bytes a narrow (or UTF-8) string literal token contributes
/// to its array, excluding the terminator, or `None` for a wide literal or
/// one this decoder does not understand.
fn narrow_string_bytes(token: &str) -> Option<usize> {
    let body = token
        .strip_prefix("u8")
        .unwrap_or(token)
        .strip_prefix('"')?
        .strip_suffix('"')?;
    let bytes = body.as_bytes();
    let mut count = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'\\' {
            count += 1;
            index += 1;
            continue;
        }
        index += 1;
        let escape = *bytes.get(index)?;
        index += 1;
        match escape {
            b'n' | b't' | b'\\' | b'\'' | b'"' | b'?' | b'a' | b'b' | b'f' | b'r' | b'v' | b'e'
            | b'E' => count += 1,
            b'0'..=b'7' => {
                let mut taken = 1;
                while taken < 3 && index < bytes.len() && (b'0'..=b'7').contains(&bytes[index]) {
                    index += 1;
                    taken += 1;
                }
                count += 1;
            }
            b'x' => {
                let start = index;
                while index < bytes.len() && bytes[index].is_ascii_hexdigit() {
                    index += 1;
                }
                if index == start {
                    return None;
                }
                count += 1;
            }
            b'u' | b'U' => {
                let width = if escape == b'u' { 4 } else { 8 };
                let hex = body.get(index..index + width)?;
                let code = u32::from_str_radix(hex, 16).ok()?;
                count += char::from_u32(code)?.len_utf8();
                index += width;
            }
            _ => return None,
        }
    }
    Some(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn promotion_and_usual_conversions_follow_the_standard() {
        assert_eq!(IntKind::UShort.promoted(), IntKind::Int);
        assert_eq!(IntKind::Char.promoted(), IntKind::Int);
        assert_eq!(IntKind::Bool.promoted(), IntKind::Int);
        assert_eq!(IntKind::UInt.promoted(), IntKind::UInt);
        assert_eq!(IntKind::Int.usual(IntKind::UInt), IntKind::UInt);
        assert_eq!(IntKind::Long.usual(IntKind::UInt), IntKind::Long);
        assert_eq!(IntKind::Int.usual(IntKind::ULong), IntKind::ULong);
        assert_eq!(IntKind::LongLong.usual(IntKind::ULong), IntKind::ULongLong);
        assert_eq!(IntKind::UShort.usual(IntKind::Short), IntKind::Int);
        assert_eq!(IntKind::Long.usual(IntKind::Int), IntKind::Long);
    }

    #[test]
    fn integer_literals_take_the_first_type_that_fits() {
        assert_eq!(integer_literal("1").render(), "int");
        assert_eq!(integer_literal("2147483647").render(), "int");
        assert_eq!(integer_literal("2147483648").render(), "long");
        assert_eq!(integer_literal("0x7fffffff").render(), "int");
        assert_eq!(integer_literal("0x80000000").render(), "unsigned int");
        assert_eq!(integer_literal("0xffffffff").render(), "unsigned int");
        assert_eq!(integer_literal("0x100000000").render(), "long");
        assert_eq!(integer_literal("1u").render(), "unsigned int");
        assert_eq!(integer_literal("1UL").render(), "unsigned long");
        assert_eq!(integer_literal("1LU").render(), "unsigned long");
        assert_eq!(integer_literal("1ll").render(), "long long");
        assert_eq!(integer_literal("1ull").render(), "unsigned long long");
        assert_eq!(integer_literal("0").render(), "int");
        assert_eq!(integer_literal("017").render(), "int");
        assert_eq!(integer_literal("4294967295u").render(), "unsigned int");
        assert_eq!(integer_literal("4294967296u").render(), "unsigned long");
        assert_eq!(
            integer_literal("99999999999999999999999999999999999999999").render(),
            "unknown"
        );
        assert_eq!(integer_literal("1wb").render(), "unknown");
    }

    #[test]
    fn floating_literals_are_typed_by_suffix() {
        assert_eq!(float_literal("1.0").render(), "double");
        assert_eq!(float_literal("1.0f").render(), "float");
        assert_eq!(float_literal("1e10").render(), "double");
        assert_eq!(float_literal("1.0L").render(), "long double");
        assert_eq!(float_literal("0x1.8p3").render(), "double");
        assert_eq!(float_literal("0x1p3f").render(), "float");
    }

    #[test]
    fn string_literal_lengths_decode_escapes() {
        assert_eq!(narrow_string_bytes("\"abc\""), Some(3));
        assert_eq!(narrow_string_bytes("\"a\\n\\x41\\101\""), Some(4));
        assert_eq!(narrow_string_bytes("\"\\u00e9\""), Some(2));
        assert_eq!(narrow_string_bytes("\"\u{4e2d}\""), Some(3));
        assert_eq!(narrow_string_bytes("L\"abc\""), None);
    }

    #[test]
    fn rendering_composes_declarators_correctly() {
        let mut ty = CType::int(IntKind::Int);
        assert_eq!(ty.render(), "int");
        ty.layers.push(Layer::Array("4".into()));
        assert_eq!(ty.render(), "int[4]");
        ty = ty.address_of();
        assert_eq!(ty.render(), "int (*)[4]");
        let mut pointer = CType::int(IntKind::Char).address_of();
        assert_eq!(pointer.render(), "char *");
        pointer.layers[0] = Layer::Pointer(Qualifiers {
            is_const: true,
            ..Qualifiers::default()
        });
        assert_eq!(pointer.render(), "char *const");
        let mut const_char = CType::int(IntKind::Char);
        const_char.base_qualifiers.is_const = true;
        assert_eq!(const_char.address_of().render(), "const char *");
        assert_eq!(CType::UNKNOWN.address_of().render(), "unknown *");
    }

    #[test]
    fn builtin_bases_accept_any_keyword_order() {
        assert_eq!(
            builtin_base(&["long", "unsigned", "int"]),
            Some(Base::Int(IntKind::ULong))
        );
        assert_eq!(
            builtin_base(&["long", "long"]),
            Some(Base::Int(IntKind::LongLong))
        );
        assert_eq!(builtin_base(&["short"]), Some(Base::Int(IntKind::Short)));
        assert_eq!(builtin_base(&["signed"]), Some(Base::Int(IntKind::Int)));
        assert_eq!(
            builtin_base(&["long", "double"]),
            Some(Base::Float(FloatKind::LongDouble))
        );
        assert_eq!(builtin_base(&["unsigned", "double"]), None);
        assert_eq!(builtin_base(&["uint32_t"]), None);
        assert_eq!(builtin_base(&[]), None);
    }
}
