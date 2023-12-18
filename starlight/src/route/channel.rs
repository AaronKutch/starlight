use std::cmp::Ordering;

use awint::awint_dag::{
    smallvec::smallvec,
    triple_arena::{Advancer, Arena, OrdArena},
    EvalError,
};

use super::RegionAdvancer;
use crate::{awint_dag::smallvec::SmallVec, ensemble::PBack, triple_arena::ptr_struct};

ptr_struct!(PCNode; PCEdge; PBackToCNode);

/// A channel node
#[derive(Debug, Clone)]
pub struct CNode {
    /// Must be sorted.
    subnodes: Vec<PCNode>,
    /// Must be sorted.
    supernodes: Vec<PCNode>,
}

/// Used by higher order edges to tell what it is capable of overall
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BulkBehavior {
    /// The number of bits that can enter this channel
    channel_entry_width: usize,
    /// The number of bits that can exit this channel
    channel_exit_width: usize,
    /// For now, we just add up the number of LUT bits in the channel
    lut_bits: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Behavior {
    /// Routes the bit from `source` to `sink`
    RouteBit,
    /// Can behave as an arbitrary lookup table outputting a bit and taking the
    /// input bits.
    ArbitraryLut(PCNode, SmallVec<[PCNode; 4]>),
    /// Bulk behavior
    Bulk(BulkBehavior),
    /// Nothing can happen between nodes, used for connecting top level nodes
    /// that have no connection to each other
    Noop,
}

/// A description of bits to set in order to achieve some desired edge behavior.
/// For now we unconditionally specify bits, in the future it should be more
/// detailed to allow for more close by programs to coexist
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instruction {
    pub set_bits: SmallVec<[(PBack, bool); 4]>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Programmability {
    /// The behavior that can be programmed into this edge
    behavior: Behavior,
    /// The instruction required to get the desired behavior
    instruction: Instruction,
}

/// An edge between channels
#[derive(Debug, Clone)]
pub struct CEdge {
    /// The sink `CNode`
    sink: PCNode,
    /// The source `CNode`, this is readonly but bidirectional `Net`s can be
    /// represented with two `CEdge`s going both ways
    source: PCNode,

    // the variables above should uniquely determine a `CEdge`, we define `Eq` and `Ord` to only
    // respect the above and any insertion needs to check for duplicates
    /// Describes the required program to route a value (could be the `p_equiv`
    /// in a unit `CNode` or bulk routing through higher level `CNode`s) from
    /// the source to the sink.
    programmability: Programmability,
    // Ideally when `CNode`s are merged, they keep approximately the same weight distribution for
    // wide edges delay_weight: u64,
    //lagrangian_weight: u64,
}

impl PartialEq for CEdge {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.sink == other.sink
    }
}

impl Eq for CEdge {}

impl PartialOrd for CEdge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.source.partial_cmp(&other.source) {
            Some(Ordering::Equal) => {}
            ord => return ord,
        }
        self.sink.partial_cmp(&other.sink)
    }
}

impl Ord for CEdge {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.source.cmp(&other.source) {
            Ordering::Equal => {}
            ord => return ord,
        }
        self.sink.cmp(&other.sink)
    }
}

impl CEdge {
    pub fn programmability(&self) -> &Programmability {
        &self.programmability
    }
}

/// Management struct for channel nodes and edges
#[derive(Debug, Clone)]
pub struct Channeler {
    cnodes: Arena<PCNode, CNode>,
    cedges: OrdArena<PCEdge, CEdge, ()>,
    /// The plan is that this always ends up with a single top level node, with
    /// all unconnected graphs being connected with `Behavior::Noop` so that the
    /// normal algorithm can allocate over them
    top_level_cnodes: SmallVec<[PCNode; 1]>,
    /// On hard dependencies where a path needs to connect to a particular
    /// `PBack`, valid descencions in the `CNode` hierarchy are determined by
    /// `find_with` to first get to the desired `PBack` section, then linear
    /// iterating to figure out which `CNode`s contain the `PBack`. The space is
    /// limited to a `n*log(n)`, there is probably some inevitable `n*log(n)`
    /// cost somewhere.
    backref_to_cnode: OrdArena<PBackToCNode, (PBack, PCNode), ()>,
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

impl Channeler {
    pub fn new() -> Self {
        Self {
            cnodes: Arena::new(),
            cedges: OrdArena::new(),
            top_level_cnodes: smallvec![],
            backref_to_cnode: OrdArena::new(),
        }
    }

    pub fn make_cnode(&mut self, p_equiv: PBack) -> PCNode {
        if self
            .backref_to_cnode
            .find_with(|_, (p_back, _), _| p_back.cmp(&p_equiv))
            .is_some()
        {
            // there shouldn't be redundant `CNode`s
            panic!()
        }
        let res = self.cnodes.insert(CNode {
            subnodes: vec![],
            supernodes: vec![],
        });
        self.top_level_cnodes.push(res);
        res
    }

    pub fn make_cedge(
        &mut self,
        source: PCNode,
        sink: PCNode,
        programmability: Programmability,
    ) -> PCEdge {
        let (p_new, duplicate) = self.cedges.insert(
            CEdge {
                source,
                sink,
                programmability,
            },
            (),
        );
        // there may be future circumstances where we allow this and combine
        // appropriately, but disallow for now
        duplicate.unwrap();
        p_new
    }

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
        ptr_struct!(P0);
        let mut fan_in_priority = OrdArena::<P0, usize, PCNode>::new();
        /*for (p_cnode, cnode) in self.cnodes {
            let p_cedge = self.cedges.find_with(|_, cedge, ()| {
                cedge.sink.cmp(&p_cnode)
            });
        }*/
    }

