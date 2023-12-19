use awint::awint_dag::triple_arena::Ptr;

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

    /*
    /// Starting from unit `CNode`s and `CEdge`s describing all known low level
    /// progam methods, this generates a logarithmic tree of higher level
    /// `CNode`s and `CEdge`s that results in top level `CNode`s that have no
    /// `CEdges` to any other (and unless the graph was disconnected there will
    /// be only one top level `CNode`).
    ///
    /// We are currently assuming that `generate_hierarchy` is being run once on
    /// a graph of unit channel nodes and edges
    pub fn generate_hierarchy(&mut self) {
        // when running out of commonality merges to make, we progress by merging based
        // on the nodes with the largest fan-in
        ptr_struct!(P0; P1);

        let mut fan_in_priority = OrdArena::<P0, (usize, PCNode), ()>::new();
        let mut merge_priority = OrdArena::<P1, (usize, PCNode, PCNode), ()>::new();
        // handles the common task of updating priorities after adding a new `CNode` to
        // consideration
        fn add_p_cnode(
            channeler: &mut Channeler,
            fan_in_priority: &mut OrdArena<P0, (usize, PCNode), ()>,
            merge_priority: &mut OrdArena<P1, (usize, PCNode, PCNode), ()>,
            new_p_cnode: PCNode,
        ) {
            // add to fan in priority
            let mut fan_in_count = 0usize;
            if let Some(mut adv) = RegionAdvancer::new(&channeler.cedges, |_, cedge, ()| {
                cedge.p_sink.cmp(&new_p_cnode)
            }) {
                while let Some(_) = adv.advance(&channeler.cedges) {
                    fan_in_count = fan_in_count.checked_add(1).unwrap();
                }
                fan_in_priority
                    .insert((fan_in_count, new_p_cnode), ())
                    .1
                    .unwrap();
            }
        }
        let mut adv = self.cnodes.advancer();
        while let Some(p_cnode) = adv.advance(&self.cnodes) {
            add_p_cnode(self, &mut fan_in_priority, &mut merge_priority, p_cnode);
        }
        loop {
            if fan_in_priority.is_empty() && merge_priority.is_empty() {
                break
            }
            while let Some(p1_max) = merge_priority.max() {
                let merge = merge_priority.remove(p1_max).unwrap().0;
                // 1.
            }
            if let Some(p0_max) = fan_in_priority.max() {
                let p_cnode = fan_in_priority.remove(p0_max).unwrap().0 .1;
                // check that it is top level and wasn't subsumed by a merge step
                if self.cnodes.get(p_cnode).unwrap().supernodes.is_empty() {
                    // the subnodes will consist of the common sink node and its top level sources
                    let mut subnodes = vec![p_cnode];
                    let mut adv = RegionAdvancer::new(&self.cedges, |_, cedge, ()| {
                        cedge.p_sink.cmp(&p_cnode)
                    })
                    .unwrap();
                    while let Some(p_edge) = adv.advance(&self.cedges) {
                        let edge = self.cedges.get(p_edge).unwrap().0;
                        let p_source = edge.p_source;
                        let source = self.cnodes.get(p_source).unwrap();
                        if source.supernodes.is_empty() {
                            subnodes.push(p_source);
                        }
                    }
                    let new_p_cnode = self.make_top_level_cnode(subnodes);
                    add_p_cnode(self, &mut fan_in_priority, &mut merge_priority, new_p_cnode);
                }
            }
        }

        // just overwrite
        self.top_level_cnodes.clear();
        for (p_cnode, cnode) in &self.cnodes {
            if cnode.supernodes.is_empty() {
                self.top_level_cnodes.push(p_cnode);
            }
        }
    }
    */
}
