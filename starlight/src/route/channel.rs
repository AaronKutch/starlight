use awint::awint_dag::{
    smallvec::smallvec,
    triple_arena::{Arena, SurjectArena},
    EvalError,
};

use crate::{
    awint_dag::smallvec::SmallVec,
    ensemble,
    route::{CEdge, CNode, PCEdge},
    triple_arena::ptr_struct,
};

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

ptr_struct!(PBack);

#[derive(Debug, Clone, Copy)]
pub enum Referent {
    ThisCNode,
    SubNode(PBack),
    SuperNode(PBack),
    CEdgeIncidence(PCEdge, usize),
    EnsembleBackRef(ensemble::PBack),
}

#[derive(Debug, Clone)]
pub struct Channeler {
    pub cnodes: SurjectArena<PBack, Referent, CNode>,
    pub cedges: Arena<PCEdge, CEdge>,
    /// The plan is that this always ends up with a single top level node, with
    /// all unconnected graphs being connected with `Behavior::Noop` so that the
    /// normal algorithm can allocate over them
    pub top_level_cnodes: SmallVec<[PBack; 1]>,
}

impl Channeler {
    pub fn new() -> Self {
        Self {
            cnodes: SurjectArena::new(),
            cedges: Arena::new(),
            top_level_cnodes: smallvec![],
        }
    }

    /*
    /// Starting from `p_cnode` assumed to contain `p_back`, this returns valid
    /// subnodes that still contain `ensemble::PBack`
    pub fn valid_cnode_descensions(&self, p_cnode: PCNode, p_back: ensemble::PBack) -> SmallVec<[PCNode; 4]> {
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
    }*/

    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        // return errors in order of most likely to be root cause

        // first check that surjects self refs aren't broken by themselves
        for p_back in self.cnodes.ptrs() {
            let cnode = self.cnodes.get_val(p_back).unwrap();
            if let Some(Referent::ThisCNode) = self.cnodes.get_key(cnode.p_this_cnode) {
                if !self.cnodes.in_same_set(p_back, cnode.p_this_cnode).unwrap() {
                    return Err(EvalError::OtherString(format!(
                        "{cnode:?}.p_this_cnode roundtrip fail"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{cnode:?}.p_this_cnode is invalid"
                )))
            }
            // need to roundtrip in both directions to ensure existence and uniqueness of a
            // `ThisCNode` for each surject
            if let Some(Referent::ThisCNode) = self.cnodes.get_key(p_back) {
                if p_back != cnode.p_this_cnode {
                    return Err(EvalError::OtherString(format!(
                        "{cnode:?}.p_this_cnode roundtrip fail"
                    )))
                }
            }
        }
        // check other referent validities
        for referent in self.cnodes.keys() {
            let invalid = match referent {
                // already checked
                Referent::ThisCNode => false,
                Referent::SubNode(p_subnode) => !self.cnodes.contains(*p_subnode),
                Referent::SuperNode(p_supernode) => !self.cnodes.contains(*p_supernode),
                Referent::CEdgeIncidence(p_cedge, i) => {
                    if let Some(cedges) = self.cedges.get(*p_cedge) {
                        if *i > cedges.incidences.len() {
                            return Err(EvalError::OtherString(format!(
                                "{referent:?} roundtrip out of bounds"
                            )))
                        }
                        false
                    } else {
                        true
                    }
                }
                Referent::EnsembleBackRef(_) => false,
            };
            if invalid {
                return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
            }
        }
        // other kinds of validity
        for p_cedge in self.cedges.ptrs() {
            let cedge = &self.cedges.get(p_cedge).unwrap();
            for p_cnode in &cedge.incidences {
                if !self.cnodes.contains(*p_cnode) {
                    return Err(EvalError::OtherString(format!(
                        "{cedge:?}.p_cnodes {p_cnode} is invalid",
                    )))
                }
            }
        }
        for p_cnode in &self.top_level_cnodes {
            if !self.cnodes.contains(*p_cnode) {
                return Err(EvalError::OtherString(format!(
                    "top_level_cnodes {p_cnode} is invalid"
                )))
            }
        }
        // Other roundtrips from `backrefs` direction to ensure bijection
        for p_back in self.cnodes.ptrs() {
            let referent = self.cnodes.get_key(p_back).unwrap();
            let fail = match referent {
                // already checked
                Referent::ThisCNode => false,
                Referent::SubNode(p_subnode) => {
                    let subnode = self.cnodes.get_key(*p_subnode).unwrap();
                    if let Referent::SuperNode(p_supernode) = subnode {
                        *p_supernode != p_back
                    } else {
                        true
                    }
                }
                Referent::SuperNode(p_supernode) => {
                    let supernode = self.cnodes.get_key(*p_supernode).unwrap();
                    if let Referent::SubNode(p_subnode) = supernode {
                        *p_subnode != p_back
                    } else {
                        true
                    }
                }
                Referent::CEdgeIncidence(p_cedge, i) => {
                    let cedge = self.cedges.get(*p_cedge).unwrap();
                    let p_cnode = cedge.incidences[*i];
                    if let Referent::CEdgeIncidence(p_cedge1, i1) =
                        self.cnodes.get_key(p_cnode).unwrap()
                    {
                        (*p_cedge != *p_cedge1) || (*i != *i1)
                    } else {
                        true
                    }
                }
                Referent::EnsembleBackRef(_) => todo!(),
            };
            if fail {
                return Err(EvalError::OtherString(format!(
                    "{referent:?} roundtrip fail"
                )))
            }
        }
        // tree invariants
        Ok(())
    }
}
