//! The reaching-definitions fixpoint.
//!
//! Pure graph work: it reads [`super::model::Definition`] and
//! [`super::model::Use`] positions and a [`Cfg`], and knows nothing about C.
//! That separation is deliberate --- the fixpoint is the part most likely to
//! be reused by a second front end, and it is the part that has to be right
//! about the lattice rather than about the grammar.

use std::collections::{BTreeMap, VecDeque};

use crate::syntax::cfg::Cfg;
use crate::syntax::ids::NodeId;

use super::model::{Binding, DataFlow, DefKind, FlowEdge};

const WORD_BITS: usize = u64::BITS as usize;

fn contains_bit(words: &[u64], index: usize) -> bool {
    words
        .get(index / WORD_BITS)
        .is_some_and(|word| word & (1 << (index % WORD_BITS)) != 0)
}

fn set_bit(words: &mut [u64], index: usize, value: bool) {
    let Some(word) = words.get_mut(index / WORD_BITS) else {
        return;
    };
    let mask = 1 << (index % WORD_BITS);
    if value {
        *word |= mask;
    } else {
        *word &= !mask;
    }
}

/// Node order when the entire CFG is one unambiguous chain.
fn linear_order(cfg: &Cfg) -> Option<Vec<usize>> {
    if cfg.node_count() == 0 {
        return None;
    }
    let mut order = Vec::with_capacity(cfg.node_count());
    let mut seen = vec![false; cfg.node_count()];
    let mut node = cfg.entry();
    loop {
        if seen[node.index()] {
            return None;
        }
        seen[node.index()] = true;
        order.push(node.index());
        let mut successors = cfg.successors(node);
        let next = successors.next();
        if successors.next().is_some() {
            return None;
        }
        match next {
            None => break,
            Some(next) => node = next,
        }
    }
    (order.len() == cfg.node_count()).then_some(order)
}

/// A deterministic topological order for a wholly entry-reachable DAG.
fn acyclic_order(cfg: &Cfg) -> Option<Vec<usize>> {
    let count = cfg.node_count();
    if count == 0 {
        return None;
    }
    let mut indegree = vec![0usize; count];
    for node in 0..count {
        for successor in cfg.successors(NodeId::new(node as u32)) {
            indegree[successor.index()] = indegree[successor.index()].saturating_add(1);
        }
    }
    let roots: Vec<_> = indegree
        .iter()
        .enumerate()
        .filter_map(|(node, &degree)| (degree == 0).then_some(node))
        .collect();
    if roots.as_slice() != [cfg.entry().index()] {
        return None;
    }
    let mut pending = VecDeque::from([cfg.entry().index()]);
    let mut order = Vec::with_capacity(count);
    while let Some(node) = pending.pop_front() {
        order.push(node);
        for successor in cfg.successors(NodeId::new(node as u32)) {
            let degree = &mut indegree[successor.index()];
            *degree = degree.saturating_sub(1);
            if *degree == 0 {
                pending.push_back(successor.index());
            }
        }
    }
    (order.len() == count).then_some(order)
}

fn event_indexes(
    flow: &DataFlow,
    node_count: usize,
) -> (Vec<Vec<usize>>, Vec<BTreeMap<Binding, Vec<u32>>>) {
    let mut uses_by_node = vec![Vec::new(); node_count];
    for (index, use_) in flow.uses.iter().enumerate() {
        if let Some(node) = uses_by_node.get_mut(use_.node as usize) {
            node.push(index);
        }
    }
    let mut definitions_by_node = vec![BTreeMap::new(); node_count];
    for (index, definition) in flow.definitions.iter().enumerate() {
        if definition.kind == DefKind::AddressTaken {
            continue;
        }
        if let Some(node) = definitions_by_node.get_mut(definition.node as usize) {
            node.entry(definition.binding)
                .or_insert_with(Vec::new)
                .push(index as u32);
        }
    }
    (uses_by_node, definitions_by_node)
}

fn reaching_on_node(
    flow: &DataFlow,
    node: usize,
    incoming: &BTreeMap<Binding, Vec<u32>>,
    uses: &[usize],
    definitions: &BTreeMap<Binding, Vec<u32>>,
    reaching: &mut [Vec<u32>],
) {
    for &use_index in uses {
        let use_ = &flow.uses[use_index];
        let local = definitions
            .get(&use_.binding)
            .map(Vec::as_slice)
            .unwrap_or_default();
        let local_latest = local
            .iter()
            .copied()
            .filter(|&index| {
                let definition = &flow.definitions[index as usize];
                definition.node as usize == node
                    && definition.kind != DefKind::MemoryWrite
                    && definition.effect_at <= use_.span.lo
            })
            .max_by_key(|&index| flow.definitions[index as usize].effect_at);
        let values = &mut reaching[use_index];
        if let Some(index) = local_latest {
            values.push(index);
        } else if let Some(live) = incoming.get(&use_.binding) {
            values.extend(live);
        }
        let after = local_latest.map_or(0, |index| flow.definitions[index as usize].effect_at);
        for &index in local {
            let definition = &flow.definitions[index as usize];
            if definition.kind == DefKind::MemoryWrite
                && definition.effect_at >= after
                && definition.effect_at <= use_.span.lo
                && !values.contains(&index)
            {
                values.push(index);
            }
        }
    }
}

