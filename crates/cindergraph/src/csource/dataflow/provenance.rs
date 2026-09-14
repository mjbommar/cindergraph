//! Parameter provenance follows reaching definitions, not spelling occurrences.
use std::collections::{BTreeMap, BTreeSet};

use super::{Binding, DataFlow, DefKind, Sink, Summary};
use crate::syntax::ids::Span;

fn contains(outer: Span, inner: Span) -> bool {
    outer.lo <= inner.lo && inner.hi <= outer.hi
}

/// A use contributes to an expression only if each enclosing call in that
/// expression propagates the corresponding argument to its return value.
fn passes_calls(
    flow: &DataFlow,
    use_span: Span,
    expression: Span,
    known: &BTreeMap<String, Summary>,
) -> bool {
    flow.calls
        .iter()
        .filter(|call| contains(expression, call.span) && contains(call.span, use_span))
        .all(|call| {
            let Some(summary) = call.callee.as_ref().and_then(|name| known.get(name)) else {
                return false;
            };
            call.argument_spans
                .iter()
                .enumerate()
                .any(|(position, span)| {
                    contains(*span, use_span) && summary.flows_to(position as u32, Sink::Return)
                })
        })
}

pub(super) fn uses(
    flow: &DataFlow,
    binding: Binding,
    known: &BTreeMap<String, Summary>,
) -> Vec<bool> {
    trace(flow, binding, known).0
}

fn trace(
    flow: &DataFlow,
    binding: Binding,
    known: &BTreeMap<String, Summary>,
) -> (Vec<bool>, BTreeSet<u32>) {
    let mut successors = vec![Vec::new(); flow.definitions.len()];
    for edge in &flow.edges {
        if let Some(out) = successors.get_mut(edge.def as usize) {
            out.push(edge.use_ as usize);
        }
    }
    let mut writes_by_node: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    let mut definitions = vec![false; flow.definitions.len()];
    let mut pending = Vec::new();
    for (index, definition) in flow.definitions.iter().enumerate() {
        if definition.kind == DefKind::Parameter && definition.binding == binding {
            definitions[index] = true;
            pending.push(index);
        } else if !matches!(definition.kind, DefKind::Parameter | DefKind::AddressTaken) {
            writes_by_node
                .entry(definition.node)
                .or_default()
                .push(index);
        }
    }
    let mut uses = vec![false; flow.uses.len()];
    let mut controlled = BTreeSet::new();
    let mut control_successors: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for &(on, node) in &flow.control_edges {
        control_successors.entry(on).or_default().push(node);
    }
    // Each definition and use enters the queue at most once. This is the same
    // finite reachability closure as repeated full scans, without one round per
    // assignment in a long chain.
    while let Some(definition) = pending.pop() {
        for &index in &successors[definition] {
            let Some(use_) = flow.uses.get(index) else {
                continue;
            };
            if uses[index] {
                continue;
            }
            uses[index] = true;
            // A conditional expression can span multiple CFG nodes; source
            // containment, not node equality, associates its reads and write.
            for (target, write) in flow.definitions.iter().enumerate() {
                if definitions[target] {
                    continue;
                }
                if matches!(write.kind, DefKind::Parameter | DefKind::AddressTaken) {
                    continue;
                }
                let expression = Span {
                    lo: write.span.lo,
                    hi: write.effect_at,
                };
                if contains(expression, use_.span)
                    && passes_calls(flow, use_.span, expression, known)
                {
                    definitions[target] = true;
                    pending.push(target);
                }
            }
            if passes_calls(
                flow,
                use_.span,
                Span {
                    lo: 0,
                    hi: u32::MAX,
                },
                known,
            ) {
                let mut branches = vec![use_.node];
                while let Some(branch) = branches.pop() {
                    for &node in control_successors.get(&branch).into_iter().flatten() {
                        if !controlled.insert(node) {
                            continue;
                        }
                        branches.push(node);
                        for &target in writes_by_node.get(&node).into_iter().flatten() {
                            if !definitions[target] {
                                definitions[target] = true;
                                pending.push(target);
                            }
                        }
                    }
                }
            }
        }
    }
    (uses, controlled)
}

pub(super) fn expression(
    flow: &DataFlow,
    uses: &[bool],
    span: Span,
    known: &BTreeMap<String, Summary>,
) -> bool {
    flow.uses.iter().enumerate().any(|(i, use_)| {
        uses[i] && contains(span, use_.span) && passes_calls(flow, use_.span, span, known)
    })
}

pub(super) fn returns(
    flow: &DataFlow,
    binding: Binding,
    known: &BTreeMap<String, Summary>,
) -> bool {
    let (uses, controlled) = trace(flow, binding, known);
    flow.return_nodes
        .iter()
        .any(|node| controlled.contains(node))
        || flow
            .return_spans
            .iter()
            .any(|span| expression(flow, &uses, *span, known))
}
