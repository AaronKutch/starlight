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
        self.top_level_cnodes.push(p_cnode);
        p_cnode
    }
}

// - A `CNode` cannot have exactly one subnode and must have either zero or at
//   least two subnodes
// - the immediate subnodes of a `CNode` must be in a clique with `CEdge`s

/*
consider a loop of `CNode`s like this
0 --- 1
|     |
|     |
2 --- 3

If higher `CNode`s and edges formed like

   01
  /   \
02    13
  \   /
   23

It could cause an infinite loop, we need to guarantee logarithmic overhead
with `CEdges` being made such that e.x. 02 should connect with 13 because
02 subnodes connect with 1 and 3 which are subnodes of 13.

   01
  / | \
02 -- 13
  \ | /
   23

the next level is

0123

for larger loops it will be like

0--1--2--3--4--5--6--7--0 (wraps around to 0)
       ___   ___   ___   ___
      /   \ /   \ /   \ /   \
 01-12-23-34-45-56-67-70-01-12
   \  /  \  /  \  /  \  /
    --    --    --    --

// we do not want this to continue, or else we end up with n^2 space
   0123  2345  4567  6701
      1234  3456  5670  7012

we notice that 12 and 23 share 0.5 of their nodes in common, what we
do is merge a "extended clique" of cliques sharing the edge between
the two nodes, specifically the 01-12-23 clique and the 12-23-34 clique

         ...
 01234-45-56-67-70-01234

the 01-12-23 subedges are still in the hierarchy, if the 23-34 edge is selected
for the commonality merge, 01234 is found as a supernode of 34, and the proposed
merge resulting in 12345 shares 12 and 23 with 01234 (if more than or equal to
half of the subnodes are shared with respect to one or the other (2 out of
01,12,23,34 for one or 2 out of 12,23,34,45 for the other), it should not be
made). 34-45 would also be too close.
45-56 however is successful resulting in 34567 which has the desired overlap.
70 is left without a supernode on this level, but it joins a three clique to
result in the final top level node

       ...
01234-34567-70-01234

0123457

8 -> 8 -> 3 -> 1 seems right, the first reduction is stalling for wider useful
cliques for the descension algorithm, and this is quickly reduced down in
the logarithmic tree we want

*/

/// Starting from unit `CNode`s and `CEdge`s describing all known low level
/// progam methods, this generates a logarithmic tree of higher level
/// `CNode`s and `CEdge`s that results in top level `CNode`s that have no
/// `CEdges` to any other (and unless the graph was disconnected there will
/// be only one top level `CNode`).
///
/// We are currently assuming that `generate_hierarchy` is being run once on
/// a graph of unit channel nodes and edges
pub fn generate_hierarchy(channeler: &mut Channeler) {
    // TODO currently we are doing a simpler strategy of merging pairs on distinct
    // layers, need to methodically determine what we really want
    ptr_struct!(P0);

    todo!()
}
