//! Reaching definitions over abstract memory regions.
//!
//! This lattice is deliberately separate from scalar bindings. An exact
//! region write is a strong update and replaces earlier definitions of that
//! region. A may-alias write is a weak update and joins without killing.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::syntax::cfg::Cfg;
use crate::syntax::ids::NodeId;

use super::{DataFlow, MemoryAccessPrecision, MemoryFlowEdge, MemoryOverlapKind, MemoryRegionId};

type State = BTreeMap<MemoryRegionId, BTreeSet<u32>>;

fn reachable_nodes(cfg: &Cfg) -> Vec<bool> {
    let mut reachable = vec![false; cfg.node_count()];
    if cfg.node_count() == 0 {
        return reachable;
    }
    let mut pending = VecDeque::from([cfg.entry()]);
    while let Some(node) = pending.pop_front() {
        if node.index() >= reachable.len() || reachable[node.index()] {
            continue;
        }
        reachable[node.index()] = true;
        pending.extend(cfg.successors(node));
    }
    reachable
}

fn merge_predecessors(node: usize, cfg: &Cfg, reachable: &[bool], out: &[State]) -> State {
    if node == cfg.entry().index() {
        return State::new();
    }
    let mut incoming = State::new();
    for predecessor in cfg.predecessors(NodeId::new(node as u32)) {
        if !reachable[predecessor.index()] {
            continue;
        }
        for (&region, definitions) in &out[predecessor.index()] {
            incoming
                .entry(region)
                .or_default()
                .extend(definitions.iter().copied());
        }
    }
    incoming
}

fn apply_definition(
    flow: &DataFlow,
    state: &mut State,
    overlap_neighbors: &[Vec<(MemoryRegionId, MemoryOverlapKind)>],
    index: u32,
) {
    let definition = &flow.memory_definitions[index as usize];
    if definition.precision == MemoryAccessPrecision::Exact {
        state.remove(&definition.region);
        if let Some(neighbors) = overlap_neighbors.get(definition.region.0 as usize) {
            for &(neighbor, kind) in neighbors {
                if kind == MemoryOverlapKind::UnionMembers {
                    state.remove(&neighbor);
                }
            }
        }
    }
    state.entry(definition.region).or_default().insert(index);
}

/// Solve possible reaching writes for every operation-owned region read.
pub(super) fn solve(flow: &mut DataFlow, cfg: &Cfg) {
    flow.memory_edges.clear();
    if cfg.node_count() == 0 || flow.memory_definitions.is_empty() || flow.memory_uses.is_empty() {
        return;
    }

    let reachable = reachable_nodes(cfg);
    let mut overlap_neighbors = vec![Vec::new(); flow.memory_regions.len()];
    for overlap in &flow.memory_overlaps {
        if !matches!(
            overlap.kind,
            MemoryOverlapKind::UnionMembers | MemoryOverlapKind::ParameterAlias
        ) {
            continue;
        }
        overlap_neighbors[overlap.left.0 as usize].push((overlap.right, overlap.kind));
        overlap_neighbors[overlap.right.0 as usize].push((overlap.left, overlap.kind));
    }
    let mut definitions_by_node = vec![Vec::<u32>::new(); cfg.node_count()];
    for (index, definition) in flow.memory_definitions.iter().enumerate() {
        if let Some(node) = definitions_by_node.get_mut(definition.node as usize) {
            node.push(index as u32);
        }
    }
    for definitions in &mut definitions_by_node {
        definitions.sort_unstable_by_key(|&index| {
            let definition = &flow.memory_definitions[index as usize];
            (definition.effect_at, index)
        });
    }

    let mut out = vec![State::new(); cfg.node_count()];
    let mut pending = VecDeque::new();
    let mut queued = vec![false; cfg.node_count()];
    for (node, is_reachable) in reachable.iter().copied().enumerate() {
        if is_reachable {
            pending.push_back(node);
            queued[node] = true;
        }
    }
    while let Some(node) = pending.pop_front() {
        queued[node] = false;
        let mut next = merge_predecessors(node, cfg, &reachable, &out);
        for &definition in &definitions_by_node[node] {
            apply_definition(flow, &mut next, &overlap_neighbors, definition);
        }
        if next == out[node] {
            continue;
        }
        out[node] = next;
        for successor in cfg.successors(NodeId::new(node as u32)) {
            if reachable[successor.index()] && !queued[successor.index()] {
                queued[successor.index()] = true;
                pending.push_back(successor.index());
            }
        }
    }

    let mut uses_by_node = vec![Vec::<u32>::new(); cfg.node_count()];
    for (index, use_) in flow.memory_uses.iter().enumerate() {
        if let Some(node) = uses_by_node.get_mut(use_.node as usize) {
            node.push(index as u32);
        }
    }
    for uses in &mut uses_by_node {
        uses.sort_unstable_by_key(|&index| {
            let use_ = &flow.memory_uses[index as usize];
            (use_.span.lo, index)
        });
    }

    for node in 0..cfg.node_count() {
        if !reachable[node] {
            continue;
        }
        let mut state = merge_predecessors(node, cfg, &reachable, &out);
        let definitions = &definitions_by_node[node];
        let mut next_definition = 0usize;
        for &use_index in &uses_by_node[node] {
            let use_ = &flow.memory_uses[use_index as usize];
            while next_definition < definitions.len()
                && flow.memory_definitions[definitions[next_definition] as usize].effect_at
                    <= use_.span.lo
            {
                apply_definition(
                    flow,
                    &mut state,
                    &overlap_neighbors,
                    definitions[next_definition],
                );
                next_definition += 1;
            }
            let mut reaching = state
                .get(&use_.region)
                .into_iter()
                .flatten()
                .map(|&definition| (definition, None))
                .collect::<BTreeMap<_, _>>();
            if let Some(neighbors) = overlap_neighbors.get(use_.region.0 as usize) {
                for &(neighbor, kind) in neighbors {
                    if let Some(definitions) = state.get(&neighbor) {
                        for &definition in definitions {
                            reaching.entry(definition).or_insert(Some(kind));
                        }
                    }
                }
            }
            flow.memory_edges
                .extend(
                    reaching
                        .into_iter()
                        .map(|(definition, overlap)| MemoryFlowEdge {
                            definition,
                            use_: use_index,
                            definition_region: flow.memory_definitions[definition as usize].region,
                            use_region: use_.region,
                            overlap,
                        }),
                );
        }
    }
}
