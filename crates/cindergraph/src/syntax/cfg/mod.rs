//! Language-neutral control-flow graphs and their builder.
//!
//! Control constructs are represented as [`Flow`] events. The builder resolves
//! loops, switches, labels, jumps, and fall-through without depending on a
//! particular token vocabulary or syntax-tree representation.
//!
//! The general CFG contains real successors, join points, loop back edges, and
//! explicit entry and function-end nodes. Comparison-specific expression
//! granularity, contraction, and entry/exit flags live in the parity layer so
//! they do not leak into metrics, dependence analysis, or ordinary graph export.
//!
//! [`Cfg::from_parts`] supports alternate projections, while
//! [`Cfg::chain_partition`], [`Cfg::in_degree`], and [`Cfg::out_degree`] expose
//! the structural information needed by downstream analyses.
//!
//! # Invariants
//!
//! * Recoverable input errors become [`crate::syntax::diag::Diagnostic`] values.
//! * Construction and graph traversals use explicit stacks rather than native recursion.
//! * Dense node identifiers and stable edge ordering make output deterministic.
//! * Every node carries at least one source span.
//!
//! The implementation separates the following concerns:
//!
//! * `flow` --- the [`Flow`] event vocabulary, [`NodeKind`], [`EdgeKind`],
//!   [`CfgNode`] and [`CfgEdge`]: the data every submodule and every future
//!   front end shares.
//! * `mod.rs` (here) --- the [`Cfg`] graph type itself: construction from
//!   parts, adjacency, and the plain accessors.
//! * `build` --- [`CfgBuilder`]: the control-context stack, label
//!   backpatching, and the `Flow` state machine.
//! * `coalesce` --- maximal-chain contraction: [`Cfg::chain_partition`],
//!   [`Cfg::coalesced`] and [`ChainPartition`].
//! * `validate` --- structural invariants: [`Cfg::validate`]
//!   and the reachability walks it is built from.

mod build;
mod coalesce;
mod flow;
mod validate;

pub use build::CfgBuilder;
pub use coalesce::ChainPartition;
pub use flow::{
    CfgEdge, CfgNode, DispatchUncertainty, EdgeKind, Flow, IndirectDispatchInfo, LoopKind,
    NodeKind, TargetPrecision, MAX_NODES,
};

use crate::syntax::ids::NodeId;

/// A control-flow graph: nodes in construction order, edges grouped by source.
///
/// Adjacency is stored in compressed-sparse-row form, built once at
/// construction, so successors, predecessors and both degrees are slice
/// arithmetic rather than a search. That is what a later graph-edit-distance
/// consumer needs (it reads nothing but the degree sequence) and what a lowering
/// consumer needs (it walks successors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cfg {
    nodes: Vec<CfgNode>,
    /// Every edge, grouped by `src` ascending, creation order preserved within
    /// a group.
    edges: Vec<CfgEdge>,
    /// `succ_start[i]..succ_start[i + 1]` indexes `edges` for node `i`.
    succ_start: Vec<u32>,
    /// Predecessor node ids, grouped by destination.
    preds: Vec<NodeId>,
    /// `pred_start[i]..pred_start[i + 1]` indexes `preds` for node `i`.
    pred_start: Vec<u32>,
    entry: NodeId,
    exit: NodeId,
    continue_targets: Vec<NodeId>,
    indirect_dispatches: Vec<IndirectDispatchInfo>,
}

impl Cfg {
    /// Assemble a graph from an explicit node and edge list.
    ///
    /// This is the seam the parity layer sits on: it builds whatever node set
    /// its metric requires and still gets this module's adjacency, degrees,
    /// coalescing and validation for free. Edges naming a node outside `nodes`
    /// are dropped rather than rejected, and `entry`/`exit` are clamped into
    /// range, so no input to this constructor can panic (`REQ-SYN-2`). On an
    /// empty node list the returned ids are placeholders and [`Cfg::node`]
    /// yields `None` for them.
    pub fn from_parts(
        nodes: Vec<CfgNode>,
        edges: Vec<CfgEdge>,
        entry: NodeId,
        exit: NodeId,
    ) -> Self {
        Self::assemble(nodes, edges, entry, exit, Vec::new(), Vec::new())
    }

