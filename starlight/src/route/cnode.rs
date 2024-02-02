use std::cmp::max;

use awint::awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena, Ptr};

use crate::{
    misc::SmallSet,
    route::{channel::Referent, BulkBehavior, Channeler, PEmbedding, Programmability},
};

#[derive(Debug, Clone)]
pub struct InternalBehavior {
    pub lut_bits: usize,
}

impl InternalBehavior {
    pub fn empty() -> Self {
        Self { lut_bits: 0 }
    }
}

/// A channel node
#[derive(Debug, Clone)]
pub struct CNode<PCNode: Ptr> {
    pub p_this_cnode: PCNode,
    pub lvl: u16,
    pub p_supernode: Option<PCNode>,
    pub internal_behavior: InternalBehavior,
    pub embeddings: SmallSet<PEmbedding>,
}

impl<PCNode: Ptr> CNode<PCNode> {
    pub fn internal_behavior(&self) -> &InternalBehavior {
        &self.internal_behavior
    }
}

impl<PCNode: Ptr, PCEdge: Ptr> Channeler<PCNode, PCEdge> {
    /// Given the `subnodes` (which should point to unique `ThisCNode`s) for a
    /// new top level `CNode`, this will manage the backrefs
    pub fn make_top_level_cnode<I>(
        &mut self,
        subnodes: I,
        lvl: u16,
        internal_behavior: InternalBehavior,
    ) -> PCNode
    where
        I: IntoIterator<Item = PCNode>,
    {
        let p_cnode = self.cnodes.insert_with(|p_this_cnode| {
            (Referent::ThisCNode, CNode {
                p_this_cnode,
                lvl,
                p_supernode: None,
                internal_behavior,
                embeddings: SmallSet::new(),
            })
        });
        for p_subnode in subnodes {
            if let Some(p) = self.top_level_cnodes.find_key(&p_subnode) {
                self.top_level_cnodes.remove(p).unwrap();
            }
            let p_supernode = self
                .cnodes
                .insert_key(p_cnode, Referent::SubNode(p_subnode))
                .unwrap();
            let cnode = self.cnodes.get_val_mut(p_subnode).unwrap();
            cnode.p_supernode = Some(p_supernode);
        }
        let _ = self.top_level_cnodes.insert(p_cnode, ());
        p_cnode
    }

    #[must_use]
    pub fn get_supernode(&self, p: PCNode) -> Option<PCNode> {
        self.cnodes.get_val(p)?.p_supernode
    }

    /// Given two `CNode`s, this will find their lowest level common supernode
    /// (or just return the higher level of the two if one is a supernode of the
    /// other, or return one if they are equal). Can only return `None` if there
    /// are disjoint `CNode` hiearchies. If this function is used in a loop with
    /// a common accumulator, this will find the common supernode of all the
    /// nodes.
    pub fn find_common_supernode(&self, p_back0: PCNode, p_back1: PCNode) -> Option<PCNode> {
        let cnode0 = self.cnodes.get_val(p_back0).unwrap();
        let mut lvl0 = cnode0.lvl;
        let mut p_cnode0 = cnode0.p_this_cnode;
        let cnode1 = self.cnodes.get_val(p_back1).unwrap();
        let mut lvl1 = cnode1.lvl;
        let mut p_cnode1 = cnode1.p_this_cnode;
        // first get on same level
        loop {
            // have this run first for all cases
            if p_cnode0 == p_cnode1 {
                // case where one is the supernode of the other
                return Some(p_cnode0)
            }
            if lvl0 < lvl1 {
                if let Some(p_super_cnode) = self.get_supernode(p_cnode0) {
                    p_cnode0 = p_super_cnode;
                } else {
                    return None
                }
                lvl0 += 1;
            } else if lvl0 > lvl1 {
                if let Some(p_super_cnode) = self.get_supernode(p_cnode1) {
                    p_cnode1 = p_super_cnode;
                } else {
                    return None
                }
                lvl1 += 1;
            } else {
                break
            }
        }
        // find common supernode
        loop {
            if let Some(p_super_cnode) = self.get_supernode(p_cnode0) {
                p_cnode0 = p_super_cnode;
            } else {
                return None
            }
            if let Some(p_super_cnode) = self.get_supernode(p_cnode1) {
                p_cnode1 = p_super_cnode;
            } else {
                return None
            }
            if p_cnode0 == p_cnode1 {
                return Some(p_cnode0)
            }
        }
    }
}

