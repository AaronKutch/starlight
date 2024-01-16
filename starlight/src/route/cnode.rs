use awint::awint_dag::triple_arena::{ptr_struct, Ptr};

use crate::route::{channel::Referent, Channeler, PBack};

/// A channel node
#[derive(Debug, Clone, Default)]
pub struct CNode {
    pub p_this_cnode: PBack,
}

impl Channeler {
    /// Given the `subnodes` (which should point to unique `ThisCNode`s) for a
    /// new top level `CNode`, this will manage the backrefs
    pub fn make_top_level_cnode(&mut self, subnodes: Vec<PBack>) -> PBack {
        let p_cnode = self
            .cnodes
            .insert_with(|p_this_cnode| (Referent::ThisCNode, CNode { p_this_cnode }));
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
            *self.cnodes.get_key_mut(p_subnode).unwrap() = Referent::SuperNode(p_supernode);
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
    ptr_struct!(P0);

    // For each cnode on a given level, we will attempt to concentrate it and all
    // its neighbors.
    //

    // when making a new top level cnode, for each subnode of the new node check
    // for other supernodes. For each transverse node Tally the number of times
    // each transverse node is seen. If it is seen more than once, cancel making
    // the new cnode.

    // if there are multiple cnodes are left in an anticlique, concentrate them into
    // a single top level node
    if channeler.top_level_cnodes.len() > 1 {
        let mut set = vec![];
        for p_cnode in channeler.top_level_cnodes.keys() {
            set.push(*p_cnode);
        }
        channeler.make_top_level_cnode(set);
    }
}