    fn assemble(
        nodes: Vec<CfgNode>,
        edges: Vec<CfgEdge>,
        entry: NodeId,
        exit: NodeId,
        mut continue_targets: Vec<NodeId>,
        mut indirect_dispatches: Vec<IndirectDispatchInfo>,
    ) -> Self {
        let count = nodes.len();
        let clamp = |id: NodeId| {
            if id.index() < count {
                id
            } else {
                NodeId::new(0)
            }
        };
        let mut edges: Vec<CfgEdge> = edges
            .into_iter()
            .filter(|e| e.src.index() < count && e.dst.index() < count)
            .collect();
        // Stable, so creation order survives within one source: the true arm of
        // a test is emitted before its false arm and stays there (`REQ-SYN-5`).
        edges.sort_by_key(|e| e.src.raw());

        let mut succ_start = vec![0u32; count + 1];
        for e in &edges {
            succ_start[e.src.index() + 1] = succ_start[e.src.index() + 1].saturating_add(1);
        }
        let mut pred_counts = vec![0u32; count + 1];
        for e in &edges {
            pred_counts[e.dst.index() + 1] = pred_counts[e.dst.index() + 1].saturating_add(1);
        }
        for i in 0..count {
            succ_start[i + 1] = succ_start[i + 1].saturating_add(succ_start[i]);
            pred_counts[i + 1] = pred_counts[i + 1].saturating_add(pred_counts[i]);
        }
        let pred_start = pred_counts.clone();
        let mut cursor = pred_counts;
        let mut preds = vec![NodeId::new(0); edges.len()];
        for e in &edges {
            let slot = cursor[e.dst.index()] as usize;
            if let Some(entry_slot) = preds.get_mut(slot) {
                *entry_slot = e.src;
            }
            cursor[e.dst.index()] = cursor[e.dst.index()].saturating_add(1);
        }

        continue_targets.retain(|id| id.index() < count);
        continue_targets.sort_unstable();
        continue_targets.dedup();
        indirect_dispatches.retain(|info| {
            info.node.index() < count
                && nodes
                    .get(info.node.index())
                    .is_some_and(|node| node.kind() == NodeKind::IndirectDispatch)
        });
        for info in &mut indirect_dispatches {
            info.reasons.sort_unstable();
            info.reasons.dedup();
        }
        indirect_dispatches.sort_by_key(|info| info.node);
        indirect_dispatches.dedup_by_key(|info| info.node);

        Self {
            nodes,
            edges,
            succ_start,
            preds,
            pred_start,
            entry: clamp(entry),
            exit: clamp(exit),
            continue_targets,
            indirect_dispatches,
        }
    }

    /// The number of nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// The number of edges, counting parallel edges separately.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// The function entry node.
    pub fn entry(&self) -> NodeId {
        self.entry
    }

    /// The function end node, which `REQ-GEN-1` requires every non-diverging
    /// path to reach.
    pub fn exit(&self) -> NodeId {
        self.exit
    }

    /// The node `id` addresses, or `None` if it addresses no node.
    pub fn node(&self, id: NodeId) -> Option<&CfgNode> {
        self.nodes.get(id.index())
    }

    /// Every node, in construction order --- which is also id order.
    pub fn nodes(&self) -> &[CfgNode] {
        &self.nodes
    }

    /// Every edge, grouped by source and stable within a group.
    pub fn edges(&self) -> &[CfgEdge] {
        &self.edges
    }

    /// The edges leaving `id`, with their kinds. Empty for an unknown id.
    pub fn successor_edges(&self, id: NodeId) -> &[CfgEdge] {
        let Some(&lo) = self.succ_start.get(id.index()) else {
            return &[];
        };
        let Some(&hi) = self.succ_start.get(id.index() + 1) else {
            return &[];
        };
        self.edges.get(lo as usize..hi as usize).unwrap_or(&[])
    }

