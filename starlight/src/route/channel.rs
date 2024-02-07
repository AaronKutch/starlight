use std::num::NonZeroU64;

use awint::awint_dag::triple_arena::{Arena, OrdArena, Ptr, SurjectArena};

use crate::{
    ensemble::PBack,
    route::{CEdge, CNode, Programmability},
    triple_arena::ptr_struct,
    Error,
};

ptr_struct!(P0; PTopLevel);

// TODO Mapping nodes, Divergence edges, and Convergence edges? Or are we only
// going to end up with Convergence edges and the hyperpath claws work from the
// sink perspectives?

#[derive(Debug, Clone, Copy)]
pub enum Referent<PCNode: Ptr, PCEdge: Ptr> {
    ThisCNode,
    SubNode(PCNode),
    /// The index is `None` if it is a sink, TODO use a NonZeroInxVec if we
    /// stick with this
    CEdgeIncidence(PCEdge, Option<usize>),
}

#[derive(Debug, Clone)]
pub struct Channeler<PCNode: Ptr, PCEdge: Ptr> {
    pub cnodes: SurjectArena<PCNode, Referent<PCNode, PCEdge>, CNode<PCNode>>,
    pub cedges: Arena<PCEdge, CEdge<PCNode>>,
    /// The plan is that this always ends up with a single top level node, with
    /// all unconnected graphs being connected with `Behavior::Noop` so that the
    /// normal algorithm can allocate over them
    pub top_level_cnodes: OrdArena<PTopLevel, PCNode, ()>,
    // needed for the unit edges to find incidences
    pub ensemble_backref_to_channeler_backref: OrdArena<P0, PBack, PCNode>,
    // used by the `related_nodes` function
    pub related_visit: NonZeroU64,
}

impl<PCNode: Ptr, PCEdge: Ptr> Channeler<PCNode, PCEdge> {
    pub fn empty() -> Self {
        Self {
            cnodes: SurjectArena::new(),
            cedges: Arena::new(),
            top_level_cnodes: OrdArena::new(),
            ensemble_backref_to_channeler_backref: OrdArena::new(),
            related_visit: NonZeroU64::new(2).unwrap(),
        }
    }

