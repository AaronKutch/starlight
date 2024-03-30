use std::{collections::BinaryHeap, num::NonZeroUsize};

use super::{BulkProperties, Programmability};
use crate::{
    route::{Channeler, PCNode},
    Error,
};

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
pub(crate) fn generate_hierarchy(channeler: &mut Channeler) -> Result<(), Error> {
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
            // the node on the current level is by itself, do not concentrate it as it will
            // be the root node of its connected region of the target
            continue
        }
        // check if any related nodes have supernodes
        for p_related in related.iter().copied() {
            let related_cnode = channeler.cnodes.get(p_related).unwrap();
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
            related,
            current_lvl.checked_add(1).unwrap(),
            Programmability::Bulk(BulkProperties::new()),
        );
        next_level_cnodes.push(p_next_lvl);
    }

    Ok(())
}

fn generate_hierarchy_level(
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
            // it was concentrated into something else
            continue
        }
        // need to also forward the internal behavior
        let p_next_lvl = channeler.make_cnode(vec![p], current_lvl, cnode.programmability.clone());
        next_level_cnodes.push(p_next_lvl);
    }

    // create bulk edges between all nodes on the level
    for p_consider in next_level_cnodes.drain(..) {
        // first get the set of direct subnodes
        let direct_subnode_visit = channeler.next_alg_visit();
        let p_subnodes = channeler.cnodes.get(p_consider).unwrap().p_subnodes.clone();
        for p_subnode in p_subnodes.iter().copied() {
            channeler.cnodes.get_mut(p_subnode).unwrap().alg_visit = direct_subnode_visit;
        }

        // iterate through the subnodes again, but now get a set of second neighbors
        // that aren't in the subnodes set
        let second_related_visit = channeler.next_alg_visit();
        let mut second_related_nodes = vec![];
        for p_subnode in p_subnodes.iter().copied() {
            let subnode = channeler.cnodes.get(p_subnode).unwrap();
            // we avoid double counting by only handling things in the sink direction,
            // sources of this subnode that are skipped over will be handled in another
            // iteration
            let sinks = subnode.sinks.clone();
            for sink in sinks {
                let other = channeler.cnodes.get_mut(sink.p_cnode).unwrap();
                // make sure the `CNode` is outside the direct subnode set, and thus its
                // supernode is not the same node as the direct subnode's supernode and the
                // current level needs an edge between the supernodes
                if other.alg_visit != direct_subnode_visit {
                    let p_supernode = other.p_supernode.unwrap();
                    let supernode = channeler.cnodes.get_mut(p_supernode).unwrap();
                    // avoid an `OrdArena` by accumulating the width on the
                    // related supernode
                    if supernode.alg_visit != second_related_visit {
                        // prep if the supernode has not been seen in the current outer loop before
                        supernode.alg_visit = second_related_visit;
                        supernode.alg_usize0 = 0;
                        second_related_nodes.push(p_supernode);
                    }
                    supernode.alg_usize0 =
                        supernode.alg_usize0.checked_add(sink.width.get()).unwrap();
                }
            }
        }
        let len = second_related_nodes.len();
        for p_second in second_related_nodes {
            let second = channeler.cnodes.get(p_second).unwrap();
            let width = NonZeroUsize::new(second.alg_usize0).unwrap();
            channeler.make_cedge(p_consider, p_second, width);
            // TODO the delay weight system is messed up for bulk edges, perhaps
            // this is perhaps where we should add more than one
            // edge per concentrated node if the weights vary
            // wildly, e.g. for an island FPGA with some long range connections

            // set weight here TODO
        }
        // We want the edge source numbers to be mostly tractable. The tree will be
        // lopsided somewhat because of this, but will ultimately be WAVL-like balanced
        // because everything that doesn't have overlap issues will be concentrated
        // every round.
        priority.push((len, p_consider));
    }
    Ok(())
}
