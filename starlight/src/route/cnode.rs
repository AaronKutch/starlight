use std::{
    collections::BinaryHeap,
    num::{NonZeroU32, NonZeroU64},
};

use crate::{
    ensemble::PEquiv,
    route::{ChannelWidths, Channeler, PCEdge, PCNode, Programmability, Source},
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
pub struct CNode {
    pub base_p_equiv: Option<PEquiv>,
    pub lvl: u16,
    pub p_supernode: Option<PCNode>,
    pub p_subnodes: Vec<PCNode>,
    pub sink_incident: Option<PCEdge>,
    pub source_incidents: Vec<(PCEdge, usize)>,
    pub internal_behavior: InternalBehavior,
    pub alg_visit: NonZeroU64,
    pub alg_entry_width: usize,
    // this is used in Dijkstras' and points backwards
    pub alg_edge: (Option<PCEdge>, usize),
}

impl CNode {
    pub fn internal_behavior(&self) -> &InternalBehavior {
        &self.internal_behavior
    }
}

impl Channeler {
    /// Given the `subnodes` (which should point to unique `ThisCNode`s) for a
    /// new top level `CNode`, this will manage the backrefs. Note that
    /// `top_level_cnodes` is not set correctly by this.
    pub fn make_cnode(
        &mut self,
        base_p_equiv: Option<PEquiv>,
        mut p_subnodes: Vec<PCNode>,
        lvl: u16,
        internal_behavior: InternalBehavior,
    ) -> PCNode {
        p_subnodes.sort();
        let p_supernode = self.cnodes.insert(CNode {
            base_p_equiv,
            lvl,
            p_supernode: None,
            p_subnodes: vec![],
            sink_incident: None,
            source_incidents: vec![],
            internal_behavior,
            alg_visit: NonZeroU64::new(1).unwrap(),
            alg_entry_width: 0,
            alg_edge: (None, 0),
        });
        for p_subnode in p_subnodes.iter().copied() {
            let cnode = self.cnodes.get_mut(p_subnode).unwrap();
            debug_assert!(cnode.p_supernode.is_none());
            cnode.p_supernode = Some(p_supernode);
        }
        self.cnodes.get_mut(p_supernode).unwrap().p_subnodes = p_subnodes;
        if let Some(base_p_equiv) = base_p_equiv {
            let replaced = self
                .p_back_to_cnode
                .insert(base_p_equiv.into(), p_supernode)
                .1;
            assert!(replaced.is_none());
        }
        p_supernode
    }

    #[must_use]
    pub fn get_supernode(&self, p: PCNode) -> Option<PCNode> {
        self.cnodes.get(p)?.p_supernode
    }

    /// Given two `CNode`s, this will find their lowest level common supernode
    /// (or just return the higher level of the two if one is a supernode of the
    /// other, or return one if they are equal). Can only return `None` if there
    /// are disjoint `CNode` hiearchies. If this function is used in a loop with
    /// a common accumulator, this will find the common supernode of all the
    /// nodes.
    pub fn find_common_supernode(
        &self,
        mut p_cnode0: PCNode,
        mut p_cnode1: PCNode,
    ) -> Option<PCNode> {
        let cnode0 = self.cnodes.get(p_cnode0).unwrap();
        let mut lvl0 = cnode0.lvl;
        let cnode1 = self.cnodes.get(p_cnode1).unwrap();
        let mut lvl1 = cnode1.lvl;
        // first get on same level
        loop {
            // have this run first for all cases
            if p_cnode0 == p_cnode1 {
                // case where one is the supernode of the other
                return Some(p_cnode0)
            }
            if lvl0 < lvl1 {
                p_cnode0 = self.get_supernode(p_cnode0)?;
                lvl0 += 1;
            } else if lvl0 > lvl1 {
                p_cnode1 = self.get_supernode(p_cnode1)?;
                lvl1 += 1;
            } else {
                break
            }
        }
        // find common supernode
        loop {
            p_cnode0 = self.get_supernode(p_cnode0)?;
            p_cnode1 = self.get_supernode(p_cnode1)?;
            if p_cnode0 == p_cnode1 {
                return Some(p_cnode0)
            }
        }
    }
}

/*
see embed.rs for other details

We have a hierarchy for the target, which we could refer to as a synthesis-desynthesis tree or a
summarization tree. The routing starts by embedding program nodes and edges into the root nodes
of the target. The main idea is that for generalized routing it can be difficult to guage where
bulk parts of the program need to be shifted around. The routing starts at a high level that
approximates what parts of the target look like, and proceeds to dilute until it reaches the
base level.

There are different kinds of steps:

(1.) Target Dilution
A program embedding is diluted with respect to the target channeler side, such that an
embedding of a program node into a target cnode is broken into an embedding of a program
node into a subnode of one of the target cnodes. There are hyperpaths in case a value
needs to make its way through `SelectorLut`s to bridge a gap.

(3.) Embedding movement
As dilution proceeds and we get a higher resolution picture of the final embedding, we
will have to transverse move the embeddings to neighboring target cnodes. In fact this is where
the bulk of the channel width constraint violations get resolved and critical paths are minimized,
since it is difficult for the target dilution step to get things correct on the first try.

(4.) Target Concentration
Equivalent to the "rip-up and reroute" process where we find inaccuracies in the
bulk predictions and need to concentrate before retrying dilution.

One of the critical things we do with hyperpaths is allow them to path between concentration
levels and not just on the same level. To initially represent a bit being copied from side
of an FPGA to another, the initial embedding can be a path from the base source target cnode
that goes in the `EdgeKind::Concentrate` direction to a common root and then `EdgeKind::Dilute`
to go to the target sink on the base level (or we detect a disconnection if there isn't a
common root). This allows the Lagrangian routing algorithm to start with completed paths between
program-target mappings, so that we do not constantly have to use maps to look up where we need
to be moving loose endpoints. The Lagrangians could potentially do advanced things by themselves
like promoting concentration or dilution of paths to different cedges when necessary. At the end,
a routing is completed when all embeddings have been diluted to the base level and there
are no violations.

We want the hierarchy to be logarithmic. `generate_hierarchy` is what I found I had to do.
*/

/// Starting from unit `CNode`s and `CEdge`s describing all known low level
/// progam methods, this generates a logarithmic tree of higher level
/// `CNode`s and `CEdge`s that results in a single top level `CNode`s from which
/// routing can start
///
/// We are currently assuming that `generate_hierarchy` is being run once on
/// a graph of unit channel nodes and edges
pub fn generate_hierarchy(channeler: &mut Channeler) -> Result<(), Error> {
    let mut possibly_single_subnode = Vec::<PCNode>::new();
    let mut next_level_cnodes = Vec::<PCNode>::new();
    let mut priority = BinaryHeap::<(usize, PCNode)>::new();

    for (p_cnode, cnode) in &channeler.cnodes {
        if cnode.lvl != 0 {
            return Err(Error::OtherStr(
                "hierarchy appears to have been generated before",
            ))
        }
        priority.push((0, p_cnode));
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
        let cnode = channeler.cnodes.get(p_consider).unwrap();
        if cnode.p_supernode.is_some() {
            // has already been concentrated
            continue
        }

        // For each cnode on a given level, we will attempt to concentrate it and all
        // its neighbors. If any neighbor has a supernode already, it skips the cnode

        let related = channeler.related_nodes(p_consider);
        if related.len() == 1 {
            // the node is disconnected
            continue
        }
        let mut subnodes_in_tree = 0usize;
        let mut lut_bits = 0usize;
        // check if any related nodes have supernodes
        for p_related in related.iter().copied() {
            let related_cnode = channeler.cnodes.get(p_related).unwrap();
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
        let p_next_lvl = channeler.make_cnode(
            None,
            related,
            current_lvl.checked_add(1).unwrap(),
            InternalBehavior {
                subnodes_in_tree,
                lut_bits,
            },
        );
        next_level_cnodes.push(p_next_lvl);
    }

    Ok(())
}

pub fn generate_hierarchy_level(
    current_lvl: u16,
    channeler: &mut Channeler,
    priority: &mut BinaryHeap<(usize, PCNode)>,
    possibly_single_subnode: &mut Vec<PCNode>,
    next_level_cnodes: &mut Vec<PCNode>,
) -> Result<(), Error> {
    // for nodes that couldn't be concentrated, create single subnode supernodes for
    // them, so that edges are only between nodes at the same level
    for p in possibly_single_subnode.drain(..) {
        let cnode = channeler.cnodes.get(p).unwrap();
        if cnode.p_supernode.is_some() {
            continue
        }
        // need to also forward the internal behavior
        let p_next_lvl = channeler.make_cnode(
            None,
            vec![p],
            current_lvl,
            cnode.internal_behavior().clone(),
        );
        next_level_cnodes.push(p_next_lvl);
    }

    // create bulk `CEdge`s between all nodes on the level
    for p_consider in next_level_cnodes.drain(..) {
        // first get the set of subnodes
        let direct_subnode_visit = channeler.next_alg_visit();
        let p_subnodes = channeler.cnodes.get(p_consider).unwrap().p_subnodes.clone();
        for p_subnode in p_subnodes.iter().copied() {
            channeler.cnodes.get_mut(p_subnode).unwrap().alg_visit = direct_subnode_visit;
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
        let related_visit = channeler.next_alg_visit();
        let mut source_set = vec![];
        let mut channel_widths = ChannelWidths::empty();
        let mut lut_bits = 0usize;
        for p_subnode in p_subnodes.iter().copied() {
            // just go over the sink incident to avoid duplication
            if let Some(p_cedge) = channeler.cnodes.get(p_subnode).unwrap().sink_incident {
                let cedge = channeler.cedges.get_mut(p_cedge).unwrap();

                let w = match cedge.programmability() {
                    Programmability::StaticLut(lut) => {
                        lut_bits = lut_bits.checked_add(lut.bw()).unwrap();
                        1
                    }
                    Programmability::ArbitraryLut(arbitrary_lut) => {
                        lut_bits = lut_bits
                            .checked_add(arbitrary_lut.lut_config().len())
                            .unwrap();
                        1
                    }
                    Programmability::SelectorLut(_) => 1,
                    Programmability::Bulk(bulk) => bulk.channel_exit_width,
                };
                channel_widths.channel_exit_width =
                    channel_widths.channel_exit_width.checked_add(w).unwrap();

                for (i, source) in cedge.sources().iter().copied().enumerate() {
                    let cnode = channeler.cnodes.get_mut(source.p_cnode).unwrap();
                    // make sure the `CNode` is outside the direct subnode set
                    if cnode.alg_visit != direct_subnode_visit {
                        // avoid an `OrdArena` by accumulating the entry width on the
                        // related supernode
                        let p_supernode = cnode.p_supernode.unwrap();
                        let supernode = channeler.cnodes.get_mut(p_supernode).unwrap();
                        if supernode.alg_visit != related_visit {
                            supernode.alg_visit = related_visit;
                            supernode.alg_entry_width = 0;
                            // TODO fix the delay here
                            source_set.push(Source {
                                p_cnode: p_supernode,
                                delay_weight: NonZeroU32::new(1).unwrap(),
                            });
                        }
                        let w = match cedge.programmability() {
                            Programmability::StaticLut(_)
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
        // add on the bits from edges with sinks in `p_consider`
        let internal_behavior = &mut channeler
            .cnodes
            .get_mut(p_consider)
            .unwrap()
            .internal_behavior;
        internal_behavior.lut_bits = internal_behavior.lut_bits.checked_add(lut_bits).unwrap();
        // We want the edge source numbers to be mostly tractable. The tree will be
        // lopsided somewhat because of this, but will ultimately be WAVL-like balanced
        // because everything that doesn't have overlap issues will be concentrated
        // every round.
        let channel_exit_width = channel_widths.channel_exit_width;
        priority.push((channel_exit_width, p_consider));
        // create the edge
        if !source_set.is_empty() {
            for source in source_set.iter().copied() {
                let cnode = channeler.cnodes.get(source.p_cnode).unwrap();
                channel_widths
                    .channel_entry_widths
                    .push(cnode.alg_entry_width);
            }
            // TODO the delay weight system is messed up for bulk edges, perhaps this is
            // where we can add more than one edge per concentrated node if the weights vary
            // wildly, e.g. for an island FPGA with some long range connections
            channeler.make_cedge(
                source_set,
                p_consider,
                Programmability::Bulk(channel_widths),
            );
        }
    }
    Ok(())
}
