use std::{cmp::max, collections::BinaryHeap, num::NonZeroU64};

use awint::awint_dag::triple_arena::{Advancer, Ptr};

use crate::{
    misc::SmallSet,
    route::{ChannelWidths, Channeler, PEmbedding, Programmability, Referent},
    Error,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InternalBehavior {
    // note for future changes that the hierarchy generating is using this for ordering

    // looking all the way at the bottom of the hierarchy, this counts the total number of subnodes
    pub subnodes_in_tree: usize,

    pub lut_bits: usize,
}

impl InternalBehavior {
    pub fn empty() -> Self {
        Self {
            subnodes_in_tree: 1,
            lut_bits: 0,
        }
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
    pub alg_visit: NonZeroU64,
    pub alg_entry_width: usize,
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
                alg_visit: NonZeroU64::new(1).unwrap(),
                alg_entry_width: 0,
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
    pub fn get_supernode_referent(&self, p: PCNode) -> Option<PCNode> {
        self.cnodes.get_val(p)?.p_supernode
    }

    #[must_use]
    pub fn get_supernode(&self, p: PCNode) -> Option<PCNode> {
        let p_supernode_ref = self.cnodes.get_val(p)?.p_supernode?;
        Some(self.cnodes.get_val(p_supernode_ref)?.p_this_cnode)
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

There are distinct levels with no `CEdge`s between them

This allows the Lagrangian routing algorithm to start with completed paths between program-target
mappings, so that we do not constantly have to use maps to look up where we need to be moving loose
endpoints. The Lagrangians can do advanced things by themselves like promoting concentration or
dilution of paths to different cedges when necessary.
*/

/// Starting from unit `CNode`s and `CEdge`s describing all known low level
/// progam methods, this generates a logarithmic tree of higher level
/// `CNode`s and `CEdge`s that results in a single top level `CNode` from which
/// routing can start
///
/// We are currently assuming that `generate_hierarchy` is being run once on
/// a graph of unit channel nodes and edges
pub fn generate_hierarchy<PCNode: Ptr, PCEdge: Ptr>(
    channeler: &mut Channeler<PCNode, PCEdge>,
) -> Result<(), Error> {
    // when a `CNode` ends up with no edges to anything
    let mut final_top_level_cnodes = Vec::<PCNode>::new();
    let mut possibly_single_subnode = Vec::<PCNode>::new();
    let mut next_level_cnodes = Vec::<PCNode>::new();
    let mut priority = BinaryHeap::<(usize, PCNode)>::new();

    for cnode in channeler.cnodes.vals() {
        if cnode.lvl != 0 {
            return Err(Error::OtherStr(
                "hierarchy appears to have been generated before",
            ))
        }
        priority.push((0, cnode.p_this_cnode));
    }

    let mut current_lvl = 0u16;
    'outer: loop {
        let p_consider = if let Some((_, p_consider)) = priority.pop() {
            p_consider
        } else {
            if next_level_cnodes.is_empty() {
                break
            }
            current_lvl = current_lvl.checked_add(1).unwrap();
            // before going to the next level, need to handle this
            generate_hierarchy_level(
                current_lvl,
                channeler,
                &mut priority,
                &mut possibly_single_subnode,
                &mut next_level_cnodes,
            )?;
            continue;
        };
        let cnode = channeler.cnodes.get_val(p_consider).unwrap();
        if cnode.p_supernode.is_some() {
            // has already been concentrated
            continue
        }

        // For each cnode on a given level, we will attempt to concentrate it and all
        // its neighbors. If any neighbor has a supernode already, it skips the cnode

        let related = channeler.related_nodes(p_consider);
        if related.len() == 1 {
            // the node is disconnected
            final_top_level_cnodes.push(p_consider);
            continue
        }
        let mut subnodes_in_tree = 0usize;
        let mut lut_bits = 0usize;
        // check if any related nodes have supernodes
        for p_related in related.iter().copied() {
            let related_cnode = channeler.cnodes.get_val(p_related).unwrap();
            subnodes_in_tree = subnodes_in_tree
                .checked_add(related_cnode.internal_behavior.subnodes_in_tree)
                .unwrap();
            lut_bits = lut_bits
                .checked_add(related_cnode.internal_behavior.lut_bits)
                .unwrap();
            if related_cnode.p_supernode.is_some() {
                // We can't concentrate `p_consider` because it would concentrate related nodes
                // that are already concentrated, instead put it in `possibly_single_subnode`
                // because it may end up in a solution where it can't concentrate with any other
                // nodes because of overlap.
                possibly_single_subnode.push(p_consider);
                continue 'outer
            }
        }
        // concentrate
        let p_next_lvl = channeler.make_top_level_cnode(
            related.iter().copied(),
            current_lvl.checked_add(1).unwrap(),
            InternalBehavior {
                subnodes_in_tree,
                lut_bits,
            },
        );
        next_level_cnodes.push(p_next_lvl);
    }

    // if there are multiple cnodes are left in an anticlique, concentrate them into
    // a single top level node
    if channeler.top_level_cnodes.len() > 1 {
        let mut set = vec![];
        let mut max_lvl = 0;
        let mut subnodes_in_tree = 0usize;
        let mut lut_bits = 0usize;
        for p_cnode in channeler.top_level_cnodes.keys().copied() {
            set.push(p_cnode);
            let cnode = channeler.cnodes.get_val(p_cnode).unwrap();
            subnodes_in_tree = subnodes_in_tree
                .checked_add(cnode.internal_behavior.subnodes_in_tree)
                .unwrap();
            lut_bits = lut_bits
                .checked_add(cnode.internal_behavior().lut_bits)
                .unwrap();
            max_lvl = max(max_lvl, cnode.lvl);
        }
        channeler.make_top_level_cnode(set, max_lvl.checked_add(1).unwrap(), InternalBehavior {
            subnodes_in_tree,
            lut_bits,
        });
    }
    Ok(())
}

pub fn generate_hierarchy_level<PCNode: Ptr, PCEdge: Ptr>(
    current_lvl: u16,
    channeler: &mut Channeler<PCNode, PCEdge>,
    priority: &mut BinaryHeap<(usize, PCNode)>,
    possibly_single_subnode: &mut Vec<PCNode>,
    next_level_cnodes: &mut Vec<PCNode>,
) -> Result<(), Error> {
    // for nodes that couldn't be concentrated, create single subnode supernodes for
    // them, so that edges are only between nodes at the same level
    for p in possibly_single_subnode.drain(..) {
        let cnode = channeler.cnodes.get_val(p).unwrap();
        if cnode.p_supernode.is_some() {
            continue
        }
        // need to also forward the internal behavior
        let p_next_lvl =
            channeler.make_top_level_cnode([p], current_lvl, cnode.internal_behavior().clone());
        next_level_cnodes.push(p_next_lvl);
    }

    // create bulk `CEdge`s between all nodes on the level
    for p_consider in next_level_cnodes.drain(..) {
        // first get the set of subnodes
        channeler.alg_visit = channeler.alg_visit.checked_add(1).unwrap();
        let direct_subnode_visit = channeler.alg_visit;
        let mut subnode_set = vec![];
        let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
        while let Some(p_subnode) = subnode_adv.advance(channeler) {
            channeler.cnodes.get_val_mut(p_subnode).unwrap().alg_visit = direct_subnode_visit;
            subnode_set.push(p_subnode);
        }
        // The current plan is that we just create one big edge that has its sink
        // incident in `p_consider`, with source incidents to all supernodes of subnodes
        // outside of the subnode set that have source incidents to an edge that has a
        // sink in the subnodes of `p_consider`. I'm not sure if we should discretize
        // this more since the channel source widths are tracked separately to begin
        // with. However I suspect that this is the correct approach because we can
        // simplify bulk edge behavior to only track channel widths and is the only
        // straightforward way to avoid `OrdArena`s.

        // iterate through the subnodes again, but now get a set of second neighbors
        // that aren't in the subnodes set
        channeler.alg_visit = channeler.alg_visit.checked_add(1).unwrap();
        let related_visit = channeler.alg_visit;
        let mut source_set = vec![];
        let mut channel_widths = ChannelWidths::empty();
        let mut lut_bits = 0usize;
        for p_subnode in subnode_set.iter().copied() {
            // go over all neighbors through the edges
            let mut adv = channeler.cnodes.advancer_surject(p_subnode);
            while let Some(p_referent) = adv.advance(&channeler.cnodes) {
                if let Referent::CEdgeIncidence(p_cedge, i) =
                    *channeler.cnodes.get_key(p_referent).unwrap()
                {
                    // avoid duplication, if this is a sink incidence we automatically have
                    // a one time iter of the edge we need to handle
                    if i.is_none() {
                        let cedge = channeler.cedges.get_mut(p_cedge).unwrap();

                        let w = match cedge.programmability() {
                            Programmability::TNode => 1,
                            Programmability::StaticLut(lut) => {
                                lut_bits = lut_bits.checked_add(lut.bw()).unwrap();
                                1
                            }
                            Programmability::ArbitraryLut(lut) => {
                                lut_bits = lut_bits.checked_add(lut.len()).unwrap();
                                1
                            }
                            Programmability::SelectorLut(_) => 1,
                            Programmability::Bulk(bulk) => bulk.channel_exit_width,
                        };
                        channel_widths.channel_exit_width =
                            channel_widths.channel_exit_width.checked_add(w).unwrap();

                        for (i, p_incident) in cedge.sources().iter().copied().enumerate() {
                            let cnode = channeler.cnodes.get_val_mut(p_incident).unwrap();
                            // make sure the `CNode` is outside the direct subnode set
                            if cnode.alg_visit != direct_subnode_visit {
                                // avoid an `OrdArena` by accumulating the entry width on the
                                // related supernode
                                let p_supernode = cnode.p_supernode.unwrap();
                                let supernode = channeler.cnodes.get_val_mut(p_supernode).unwrap();
                                if supernode.alg_visit != related_visit {
                                    supernode.alg_visit = related_visit;
                                    supernode.alg_entry_width = 0;
                                    source_set.push(supernode.p_this_cnode);
                                }
                                let w = match cedge.programmability() {
                                    Programmability::TNode
                                    | Programmability::StaticLut(_)
                                    | Programmability::ArbitraryLut(_)
                                    | Programmability::SelectorLut(_) => 1,
                                    Programmability::Bulk(bulk) => bulk.channel_entry_widths[i],
                                };
                                supernode.alg_entry_width =
                                    supernode.alg_entry_width.checked_add(w).unwrap();
                            }
                            // else the connections are internal, TODO are there
                            // any internal connection statistics we should want
                            // to track?
                        }
                    }
                }
            }
        }
        // add on the bits from edges with sinks in `p_consider`
        let internal_behavior = &mut channeler
            .cnodes
            .get_val_mut(p_consider)
            .unwrap()
            .internal_behavior;
        internal_behavior.lut_bits = internal_behavior.lut_bits.checked_add(lut_bits).unwrap();
        // keeps the tree both relatively balanced and edge sizes tractable
        priority.push((channel_widths.channel_exit_width, p_consider));
        // create the edge
        if !source_set.is_empty() {
            for source in source_set.iter().cloned() {
                let cnode = channeler.cnodes.get_val(source).unwrap();
                channel_widths
                    .channel_entry_widths
                    .push(cnode.alg_entry_width);
            }
            channeler.make_cedge(
                &source_set,
                p_consider,
                Programmability::Bulk(channel_widths),
            );
        }
    }
    Ok(())
}
