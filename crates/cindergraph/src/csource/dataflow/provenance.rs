//! Parameter provenance follows reaching definitions, not spelling occurrences.
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use super::{DataFlow, DefKind, Sink, Summary};
use crate::syntax::ids::Span;

fn contains(outer: Span, inner: Span) -> bool {
    outer.lo <= inner.lo && inner.hi <= outer.hi
}

/// A use contributes to an expression only if its value is not discarded by
/// a comma operator and each enclosing call propagates the corresponding
/// argument to its return value. Side effects still use their own spans.
fn passes_calls(
    flow: &DataFlow,
    use_span: Span,
    expression: Span,
    known: &BTreeMap<String, Summary>,
) -> bool {
    if flow
        .discarded_values
        .iter()
        .any(|(owner, discarded)| contains(expression, *owner) && contains(*discarded, use_span))
    {
        return false;
    }
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

/// Function-local adjacency shared by every parameter provenance trace.
///
/// None of these indexes depends on the selected parameter or on the current
/// interprocedural summaries. Building them once keeps wide signatures from
/// repeatedly scanning the same definitions and edges.
pub(super) struct TraceIndex {
    successors: Vec<Vec<usize>>,
    writes_by_node: BTreeMap<u32, Vec<usize>>,
    control_successors: BTreeMap<u32, Vec<u32>>,
    writes_containing_use: OnceLock<Vec<Vec<usize>>>,
}

impl TraceIndex {
    pub(super) fn new(flow: &DataFlow) -> Self {
        let mut successors = vec![Vec::new(); flow.definitions.len()];
        for edge in &flow.edges {
            if let Some(out) = successors.get_mut(edge.def as usize) {
                out.push(edge.use_ as usize);
            }
        }
        let mut writes_by_node: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
        for (index, definition) in flow.definitions.iter().enumerate() {
            if !matches!(definition.kind, DefKind::Parameter | DefKind::AddressTaken) {
                writes_by_node
                    .entry(definition.node)
                    .or_default()
                    .push(index);
            }
        }
        let mut control_successors: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for &(on, node) in &flow.control_edges {
            control_successors.entry(on).or_default().push(node);
        }
        Self {
            successors,
            writes_by_node,
            control_successors,
            writes_containing_use: OnceLock::new(),
        }
    }

    fn writes_containing_use(&self, flow: &DataFlow) -> &[Vec<usize>] {
        self.writes_containing_use.get_or_init(|| {
            let mut containing = vec![Vec::new(); flow.uses.len()];
            let mut writes: Vec<(Span, usize)> = flow
                .definitions
                .iter()
                .enumerate()
                .filter(|(_, write)| {
                    !matches!(write.kind, DefKind::Parameter | DefKind::AddressTaken)
                })
                .map(|(target, write)| {
                    (
                        Span {
                            lo: write.span.lo,
                            hi: write.effect_at,
                        },
                        target,
                    )
                })
                .collect();
            writes.sort_unstable_by_key(|(span, target)| (span.lo, *target));
            let mut uses: Vec<usize> = (0..flow.uses.len()).collect();
            uses.sort_unstable_by_key(|use_index| {
                let span = flow.uses[*use_index].span;
                (span.lo, span.hi, *use_index)
            });

            let mut next_write = 0usize;
            let mut active: Vec<(Span, usize)> = Vec::new();
            for use_index in uses {
                let use_span = flow.uses[use_index].span;
                while next_write < writes.len() && writes[next_write].0.lo <= use_span.lo {
                    active.push(writes[next_write]);
                    next_write += 1;
                }
                // Source-ordered future uses cannot be contained by a region
                // that already ends before this use begins.
                active.retain(|(span, _)| span.hi >= use_span.lo);
                for &(expression, target) in &active {
                    if contains(expression, use_span) {
                        containing[use_index].push(target);
                    }
                }
            }
            containing
        })
    }
}

pub(super) fn uses(
    flow: &DataFlow,
    index: &TraceIndex,
    parameter_definition: usize,
    known: &BTreeMap<String, Summary>,
) -> Vec<bool> {
    trace(flow, index, parameter_definition, known).0
}

fn trace(
    flow: &DataFlow,
    index: &TraceIndex,
    parameter_definition: usize,
    known: &BTreeMap<String, Summary>,
) -> (Vec<bool>, BTreeSet<u32>) {
    let mut definitions = vec![false; flow.definitions.len()];
    let mut pending = Vec::with_capacity(1);
    if flow
        .definitions
        .get(parameter_definition)
        .is_some_and(|definition| definition.kind == DefKind::Parameter)
    {
        definitions[parameter_definition] = true;
        pending.push(parameter_definition);
    }
    let mut uses = vec![false; flow.uses.len()];
    let mut controlled = BTreeSet::new();
    // Each definition and use enters the queue at most once. This is the same
    // finite reachability closure as repeated full scans, without one round per
    // assignment in a long chain.
    while let Some(definition) = pending.pop() {
        for &use_index in &index.successors[definition] {
            let Some(use_) = flow.uses.get(use_index) else {
                continue;
            };
            if uses[use_index] {
                continue;
            }
            uses[use_index] = true;
            // A conditional expression can span multiple CFG nodes; source
            // containment, not node equality, associates its reads and write.
            for &target in &index.writes_containing_use(flow)[use_index] {
                if definitions[target] {
                    continue;
                }
                let write = &flow.definitions[target];
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
            // A condition inside a discarded comma operand can still govern
            // side effects. Evaluate that condition's own extent, not the
            // enclosing function/return expression whose value discards it.
            if flow
                .node_spans
                .get(use_.node as usize)
                .is_some_and(|span| passes_calls(flow, use_.span, *span, known))
            {
                let mut branches = vec![use_.node];
                while let Some(branch) = branches.pop() {
                    for &node in index.control_successors.get(&branch).into_iter().flatten() {
                        if !controlled.insert(node) {
                            continue;
                        }
                        branches.push(node);
                        for &target in index.writes_by_node.get(&node).into_iter().flatten() {
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
        let is_memory_address = flow
            .types
            .get(use_.binding.0 as usize)
            .is_some_and(|ty| ty.pointer_depth > 0 || ty.array_rank > 0)
            && flow
                .memory_accesses
                .iter()
                .any(|access| access.span != use_.span && contains(access.span, use_.span));
        uses[i]
            && !is_memory_address
            && contains(span, use_.span)
            && passes_calls(flow, use_.span, span, known)
    })
}

pub(super) fn returns(
    flow: &DataFlow,
    index: &TraceIndex,
    parameter_definition: usize,
    known: &BTreeMap<String, Summary>,
) -> bool {
    let (uses, controlled) = trace(flow, index, parameter_definition, known);
    flow.return_nodes
        .iter()
        .any(|node| controlled.contains(node))
        || flow
            .return_spans
            .iter()
            .any(|span| expression(flow, &uses, *span, known))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interval_sweep_matches_definition_by_use_linear_reference() {
        let sources = [
            "int f(int a, int b) { int x = a + b; return x; }",
            "int f(int a, int b) { int x = (a ? b : a), y = x + b; return y; }",
            "int f(int a) { int x; x = (x = a + 1) + a; return x; }",
            "int f(int a) { if (a) { int x = a; return x; } return a; }",
            "int f( { int x = ???; return x;",
        ];
        for source in sources {
            for flow in super::super::analyze(source).into_parts().0 {
                let index = TraceIndex::new(&flow);
                let actual = index.writes_containing_use(&flow);
                for (use_index, use_) in flow.uses.iter().enumerate() {
                    let expected: Vec<usize> = flow
                        .definitions
                        .iter()
                        .enumerate()
                        .filter(|(_, write)| {
                            !matches!(write.kind, DefKind::Parameter | DefKind::AddressTaken)
                        })
                        .filter(|(_, write)| {
                            contains(
                                Span {
                                    lo: write.span.lo,
                                    hi: write.effect_at,
                                },
                                use_.span,
                            )
                        })
                        .map(|(target, _)| target)
                        .collect();
                    let mut observed = actual[use_index].clone();
                    observed.sort_unstable();
                    assert_eq!(observed, expected, "{source}: {:?}", use_.span);
                }
            }
        }
    }
}