    pub fn find_channeler_cnode(&self, ensemble_backref: PBack) -> Option<PCNode> {
        let p = self
            .ensemble_backref_to_channeler_backref
            .find_key(&ensemble_backref)?;
        let p_ref = self
            .ensemble_backref_to_channeler_backref
            .get(p)
            .map(|(_, q)| *q)?;
        Some(self.cnodes.get_val(p_ref).unwrap().p_this_cnode)
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
        // return errors in order of most likely to be root cause

        // first check that surjects self refs aren't broken by themselves
        for p_back in self.cnodes.ptrs() {
            let cnode = self.cnodes.get_val(p_back).unwrap();
            if let Some(Referent::ThisCNode) = self.cnodes.get_key(cnode.p_this_cnode) {
                if !self.cnodes.in_same_set(p_back, cnode.p_this_cnode).unwrap() {
                    return Err(Error::OtherString(format!(
                        "{cnode:?}.p_this_cnode roundtrip fail"
                    )))
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{cnode:?}.p_this_cnode is invalid"
                )))
            }
            // need to roundtrip in both directions to ensure existence and uniqueness of a
            // `ThisCNode` for each surject
            if let Some(Referent::ThisCNode) = self.cnodes.get_key(p_back) {
                if p_back != cnode.p_this_cnode {
                    return Err(Error::OtherString(format!(
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
                Referent::CEdgeIncidence(p_cedge, i) => {
                    if let Some(cedges) = self.cedges.get(*p_cedge) {
                        if let Some(source_i) = i {
                            if *source_i > cedges.sources().len() {
                                return Err(Error::OtherString(format!(
                                    "{referent:?} roundtrip out of bounds"
                                )))
                            }
                        }
                        false
                    } else {
                        true
                    }
                }
            };
            if invalid {
                return Err(Error::OtherString(format!("{referent:?} is invalid")))
            }
        }
        // supernode pointers are a special referent stored in the `CNode`s themselves
        for cnode in self.cnodes.vals() {
            if let Some(p_supernode) = cnode.p_supernode {
                if !self.cnodes.contains(p_supernode) {
                    return Err(Error::OtherString(format!(
                        "{cnode:?}.p_supernode is invalid"
                    )))
                }
            }
        }
        for p_cedge in self.cedges.ptrs() {
            let cedge = self.cedges.get(p_cedge).unwrap();
            for p_cnode in cedge.sources().iter() {
                if !self.cnodes.contains(*p_cnode) {
                    return Err(Error::OtherString(format!(
                        "{cedge:?} source {p_cnode:?} is invalid",
                    )))
                }
            }
            if !self.cnodes.contains(cedge.sink()) {
                return Err(Error::OtherString(format!(
                    "{cedge:?} sink {:?} is invalid",
                    cedge.sink()
                )))
            }
        }
        for p_cnode in self.top_level_cnodes.keys() {
            if !self.cnodes.contains(*p_cnode) {
                return Err(Error::OtherString(format!(
                    "top_level_cnodes {p_cnode:?} is invalid"
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
                    let (referent, subnode) = self.cnodes.get(*p_subnode).unwrap();
                    if let Referent::ThisCNode = referent {
                        !subnode.p_supernode.is_some_and(|p| p == p_back)
                    } else {
                        true
                    }
                }
                Referent::CEdgeIncidence(p_cedge, i) => {
                    let cedge = self.cedges.get(*p_cedge).unwrap();
                    if let Some(source_i) = *i {
                        if let Some(source) = cedge.sources().get(source_i) {
                            if let Referent::CEdgeIncidence(p_cedge1, i1) =
                                self.cnodes.get_key(*source).unwrap()
                            {
                                (*p_cedge != *p_cedge1) || (*i != *i1)
                            } else {
                                true
                            }
                        } else {
                            true
                        }
                    } else if let Referent::CEdgeIncidence(p_cedge1, i1) =
                        self.cnodes.get_key(cedge.sink()).unwrap()
                    {
                        (*p_cedge != *p_cedge1) || i1.is_some()
                    } else {
                        true
                    }
                }
            };
            if fail {
                return Err(Error::OtherString(format!("{referent:?} roundtrip fail")))
            }
        }
        // non `Ptr` validities
        for p_cedge in self.cedges.ptrs() {
            let cedge = self.cedges.get(p_cedge).unwrap();
            let sources_len = cedge.sources().len();
            let ok = match cedge.programmability() {
                Programmability::TNode => sources_len == 1,
                Programmability::StaticLut(lut) => {
                    // TODO find every place I did the trailing zeros thing and have a function that
                    // does the more efficient thing the core `lut_` function does
                    lut.bw().is_power_of_two()
                        && (lut.bw().trailing_zeros() as usize == sources_len)
                }
                Programmability::ArbitraryLut(lut) => {
                    lut.len().is_power_of_two()
                        && ((lut.len().trailing_zeros() as usize) == sources_len)
                }
                Programmability::SelectorLut(selector_lut) => {
                    selector_lut.verify_integrity(sources_len)?;
                    true
                }
                Programmability::Bulk(bulk_behavior) => {
                    bulk_behavior.channel_entry_widths.len() == cedge.sources().len()
                }
            };
            if !ok {
                return Err(Error::OtherString(format!(
                    "{cedge:?} an invariant is broken"
                )))
            }
        }
        // check `top_level_cnodes`
        for p_cnode in self.top_level_cnodes.keys() {
            let (referent, cnode) = self.cnodes.get(*p_cnode).unwrap();
            if let Referent::ThisCNode = referent {
                if cnode.p_supernode.is_some() {
                    return Err(Error::OtherString(format!(
                        "top_level_cnodes {p_cnode:?} disagrees with p_supernode.is_some()"
                    )));
                }
            } else {
                return Err(Error::OtherString(format!(
                    "top_level_cnodes {p_cnode:?} referent is the wrong kind"
                )));
            }
        }
        for cnode in self.cnodes.vals() {
            if cnode.p_supernode.is_none()
                && self
                    .top_level_cnodes
                    .find_key(&cnode.p_this_cnode)
                    .is_none()
            {
                return Err(Error::OtherString(format!(
                    "{:?} not contained in top_level_cnodes when it should be",
                    cnode.p_this_cnode
                )));
            }
        }
        // insure `CEdge`s are only between nodes on the same level
        for (p_cedge, cedge) in &self.cedges {
            let mut lvl = None;
            let mut res = Ok(());
            cedge.incidents(|p_cnode| {
                let other_lvl = self.cnodes.get_val(p_cnode).unwrap().lvl;
                if let Some(lvl) = lvl {
                    if lvl != other_lvl {
                        res = Err(Error::OtherString(format!(
                            "{p_cedge:?} incidents not all on same level",
                        )));
                    }
                } else {
                    lvl = Some(other_lvl);
                }
            })
        }
        Ok(())
    }
}
