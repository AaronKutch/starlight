use std::cmp::max;

use awint::awint_dag::{
    smallvec::SmallVec,
    triple_arena::{ptr_struct, Advancer, ArenaTrait, OrdArena, Ptr},
};

use super::{
    cedge::{self, PUniqueCNode},
    BulkBehavior, Programmability,
};
use crate::route::{channel::Referent, Channeler, PBack};

/// A channel node
#[derive(Debug, Clone, Default)]
pub struct CNode {
    pub p_this_cnode: PBack,
    pub lvl: u16,
    pub has_supernode: bool,
}

impl Channeler {
    /// Given the `subnodes` (which should point to unique `ThisCNode`s) for a
    /// new top level `CNode`, this will manage the backrefs
    pub fn make_top_level_cnode<I>(&mut self, subnodes: I, lvl: u16) -> PBack
    where
        I: IntoIterator<Item = PBack>,
    {
        let p_cnode = self.cnodes.insert_with(|p_this_cnode| {
            (Referent::ThisCNode, CNode {
                p_this_cnode,
                lvl,
                has_supernode: false,
            })
        });
        for subnode in subnodes {
            if let Some(p) = self.top_level_cnodes.find_key(&subnode) {
                self.top_level_cnodes.remove(p).unwrap();
            }
            let p_subnode = self
                .cnodes
                .insert_key(subnode, Referent::SuperNode(Ptr::invalid()))
                .unwrap();
            let p_supernode = self
                .cnodes
                .insert_key(p_cnode, Referent::SubNode(p_subnode))
                .unwrap();
            // we want the referents to point exactly at each other's keys and not the
            // `p_this_cnode`
            let (referent, cnode) = self.cnodes.get_mut(p_subnode).unwrap();
            *referent = Referent::SuperNode(p_supernode);
            cnode.has_supernode = true;
        }
        self.top_level_cnodes.insert(p_cnode, ());
        p_cnode
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
pub fn generate_hierarchy(channeler: &mut Channeler) {
    // For each cnode on a given level, we will attempt to concentrate it and all
    // its neighbors. If any neighbor has a supernode already, it skips the cnode

    let mut current_lvl = 0u16;
    // TODO this is somewhat inefficient, may want to keep an array of the previous
    // and next level `PBack`s around
    loop {
        let next_lvl = current_lvl.checked_add(1).unwrap();
        let mut concentrated = false;
        let mut adv = channeler.cnodes.advancer();
        'over_cnodes: while let Some(p_consider) = adv.advance(&channeler.cnodes) {
            if let Referent::ThisCNode = channeler.cnodes.get_key(p_consider).unwrap() {
                let cnode = channeler.cnodes.get_val(p_consider).unwrap();
                if (cnode.lvl != current_lvl) || cnode.has_supernode {
                    continue
                }
                // check if the node's neighbors have supernodes
                let mut neighbor_adv = channeler.advancer_neighbors_of_node(p_consider);
                while let Some(p) = neighbor_adv.advance(&channeler) {
                    if channeler.cnodes.get_val(p).unwrap().has_supernode {
                        continue 'over_cnodes;
                    }
                }
                // concentrate
                let neighbors = neighbor_adv.into_unique();
                channeler.make_top_level_cnode(neighbors.keys().map(|p| *p), next_lvl);

                concentrated = true;
            }
        }
        if !concentrated {
            // there are only disconnected nodes left
            break
        }
        // for nodes that couldn't be concentrated, create single subnode supernodes for
        // them
        let mut adv = channeler.cnodes.advancer();
        'over_cnodes: while let Some(p_consider) = adv.advance(&channeler.cnodes) {
            if let Referent::ThisCNode = channeler.cnodes.get_key(p_consider).unwrap() {
                let cnode = channeler.cnodes.get_val(p_consider).unwrap();
                if (cnode.lvl != current_lvl) || cnode.has_supernode {
                    continue
                }
                channeler.make_top_level_cnode([p_consider], next_lvl);
            }
        }

        // we have all the next level nodes, but we need to create the bulk `CEdge`s
        // between them
        let mut adv = channeler.cnodes.advancer();
        'over_cnodes: while let Some(p_consider) = adv.advance(&channeler.cnodes) {
            if let Referent::ThisCNode = channeler.cnodes.get_key(p_consider).unwrap() {
                let cnode = channeler.cnodes.get_val(p_consider).unwrap();
                if cnode.lvl != next_lvl {
                    continue
                }
                // TODO in the referents refactor, we need some formulaic way to add extra data
                // to the surject value structs to avoid all these `OrdArena`s
                ptr_struct!(P0; P1; P2);
                // first get the set of subnodes
                let mut subnode_set = OrdArena::<P0, PBack, ()>::new();
                let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
                while let Some(p_subnode) = subnode_adv.advance(&channeler) {
                    let _ = subnode_set.insert(p_subnode, ());
                }
                // iterate through the subnodes again, but now get a set of the neighbors that
                // aren't in the subnodes set
                let mut related_subnodes_set = OrdArena::<P1, PBack, ()>::new();
                let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
                while let Some(p_subnode) = subnode_adv.advance(&channeler) {
                    let mut second_neighbors = channeler.advancer_neighbors_of_node(p_subnode);
                    while let Some(p_neighbor) = second_neighbors.advance(&channeler) {
                        if subnode_set.find_key(&p_neighbor).is_none() {
                            related_subnodes_set.insert(p_neighbor, ());
                        }
                    }
                }
                // get all the supernodes of the related subnodes, and associate them with
                // bulk behavior for the `CEdge` with them later. This bulk behavior will be the
                // edge from this `CNode` under consideration to the related node (so only sink
                // incidents will contribute to the bulk behavior), and when the related cnode
                // is under consideration it will handle the edge in the other direction, so we
                // can avoid duplication.
                let mut related_supernodes_set = OrdArena::<P1, PBack, BulkBehavior>::new();
                for p_related_subnode in related_subnodes_set.keys() {
                    let p_related_supernode = channeler.get_supernode(*p_related_subnode).unwrap();
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

                // iterate through the subnodes one more time, finding bipartite edges between
                // the subnodes and related subnodes
                let mut subnode_adv = channeler.advancer_subnodes_of_node(p_consider);
                while let Some(p_subnode) = subnode_adv.advance(&channeler) {
                    let mut adv_edges = channeler.cnodes.advancer_surject(p_subnode);
                    while let Some(p_referent) = adv_edges.advance(&channeler.cnodes) {
                        if let Referent::CEdgeIncidence(p_cedge, i) =
                            channeler.cnodes.get_key(p_referent).unwrap()
                        {
                            let cedge = channeler.cedges.get(*p_cedge).unwrap();
                            let p_cnode =
                                channeler.cnodes.get_val(cedge.sink()).unwrap().p_this_cnode;
                            if subnode_set.find_key(&p_cnode).is_some() {
                                // the sink is in our sphere, if any source is from the related
                                // subnodes then we need
                                for p_source in cedge.sources() {
                                    let p_cnode = channeler
                                        .cnodes
                                        .get_val(cedge.sink())
                                        .unwrap()
                                        .p_this_cnode;
                                }
                            }
                            match cedge.programmability() {
                                Programmability::StaticLut(_) => todo!(),
                                Programmability::ArbitraryLut(_) => todo!(),
                                Programmability::SelectorLut(_) => todo!(),
                                Programmability::Bulk(_) => todo!(),
                            }
                        }
                    }
                }

                //channeler.make_cedge(&[], &[],
                // Programmability::Bulk(BulkBehavior { channel_entry_width:
                // todo!(), channel_exit_width: todo!(), lut_bits: todo!() }));
            }
        }

        current_lvl = next_lvl;
    }

    // if there are multiple cnodes are left in an anticlique, concentrate them into
    // a single top level node
    if channeler.top_level_cnodes.len() > 1 {
        let mut set = vec![];
        let mut max_lvl = 0;
        for p_cnode in channeler.top_level_cnodes.keys() {
            set.push(*p_cnode);
            max_lvl = max(max_lvl, channeler.cnodes.get_val(*p_cnode).unwrap().lvl)
        }
        channeler.make_top_level_cnode(set, max_lvl);
    }
}
