use awint::awint_dag::triple_arena::{Advancer, Arena, OrdArena, SurjectArena};

use crate::{
    ensemble,
    route::{CEdge, CNode, PCEdge, Programmability},
    triple_arena::ptr_struct,
    Error,
};

ptr_struct!(P0; PTopLevel; PBack);

#[derive(Debug, Clone, Copy)]
pub enum Referent {
    ThisCNode,
    SubNode(PBack),
    SuperNode(PBack),
    /// The index is `None` if it is a sink, TODO use a NonZeroInxVec if we
    /// stick with this
    CEdgeIncidence(PCEdge, Option<usize>),
    // TODO do we actually need this?
    EnsembleBackRef(ensemble::PBack),
}

#[derive(Debug, Clone)]
pub struct Channeler {
    pub cnodes: SurjectArena<PBack, Referent, CNode>,
    pub cedges: Arena<PCEdge, CEdge>,
    /// The plan is that this always ends up with a single top level node, with
    /// all unconnected graphs being connected with `Behavior::Noop` so that the
    /// normal algorithm can allocate over them
    pub top_level_cnodes: OrdArena<PTopLevel, PBack, ()>,
    // needed for the unit edges to find incidences
    pub ensemble_backref_to_channeler_backref: OrdArena<P0, ensemble::PBack, PBack>,
}

impl Channeler {
    pub fn empty() -> Self {
        Self {
            cnodes: SurjectArena::new(),
            cedges: Arena::new(),
            top_level_cnodes: OrdArena::new(),
            ensemble_backref_to_channeler_backref: OrdArena::new(),
        }
    }

    pub fn find_channeler_backref(&self, ensemble_backref: ensemble::PBack) -> Option<PBack> {
        let p = self
            .ensemble_backref_to_channeler_backref
            .find_key(&ensemble_backref)?;
        self.ensemble_backref_to_channeler_backref
            .get(p)
            .map(|(_, q)| *q)
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
                Referent::SuperNode(p_supernode) => !self.cnodes.contains(*p_supernode),
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
                Referent::EnsembleBackRef(_) => false,
            };
            if invalid {
                return Err(Error::OtherString(format!("{referent:?} is invalid")))
            }
        }
        for p_cedge in self.cedges.ptrs() {
            let cedge = self.cedges.get(p_cedge).unwrap();
            for p_cnode in cedge.sources().iter() {
                if !self.cnodes.contains(*p_cnode) {
                    return Err(Error::OtherString(format!(
                        "{cedge:?} source {p_cnode} is invalid",
                    )))
                }
            }
            if !self.cnodes.contains(cedge.sink()) {
                return Err(Error::OtherString(format!(
                    "{cedge:?} sink {} is invalid",
                    cedge.sink()
                )))
            }
        }
        for p_cnode in self.top_level_cnodes.keys() {
            if !self.cnodes.contains(*p_cnode) {
                return Err(Error::OtherString(format!(
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
                Referent::EnsembleBackRef(_) => false,
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
                Programmability::Bulk(_) => todo!(),
            };
            if !ok {
                return Err(Error::OtherString(format!(
                    "{cedge:?} an invariant is broken"
                )))
            }
        }
        // TODO check uniqueness of super/sub relations, check that there is at most one
        // supernode per node
        for cnode in self.cnodes.vals() {
            let contained = self
                .top_level_cnodes
                .find_key(&cnode.p_this_cnode)
                .is_some();
            if cnode.has_supernode == contained {
                return Err(Error::OtherString(format!(
                    "{cnode:?}.has_supernode is wrong"
                )));
            }
            let mut adv = self.cnodes.advancer_surject(cnode.p_this_cnode);
            let mut found_super_node = false;
            while let Some(p) = adv.advance(&self.cnodes) {
                if let Referent::SuperNode(_) = self.cnodes.get_key(p).unwrap() {
                    found_super_node = true;
                    break
                }
            }
            if contained && found_super_node {
                return Err(Error::OtherString(format!(
                    "{cnode:?} has a super node when it is also a top level node"
                )));
            }
            if !(contained || found_super_node) {
                return Err(Error::OtherString(format!(
                    "{cnode:?} is not top level node but does not have a super node"
                )));
            }
        }
        Ok(())
    }
}