    /// The nodes control can reach in one step from `id`.
    pub fn successors(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.successor_edges(id).iter().map(|e| e.dst)
    }

    /// The nodes that can reach `id` in one step. Empty for an unknown id.
    pub fn predecessors(&self, id: NodeId) -> &[NodeId] {
        let Some(&lo) = self.pred_start.get(id.index()) else {
            return &[];
        };
        let Some(&hi) = self.pred_start.get(id.index() + 1) else {
            return &[];
        };
        self.preds.get(lo as usize..hi as usize).unwrap_or(&[])
    }

    /// How many edges leave `id`.
    pub fn out_degree(&self, id: NodeId) -> u32 {
        self.successor_edges(id).len() as u32
    }

    /// How many edges arrive at `id`.
    pub fn in_degree(&self, id: NodeId) -> u32 {
        self.predecessors(id).len() as u32
    }

    /// The nodes a `continue` may land on: each loop's test node, or its step
    /// region's head where it has one.
    ///
    /// Recorded because `REQ-GEN-1`'s "`break` and `continue` target the
    /// enclosing construct" is otherwise unverifiable after the fact ---
    /// [`Cfg::validate`] checks every `continue` against this list.
    pub fn continue_targets(&self) -> &[NodeId] {
        &self.continue_targets
    }

    /// Sparse metadata for every first-class indirect dispatch, in node order.
    pub fn indirect_dispatches(&self) -> &[IndirectDispatchInfo] {
        &self.indirect_dispatches
    }

    /// Resolution metadata for `id`, if it is an indirect dispatch node.
    pub fn indirect_dispatch(&self, id: NodeId) -> Option<&IndirectDispatchInfo> {
        self.indirect_dispatches
            .binary_search_by_key(&id, |info| info.node)
            .ok()
            .and_then(|index| self.indirect_dispatches.get(index))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::diag::Diagnostics;
    use crate::syntax::ids::Span;

    fn s(lo: u32) -> Span {
        Span::new(lo, lo + 1)
    }

    fn build(flows: Vec<Flow>) -> (Cfg, Diagnostics) {
        let mut builder = CfgBuilder::new(Span::new(0, 1000));
        builder.extend(flows);
        builder.finish().into_parts()
    }

    #[test]
    fn from_parts_drops_an_edge_naming_a_node_that_does_not_exist() {
        let nodes = vec![CfgNode::single(NodeKind::Entry, s(0))];
        let edges = vec![CfgEdge::new(NodeId::new(0), NodeId::new(9), EdgeKind::Fall)];
        let cfg = Cfg::from_parts(nodes, edges, NodeId::new(0), NodeId::new(4));
        assert_eq!(cfg.edge_count(), 0);
        assert_eq!(cfg.exit(), NodeId::new(0), "an out-of-range id is clamped");
        assert!(
            cfg.indirect_dispatches().is_empty(),
            "a structural projection must not invent source resolution metadata"
        );
    }

    #[test]
    fn an_empty_node_list_yields_an_empty_graph_rather_than_a_panic() {
        let cfg = Cfg::from_parts(Vec::new(), Vec::new(), NodeId::new(3), NodeId::new(4));
        assert_eq!(cfg.node_count(), 0);
        assert!(cfg.node(cfg.entry()).is_none());
        assert!(cfg.reachable().is_empty());
        assert!(cfg.validate().is_empty());
        assert_eq!(cfg.chain_partition().chains().len(), 0);
        assert_eq!(cfg.coalesced().node_count(), 0);
        assert!(cfg.cycle_closing_edges().is_empty());
        assert!(cfg.co_reachable().is_empty());
    }

    #[test]
    fn successors_and_degrees_of_an_unknown_id_are_empty_rather_than_a_panic() {
        let (cfg, _) = build(vec![Flow::Stmt(s(10))]);
        let bogus = NodeId::new(999);
        assert_eq!(cfg.out_degree(bogus), 0);
        assert_eq!(cfg.in_degree(bogus), 0);
        assert!(cfg.successor_edges(bogus).is_empty());
        assert!(cfg.predecessors(bogus).is_empty());
    }
}
