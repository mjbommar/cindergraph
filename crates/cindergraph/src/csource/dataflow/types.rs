//! Reading the declared type of every name.
//!
//! Split from [`super::events`] because the two answer different questions
//! about the same tree: that module decides which names are written and read,
//! this one decides what each name was declared as. They share the tree walk's
//! helpers and nothing else.
//!
//! # As written, not resolved
//!
//! This front end reads one translation unit and does not process `#include`
//! (`REQ-GEN`), so a typedef from a header is an opaque name and is recorded
//! as one. `uint32_t` is stored as `uint32_t`; nothing here claims to know it
//! is four bytes.
//!
//! Deliberately **not** built on `crate::metrics::type_name::normalize_type`,
//! which reproduces four defects in DecBench's reference implementation on
//! purpose --- it emits the non-C spelling `long long long`, and turns `_Bool`
//! into `_bool` --- because parity with the benchmark is its contract. A
//! consumer who wants the type the programmer wrote needs a different reader,
//! so this is one.

use crate::csource::parse::tag::NodeTag;
use crate::csource::parse::Tree;
use crate::csource::semantic::declarations::{name_of, ParameterDeclarator};
use crate::csource::semantic::types::FunctionTypes;
use crate::syntax::ids::{NodeId, Span};

use super::model::CType;

/// The declared type of every name in this function, keyed by the name's span.
///
/// A declaration is `DeclSpecifiers` followed by one or more declarators, and
/// the specifiers apply to all of them: in `int a = 1, *b, c[4];` every name
/// has base `int`, and `b` additionally has a pointer and `c` an array rank.
/// So this reads the specifiers once per `Decl` and then reads each
/// [`NodeTag::Declarator`] independently. Initializer expressions are sibling
/// nodes, never part of a declarator's token extent; their `*` and `[` tokens
/// therefore cannot contaminate the following or preceding binding's type.
///
/// Parameters consume the same recovered declaration records as event binding,
/// so type and value identity cannot diverge through duplicate scans.
pub(super) fn declared_types(
    tree: &Tree,
    text: &str,
    token_spans: &[Span],
    root: NodeId,
    parameters: &[ParameterDeclarator],
    structural: &FunctionTypes,
) -> Vec<(Span, CType)> {
    let arena = tree.arena();
    let mut out: Vec<(Span, CType)> = Vec::new();

    for node in arena.preorder(root) {
        if arena.tag(node) != Some(NodeTag::Decl.as_u16()) {
            continue;
        }
        // The specifier text shared by every declarator.
        let mut specifiers = String::new();
        for inner in arena.preorder(node) {
            if arena.tag(inner) != Some(NodeTag::DeclSpecifiers.as_u16()) {
                continue;
            }
            if let Some(span) = arena.span(inner, token_spans) {
                if let Some(slice) = text.get(span.lo as usize..span.hi as usize) {
                    specifiers = slice.split_whitespace().collect::<Vec<_>>().join(" ");
                }
            }
            break;
        }

        let base = CType {
            is_const: has_word(&specifiers, "const"),
            is_volatile: has_word(&specifiers, "volatile"),
            is_static: has_word(&specifiers, "static"),
            is_extern: has_word(&specifiers, "extern"),
            specifiers,
            ..CType::default()
        };

        // Declarators are direct children of the declaration. Keeping this
        // walk at one level also prevents a declaration inside a statement
        // expression used as an initializer from being attributed to its
        // enclosing declaration.
        for declarator in arena.children_iter(node) {
            if arena.tag(declarator) != Some(NodeTag::Declarator.as_u16()) {
                continue;
            }
            let Some(span) = arena.preorder(declarator).find_map(|inner| {
                (arena.tag(inner) == Some(NodeTag::DeclName.as_u16()))
                    .then(|| name_of(tree, text, token_spans, inner))
                    .flatten()
                    .map(|(_, span)| span)
            }) else {
                continue;
            };
            let mut ty = base.clone();
            if let Some(shape) = structural.shape_of_declaration(span) {
                ty.specifiers = shape.specifiers.to_owned();
                ty.pointer_depth = shape.pointer_depth;
                ty.array_rank = shape.array_rank;
            }
            out.push((span, ty));
        }
    }

    // Parameters, from the outermost parameter list's tokens.
    for (span, ty) in parameter_types(parameters, structural) {
        out.push((span, ty));
    }
    out
}

/// The spelling-oriented view of every structurally resolved parameter type.
fn parameter_types(
    parameters: &[ParameterDeclarator],
    structural: &FunctionTypes,
) -> Vec<(Span, CType)> {
    let mut out = Vec::new();
    for parameter in parameters {
        let Some(shape) = structural.shape_of_declaration(parameter.span) else {
            continue;
        };
        let specifiers = shape.specifiers.to_owned();
        out.push((
            parameter.span,
            CType {
                is_const: has_word(&specifiers, "const"),
                is_volatile: has_word(&specifiers, "volatile"),
                is_static: has_word(&specifiers, "static"),
                is_extern: has_word(&specifiers, "extern"),
                specifiers,
                pointer_depth: shape.pointer_depth,
                array_rank: shape.array_rank,
            },
        ));
    }
    out
}

/// Whether `haystack` contains `word` as a whole word.
fn has_word(haystack: &str, word: &str) -> bool {
    haystack
        .split_whitespace()
        .any(|candidate| candidate == word)
}