    pub fn get_cnode(&self, p_cnode: PCNode) -> Option<&CNode> {
        self.cnodes.get(p_cnode)
    }

    pub fn get_cedge(&self, p_cedge: PCEdge) -> Option<&CEdge> {
        self.cedges.get(p_cedge).map(|(cedge, _)| cedge)
    }

    /// Starting from `p_cnode` assumed to contain `p_back`, this returns valid
    /// subnodes that still contain `PBack`
    pub fn valid_cnode_descensions(&self, p_cnode: PCNode, p_back: PBack) -> SmallVec<[PCNode; 4]> {
        let cnode = self.cnodes.get(p_cnode).unwrap();
        if let Some(mut adv) = RegionAdvancer::new(&self.backref_to_cnode, |_, (p_back1, _), ()| {
            p_back1.cmp(&p_back)
        }) {
            // uses the fact that `subnodes` is ordered to linearly iterate over a region
            let mut res = smallvec![];
            let mut i = 0;
            'outer: while let Some(p) = adv.advance(&self.backref_to_cnode) {
                let (_, p_cnode1) = self.backref_to_cnode.get_key(p).unwrap();
                loop {
                    if i >= cnode.subnodes.len() {
                        break 'outer;
                    }
                    match cnode.subnodes[i].cmp(&p_cnode1) {
                        Ordering::Less => {
                            i += 1;
                        }
                        Ordering::Equal => {
                            res.push(*p_cnode1);
                            i += 1;
                            break
                        }
                        Ordering::Greater => break,
                    }
                }
            }
            res
        } else {
            unreachable!()
        }
    }

    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        fn is_sorted_and_unique(x: &[PCNode]) -> bool {
            for i in 1..x.len() {
                if x[i - 1] >= x[i] {
                    return false
                }
            }
            true
        }
        // verify all pointer validities and sorting invariants first
        for p_cnode in self.cnodes.ptrs() {
            let cnode = &self.cnodes[p_cnode];
            if !is_sorted_and_unique(&cnode.subnodes) {
                return Err(EvalError::OtherString(format!(
                    "{cnode:?}.subnodes is unsorted"
                )))
            }
            if !is_sorted_and_unique(&cnode.supernodes) {
                return Err(EvalError::OtherString(format!(
                    "{cnode:?}.supernodes is unsorted"
                )))
            }
            for subnode in &cnode.subnodes {
                if !self.cnodes.contains(*subnode) {
                    return Err(EvalError::OtherString(format!(
                        "{cnode:?}.subnodes[{subnode}] is invalid"
                    )))
                }
            }
            for supernode in &cnode.supernodes {
                if !self.cnodes.contains(*supernode) {
                    return Err(EvalError::OtherString(format!(
                        "{cnode:?}.subnodes[{supernode}] is invalid"
                    )))
                }
            }
        }
        for p_cedge in self.cedges.ptrs() {
            let cedge = &self.cedges.get_key(p_cedge).unwrap();
            if !self.cnodes.contains(cedge.source) {
                return Err(EvalError::OtherString(format!(
                    "{cedge:?}.source {} is invalid",
                    cedge.source
                )))
            }
            if !self.cnodes.contains(cedge.sink) {
                return Err(EvalError::OtherString(format!(
                    "{cedge:?}.sink {} is invalid",
                    cedge.sink
                )))
            }
        }
        for p_cnode in &self.top_level_cnodes {
            if !self.cnodes.contains(*p_cnode) {
                return Err(EvalError::OtherString(format!(
                    "top_level_cnodes {p_cnode} is invalid"
                )))
            }
        }
        for p_back_to_cnode in self.backref_to_cnode.ptrs() {
            let (_, p_cnode) = self.backref_to_cnode.get_key(p_back_to_cnode).unwrap();
            if !self.cnodes.contains(*p_cnode) {
                return Err(EvalError::OtherString(format!(
                    "{p_back_to_cnode} key {p_cnode} is invalid"
                )))
            }
        }
        // check basic tree invariants
        for p_cnode in &self.top_level_cnodes {
            if !self.cnodes[p_cnode].supernodes.is_empty() {
                return Err(EvalError::OtherString(format!(
                    "top_level_cnodes {p_cnode} is not a top level `CNode`"
                )))
            }
        }
        for p_cnode in self.cnodes.ptrs() {
            let cnode = &self.cnodes[p_cnode];
            for subnode in &cnode.subnodes {
                if self.cnodes[subnode]
                    .supernodes
                    .binary_search(&p_cnode)
                    .is_err()
                {
                    return Err(EvalError::OtherString(format!(
                        "{cnode:?} subnode {subnode} does not roundtrip"
                    )))
                }
            }
            for supernode in &cnode.supernodes {
                if self.cnodes[supernode]
                    .subnodes
                    .binary_search(&p_cnode)
                    .is_err()
                {
                    return Err(EvalError::OtherString(format!(
                        "{cnode:?} supernode {supernode} does not roundtrip"
                    )))
                }
            }
        }
        Ok(())
    }
}