fn insert_sorted(values: &mut Vec<u32>, index: u32) {
    if let Err(position) = values.binary_search(&index) {
        values.insert(position, index);
    }
}

fn emit_reaching(flow: &mut DataFlow, reaching: Vec<Vec<u32>>) {
    for (use_index, values) in reaching.into_iter().enumerate() {
        for index in &values {
            flow.edges.push(FlowEdge {
                def: *index,
                use_: use_index as u32,
                name: flow.uses[use_index].name.clone(),
            });
        }
        if values.is_empty() {
            flow.unresolved_uses.push(use_index as u32);
        }
    }
}

/// Reaching edges for a CFG with exactly one path and no cycle.
fn solve_linear(
    flow: &mut DataFlow,
    gen: &[Vec<u32>],
    transfer: &[(Vec<Binding>, Vec<u32>)],
    order: &[usize],
) {
    let mut live: BTreeMap<Binding, Vec<u32>> = BTreeMap::new();
    let mut reaching = vec![Vec::new(); flow.uses.len()];
    let (uses_by_node, definitions_by_node) = event_indexes(flow, gen.len());

    for &node in order {
        reaching_on_node(
            flow,
            node,
            &live,
            &uses_by_node[node],
            &definitions_by_node[node],
            &mut reaching,
        );

        for binding in &transfer[node].0 {
            live.remove(binding);
        }
        for &index in &transfer[node].1 {
            let definition = &flow.definitions[index as usize];
            let values = live.entry(definition.binding).or_default();
            insert_sorted(values, index);
        }
    }

    emit_reaching(flow, reaching);
}

fn solve_acyclic(
    flow: &mut DataFlow,
    gen: &[Vec<u32>],
    transfer: &[(Vec<Binding>, Vec<u32>)],
    order: &[usize],
    cfg: &Cfg,
) -> bool {
    let (uses_by_node, definitions_by_node) = event_indexes(flow, gen.len());
    let mut incoming = vec![BTreeMap::<Binding, Vec<u32>>::new(); gen.len()];
    let mut reaching = vec![Vec::new(); flow.uses.len()];
    let merge_budget = gen.len().saturating_mul(16).max(4_096);
    let mut merge_work = 0usize;
    for &node in order {
        let mut live = std::mem::take(&mut incoming[node]);
        reaching_on_node(
            flow,
            node,
            &live,
            &uses_by_node[node],
            &definitions_by_node[node],
            &mut reaching,
        );
        for binding in &transfer[node].0 {
            live.remove(binding);
        }
        for &index in &transfer[node].1 {
            let definition = &flow.definitions[index as usize];
            insert_sorted(live.entry(definition.binding).or_default(), index);
        }
        for successor in cfg.successors(NodeId::new(node as u32)) {
            let target = &mut incoming[successor.index()];
            for (&binding, values) in &live {
                merge_work = merge_work.saturating_add(values.len());
                if merge_work > merge_budget {
                    return false;
                }
                let merged = target.entry(binding).or_default();
                for &index in values {
                    insert_sorted(merged, index);
                }
            }
        }
    }
    emit_reaching(flow, reaching);
    true
}