/*
here are the current ideas on the channeling hierarchy

We know we want a hierarchy for the target and a hierarchy for the program.
The routing starts by having an embedding of the single top level program cnode
into the single top level target cnode (modulo handling how we see fit for if
the target and/or program has disconnections). There are different kinds of steps:

(1.) Program dilution
In one kind of step, a program's embeddings
are "diluted" (as opposed to concentrating when looking from the bottom to the top
of the hierarchy) with a embedding of one program cnode into a target cnode being
broken into an embedding of that program cnode's subnodes into the same target cnode.

(2.) Target dilution
A program embedding is diluted with respect to the target channeler side, such that an
embedding of a program cnode into a target cnode is broken into an embedding of a program
cnode into a subnode of one of the target cnodes.
There is one step of embedding movement implicit in this, where we choose which
subnode to embed.

(3.) Embedding movement
As dilution proceeds and we get a higher resolution picture of the final embedding, we
will have to transverse move the embeddings to neighboring target cnodes

(4.) Concentration
Equivalent to the "rip-up and reroute" process where we find inaccuracies in the
bulk predictions and need to concentrate before retrying dilution.

The routing process progresses from the initial single top level embedding by first
diluting the program, and then diluting both while maintaining a more dilution
for the program than the target. There are usually multiple program cnodes embedded
into a single target cnode, until the lowest level.

The questions are: Do we have distinct mandatory levels? Do we have special CEdges
between the levels? Do we have intersections in the subnode sets of cnodes?

Currently, it seems that we do not want intersections in the subnode cnode and/or cedge sets,
because the concentrated cedges will have redundancies and probably confuse the channel
routing capacities. We treat the supernode and subnode backrefs as zero cost edges
that the hyperpaths can use during intermediate routing. We keep the levels
distinct and do not let there be super/sub node backrefs within the same level, and
we keep an integer for the level with the `CNode`. We try to store all other
information in the `CEdge`s, and alternate exclusive paths are encoded into the
topology instead.

This allows the Lagrangian routing algorithm to start with completed paths between program-target
mappings, so that we do not constantly have to use maps to look up where we need to be moving loose
endpoints. The Lagrangians can do advanced things by themselves like promoting concentration or
dilution of paths to different cedges when necessary.

For one more detail, what do we do about disconnected graphs?

*/