/// The reaching-definitions fixpoint, and the edges it implies.
///
/// Sets are explicit `u64` words over definition indices. This keeps the
/// substrate dependency-free while joining 64 definitions per operation on
/// wide recovered functions.
pub(super) fn solve(flow: &mut DataFlow, cfg: &Cfg) {
    // Refinement passes may add definitions and then solve the same flow again.
    // Public result vectors are derived products, not accumulators.
    flow.edges.clear();
    flow.unresolved_uses.clear();
    flow.dead_stores.clear();
    let def_count = flow.definitions.len();
    let node_count = cfg.node_count();
    if def_count == 0 || node_count == 0 {
        flow.unresolved_uses = (0..flow.uses.len() as u32).collect();
        flow.dead_stores = (0..def_count as u32)
            .filter(|index| flow.definitions[*index as usize].kind != DefKind::Parameter)
            .collect();
        return;
    }

    // Public definition indices include address-taking events because callers
    // need to observe escapes. The reaching lattice does not. Keep a compact
    // internal index so pointer-heavy functions do not pay bitset width for
    // events that can never become live values.
    let mut lattice_position = vec![usize::MAX; def_count];
    let mut active_def_count = 0usize;
    for (index, definition) in flow.definitions.iter().enumerate() {
        if definition.kind != DefKind::AddressTaken {
            lattice_position[index] = active_def_count;
            active_def_count += 1;
        }
    }

    // GEN and KILL per node.
    let mut gen: Vec<Vec<u32>> = vec![Vec::new(); node_count];
    for (index, definition) in flow.definitions.iter().enumerate() {
        // Address-taking feeds the points-to model but does not write the
        // addressed value. Keep the event in the public definition table;
        // excluding it here prevents `&x` from becoming a spurious value
        // definition that reaches later reads of `x`.
        if definition.kind == DefKind::AddressTaken {
            continue;
        }
        if let Some(slot) = gen.get_mut(definition.node as usize) {
            slot.push(index as u32);
        }
    }
    for definitions in &mut gen {
        definitions.sort_by_key(|index| flow.definitions[*index as usize].effect_at);
    }
    // Definitions grouped by binding, so KILL is a lookup rather than a scan.
    let mut by_binding: BTreeMap<Binding, Vec<u32>> = BTreeMap::new();
    for (index, definition) in flow.definitions.iter().enumerate() {
        if definition.kind == DefKind::AddressTaken {
            continue;
        }
        by_binding
            .entry(definition.binding)
            .or_default()
            .push(index as u32);
    }

    // OUT only retains the last strong write to each binding on a node, plus
    // weak writes that occur after it. Keep every event in `flow.definitions`
    // for same-node use ordering, but precompute this reduced transfer so a
    // run of assignments does not repeatedly clear the same sibling set.
    let transfer: Vec<(Vec<Binding>, Vec<u32>)> = gen
        .iter()
        .map(|definitions| {
            let mut last_strong: BTreeMap<Binding, usize> = BTreeMap::new();
            for (position, index) in definitions.iter().enumerate() {
                let definition = &flow.definitions[*index as usize];
                if !definition.binding.is_free() && definition.kind != DefKind::MemoryWrite {
                    last_strong.insert(definition.binding, position);
                }
            }
            let killed: Vec<Binding> = last_strong.keys().copied().collect();
            let generated = definitions
                .iter()
                .enumerate()
                .filter_map(|(position, index)| {
                    let definition = &flow.definitions[*index as usize];
                    if definition.binding.is_free() {
                        return Some(*index);
                    }
                    let last = last_strong.get(&definition.binding).copied();
                    if definition.kind == DefKind::MemoryWrite {
                        if last.is_none_or(|strong| position > strong) {
                            return Some(*index);
                        }
                    } else if last == Some(position) {
                        return Some(*index);
                    }
                    None
                })
                .collect();
            (killed, generated)
        })
        .collect();

    if let Some(order) = linear_order(cfg) {
        solve_linear(flow, &gen, &transfer, &order);
    } else if acyclic_order(cfg)
        .is_some_and(|order| solve_acyclic(flow, &gen, &transfer, &order, cfg))
    {
    } else {
        let word_count = active_def_count.div_ceil(WORD_BITS);
        let mut out = vec![vec![0u64; word_count]; node_count];
        let mut in_ = vec![vec![0u64; word_count]; node_count];

        // Iterate to a fixed point. The lattice is a finite powerset and the
        // transfer function is monotone, so this terminates; the bound is there
        // only so a graph the builder left malformed cannot spin.
        let bound = node_count
            .saturating_mul(active_def_count)
            .saturating_add(8);
        for _ in 0..bound {
            let mut changed = false;
            for node in 0..node_count {
                let id = NodeId::new(node as u32);
                let mut incoming = vec![0u64; word_count];
                for predecessor in cfg.predecessors(id) {
                    let source = &out[predecessor.index()];
                    for (slot, value) in incoming.iter_mut().zip(source.iter()) {
                        *slot |= *value;
                    }
                }
                if incoming != in_[node] {
                    in_[node] = incoming.clone();
                    changed = true;
                }

                // OUT = GEN | (IN - KILL).
                let mut next = incoming;
                for binding in &transfer[node].0 {
                    if let Some(siblings) = by_binding.get(binding) {
                        for sibling in siblings {
                            set_bit(&mut next, lattice_position[*sibling as usize], false);
                        }
                    }
                }
                for definition in &transfer[node].1 {
                    set_bit(&mut next, lattice_position[*definition as usize], true);
                }
                if next != out[node] {
                    out[node] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }

        // A use sees exactly one of two things.
        //
        // If any definition of its binding takes effect earlier on the *same*
        // node, the latest such definition is what it sees, and it sees nothing
        // else: a write on the path between two points on one straight-line node
        // kills everything that reached the node. Otherwise it sees whatever the
        // fixpoint says is live at the node's entry.
        //
        // Ordering by `effect_at` rather than by the target's position is what
        // makes `sum = sum + i` read the value that reached the statement.
        for (use_index, use_) in flow.uses.iter().enumerate() {
            let node = use_.node as usize;
            let live = in_.get(node).map(Vec::as_slice).unwrap_or_default();

            let siblings = by_binding
                .get(&use_.binding)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let local_latest = siblings
                .iter()
                .copied()
                .filter(|&index| {
                    let definition = &flow.definitions[index as usize];
                    definition.binding == use_.binding
                        && definition.kind != DefKind::MemoryWrite
                        && definition.node == use_.node
                        && definition.effect_at <= use_.span.lo
                })
                .max_by_key(|&index| flow.definitions[index as usize].effect_at)
                .map(|index| index as usize);

            let mut reaching: Vec<usize> = match local_latest {
                Some(index) => vec![index],
                None => siblings
                    .iter()
                    .copied()
                    .filter(|&index| contains_bit(live, lattice_position[index as usize]))
                    .map(|index| index as usize)
                    .collect(),
            };
            // Weak updates on this node add alternatives after the latest strong
            // update. They never replace the earlier value on their own.
            let after = local_latest.map_or(0, |i| flow.definitions[i].effect_at);
            for &index in siblings {
                let index = index as usize;
                let definition = &flow.definitions[index];
                if definition.binding == use_.binding
                    && definition.node == use_.node
                    && definition.kind == DefKind::MemoryWrite
                    && definition.effect_at >= after
                    && definition.effect_at <= use_.span.lo
                    && !reaching.contains(&index)
                {
                    reaching.push(index);
                }
            }

            for index in &reaching {
                flow.edges.push(FlowEdge {
                    def: *index as u32,
                    use_: use_index as u32,
                    name: use_.name.clone(),
                });
            }
            if reaching.is_empty() {
                flow.unresolved_uses.push(use_index as u32);
            }
        }
    }

    // A definition no edge leaves is a dead store. Parameters are excluded:
    // the caller wrote them and the signature is the contract.
    let mut used_definitions = vec![false; def_count];
    for edge in &flow.edges {
        if let Some(used) = used_definitions.get_mut(edge.def as usize) {
            *used = true;
        }
    }
    let address_taken: std::collections::BTreeSet<Binding> = flow
        .definitions
        .iter()
        .filter(|definition| definition.kind == DefKind::AddressTaken)
        .map(|definition| definition.binding)
        .collect();
    for (index, definition) in flow.definitions.iter().enumerate() {
        if definition.kind == DefKind::Parameter {
            continue;
        }
        // A write to a global, or to anything this function could not resolve,
        // escapes: the read that observes it is in another function, and this
        // analysis is intraprocedural. Calling it dead would flag every
        // constructor's witness variable.
        if definition.binding.is_free() || flow.unresolved_bindings.contains(&definition.binding) {
            continue;
        }
        // Once storage escapes through `&x`, a reader outside this local
        // value graph may observe any earlier store. Do not call it dead merely
        // because address-taking itself is no longer represented as a read.
        if address_taken.contains(&definition.binding) {
            continue;
        }
        // Taking an address is not a store, so it cannot be a dead one.
        if matches!(
            definition.kind,
            DefKind::AddressTaken | DefKind::MemoryWrite
        ) {
            continue;
        }
        if !used_definitions[index] {
            flow.dead_stores.push(index as u32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::csource::{cfg::function_cfgs, parse::parse};

    fn order(source: &str) -> Option<Vec<usize>> {
        let (tree, _) = parse(source).into_parts();
        let (functions, _) = function_cfgs(&tree, source).into_parts();
        linear_order(&functions[0].cfg)
    }

    fn dag_order(source: &str) -> Option<Vec<usize>> {
        let (tree, _) = parse(source).into_parts();
        let (functions, _) = function_cfgs(&tree, source).into_parts();
        acyclic_order(&functions[0].cfg)
    }

    #[test]
    fn the_linear_fast_path_accepts_only_one_complete_acyclic_chain() {
        let straight = order("int f(int x){int y=x;y++;return y;}");
        assert!(straight.is_some_and(|nodes| nodes.len() == 5));
        assert!(order("int f(int x){if(x)x++;return x;}").is_none());
        assert!(order("int f(int x){while(x)x--;return x;}").is_none());
        assert!(order("int f(int x){x?x++:x--;return x;}").is_none());
        assert!(dag_order("int f(int x){if(x)x++;return x;}").is_some());
        assert!(dag_order("int f(int x){x?x++:x--;return x;}").is_some());
        assert!(dag_order("int f(int x){while(x)x--;return x;}").is_none());
    }
}