/// Starting from unit `CNode`s and `CEdge`s describing all known low level
/// progam methods, this generates a logarithmic tree of higher level
/// `CNode`s and `CEdge`s that results in a single top level `CNode` from which
/// routing can start
///
/// We are currently assuming that `generate_hierarchy` is being run once on
/// a graph of unit channel nodes and edges
pub fn generate_hierarchy<PCNode: Ptr, PCEdge: Ptr>(channeler: &mut Channeler<PCNode, PCEdge>) {
    // For each cnode on a given level, we will attempt to concentrate it and all
    // its neighbors. If any neighbor has a supernode already, it skips the cnode

    let mut current_lvl = 0u16;
    // TODO this is somewhat inefficient, may want to keep an array of the previous
    // and next level `PCNode`s around
    loop {
        let next_lvl = current_lvl.checked_add(1).unwrap();
        let mut concentrated = false;
        let mut adv = channeler.cnodes.advancer();
        'over_cnodes: while let Some(p_consider) = adv.advance(&channeler.cnodes) {
            if let Referent::ThisCNode = channeler.cnodes.get_key(p_consider).unwrap() {
                let cnode = channeler.cnodes.get_val(p_consider).unwrap();
                if (cnode.lvl != current_lvl) || cnode.p_supernode.is_some() {
                    continue
                }
                let related = channeler.related_nodes(p_consider);
                if related.len() == 1 {
                    // the node is disconnected
                    continue
                }
                // check if any related nodes have supernodes
                for p_related in related.keys() {
                    if channeler
                        .cnodes
                        .get_val(*p_related)
                        .unwrap()
                        .p_supernode
                        .is_some()
                    {
                        continue 'over_cnodes;
                    }
                }
                // add up internal bits
                let mut lut_bits = 0usize;
                for p in related.keys() {
                    lut_bits = lut_bits
                        .checked_add(
                            channeler
                                .cnodes
                                .get_val(*p)
                                .unwrap()
                                .internal_behavior()
                                .lut_bits,
                        )
                        .unwrap();
                }
                // concentrate
                channeler.make_top_level_cnode(
                    related.keys().copied(),
                    next_lvl,
                    InternalBehavior { lut_bits },
                );

                concentrated = true;
            }
        }
        if !concentrated {
            // there are only disconnected nodes left
            break
        }
        // for nodes that couldn't be concentrated, create single subnode supernodes for
        // them, so that edges are only between nodes at the same level
        let mut adv = channeler.cnodes.advancer();
        while let Some(p_consider) = adv.advance(&channeler.cnodes) {
            if let Referent::ThisCNode = channeler.cnodes.get_key(p_consider).unwrap() {
                let cnode = channeler.cnodes.get_val(p_consider).unwrap();
                if (cnode.lvl != current_lvl) || cnode.p_supernode.is_some() {
                    continue
                }
                // need to also forward the internal behavior
                channeler.make_top_level_cnode(
                    [p_consider],
                    next_lvl,
                    cnode.internal_behavior().clone(),
                );
            }
        }

        // we have all the next level nodes, but we need to create the bulk `CEdge`s
        // between them
        let mut adv = channeler.cnodes.advancer();
        while let Some(p_consider) = adv.advance(&channeler.cnodes) {
            if let Referent::ThisCNode = channeler.cnodes.get_key(p_consider).unwrap() {
                let cnode = channeler.cnodes.get_val(p_consider).unwrap();
                if cnode.lvl != next_lvl {
                    continue
                }
                // TODO in the referents refactor, we need some formulaic way to add extra data
                // to the surject value structs to avoid all these `OrdArena`s
                ptr_struct!(P0; P1; P2; P3);
                // first get the set of subnodes
                let mut subnode_set = OrdArena::<P0, PCNode, ()>::new();
                let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
                while let Some(p_subnode) = subnode_adv.advance(channeler) {
                    let _ = subnode_set.insert(p_subnode, ());
                }
                // iterate through the subnodes again, but now get a set of the neighbors that
                // aren't in the subnodes set
                let mut related_subnodes_set = OrdArena::<P1, PCNode, ()>::new();
                let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
                while let Some(p_subnode) = subnode_adv.advance(channeler) {
                    for p_related in channeler.related_nodes(p_subnode).keys() {
                        if subnode_set.find_key(p_related).is_none() {
                            let _ = related_subnodes_set.insert(*p_related, ());
                        }
                    }
                }
                // get all the supernodes of the related subnodes, and associate them with
                // bulk behavior for the `CEdge` with them later. This bulk behavior will be the
                // edge from this `CNode` under consideration to the related node (so only sink
                // incidents will contribute to the bulk behavior), and when the related cnode
                // is under consideration it will handle the edge in the other direction, so we
                // can avoid duplication.
                let mut related_supernodes_set = OrdArena::<P2, PCNode, BulkBehavior>::new();
                for p_related_subnode in related_subnodes_set.keys() {
                    let p_related_supernode = channeler.get_supernode(*p_related_subnode).unwrap();
                    let _ =
                        related_supernodes_set.insert(p_related_supernode, BulkBehavior::empty());
                }
                // we want to find hyperedges with incidents that are both in the subnodes and
                // related subnodes, which will be concentrated as a bulk edge between the
                // supernode under consideration and the related supernodes. To avoid
                // duplication we will orient around the sink incident, and only do work in this
                // iteration if the sink is in our subnodes set. If the sink is in a related
                // subnode, another 'over_cnodes iteration will handle it. This is also one of
                // the reasons why we made each node only able to have one supernode.

                // If all the incidents are in our subnodes set, then the edge is internal and
                // we would do nothing except that we need to add onto the `InternalBehavior` so
                // that the routing knows where LUT bits are.

                // If some source incidents are in our subnodes set, we need to make sure that
                // they do not contribute to the concentrated edges.

                // Source incidents from the same edge can be in multiple other related sets, in
                // which case the bulk behavior edge can be a hyperedge.

                // Multiple source incidents can be in the same related set

                // TODO we allow combinations of edges and various hyperedges to coexist, are
                // there any exponential blowup cases that can happen despite the
                // internalization?

                let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
                while let Some(p_subnode) = subnode_adv.advance(channeler) {
                    let mut adv_edges = channeler.cnodes.advancer_surject(p_subnode);
                    while let Some(p_referent) = adv_edges.advance(&channeler.cnodes) {
                        if let Referent::CEdgeIncidence(p_cedge, i) =
                            channeler.cnodes.get_key(p_referent).unwrap()
                        {
                            // avoid duplication, if this is a sink incidence we automatically have
                            // a one time iter of the edge we need to handle
                            if i.is_none() {
                                let cedge = channeler.cedges.get(*p_cedge).unwrap();
                                // this is an `OrdArena` to handle the multiple incidents from the
                                // same set redundancy
                                let mut bulk_info = OrdArena::<P3, PCNode, usize>::new();
                                for (i, p_source) in cedge.sources().iter().enumerate() {
                                    let cnode = channeler.cnodes.get_val(*p_source).unwrap();
                                    // TODO if we commit to having a single supernode, have the info
                                    // in the `CNode` value and not in a referent.

                                    // if cnode.supernode.unwrap() == ...

                                    if subnode_set.find_key(&cnode.p_this_cnode).is_none() {
                                        // we have a source incident in the related set
                                        let p = related_subnodes_set
                                            .find_key(&cnode.p_this_cnode)
                                            .unwrap();
                                        let p_related_subnode =
                                            *related_subnodes_set.get_key(p).unwrap();
                                        let w = match cedge.programmability() {
                                            Programmability::TNode
                                            | Programmability::StaticLut(_)
                                            | Programmability::ArbitraryLut(_)
                                            | Programmability::SelectorLut(_) => 1,
                                            Programmability::Bulk(bulk) => {
                                                bulk.channel_entry_widths[i]
                                            }
                                        };
                                        let p_related_supernode =
                                            channeler.get_supernode(p_related_subnode).unwrap();
                                        // TODO `OrdArena` needs a function for the common update or
                                        // insert new pattern, use find_similar internally instead
                                        // of a potentially expensive replace
                                        let (p, replaced) =
                                            bulk_info.insert(p_related_supernode, w);
                                        if let Some((_, w_replaced)) = replaced {
                                            *bulk_info.get_val_mut(p).unwrap() =
                                                w.checked_add(w_replaced).unwrap();
                                        }
                                    }
                                }
                                if bulk_info.is_empty() {
                                    // the edge is internal, need to add to the internal LUT bit
                                    // count
                                    let internal_behavior = &mut channeler
                                        .cnodes
                                        .get_val_mut(p_consider)
                                        .unwrap()
                                        .internal_behavior;
                                    let lut_bits = match cedge.programmability() {
                                        Programmability::TNode => 0,
                                        Programmability::StaticLut(lut) => lut.bw(),
                                        Programmability::ArbitraryLut(lut) => lut.len(),
                                        Programmability::SelectorLut(_) => 0,
                                        Programmability::Bulk(bulk_behavior) => {
                                            bulk_behavior.lut_bits
                                        }
                                    };
                                    internal_behavior.lut_bits =
                                        internal_behavior.lut_bits.checked_add(lut_bits).unwrap();
                                } else {
                                    let mut sources = vec![];
                                    let mut channel_entry_widths = vec![];
                                    for (_, source, width) in bulk_info {
                                        sources.push(source);
                                        channel_entry_widths.push(width);
                                    }
                                    let (channel_exit_width, lut_bits) =
                                        match cedge.programmability() {
                                            Programmability::TNode => (1, 0),
                                            Programmability::StaticLut(lut) => (1, lut.bw()),
                                            Programmability::ArbitraryLut(lut) => (1, lut.len()),
                                            Programmability::SelectorLut(_) => (1, 0),
                                            Programmability::Bulk(bulk_behavior) => (
                                                bulk_behavior.channel_exit_width,
                                                bulk_behavior.lut_bits,
                                            ),
                                        };
                                    channeler.make_cedge(
                                        &sources,
                                        p_consider,
                                        Programmability::Bulk(BulkBehavior {
                                            channel_entry_widths,
                                            channel_exit_width,
                                            lut_bits,
                                        }),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        current_lvl = next_lvl;
    }

    // if there are multiple cnodes are left in an anticlique, concentrate them into
    // a single top level node
    if channeler.top_level_cnodes.len() > 1 {
        let mut set = vec![];
        let mut max_lvl = 0;
        let mut lut_bits = 0usize;
        for p_cnode in channeler.top_level_cnodes.keys() {
            set.push(*p_cnode);
            let cnode = channeler.cnodes.get_val(*p_cnode).unwrap();
            max_lvl = max(max_lvl, cnode.lvl);
            lut_bits = lut_bits
                .checked_add(cnode.internal_behavior().lut_bits)
                .unwrap();
        }
        channeler.make_top_level_cnode(set, max_lvl, InternalBehavior { lut_bits });
    }
}
