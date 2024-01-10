use awint::awint_dag::{
    smallvec::smallvec,
    triple_arena::{Arena, OrdArena, SurjectArena},
    EvalError,
};

use crate::{
    awint_dag::smallvec::SmallVec,
    ensemble,
    route::{CEdge, CNode, PCEdge, Programmability},
    triple_arena::ptr_struct,
};

ptr_struct!(P0; PBack);

#[derive(Debug, Clone, Copy)]
pub enum Referent {
    ThisCNode,
    SubNode(PBack),
    SuperNode(PBack),
    /// The bool indicates if it is a sink
    CEdgeIncidence(PCEdge, usize, bool),
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
    // needed for the unit edges to find incidences
    pub ensemble_backref_to_channeler_backref: OrdArena<P0, ensemble::PBack, PBack>,
}

impl Channeler {
    pub fn empty() -> Self {
        Self {
            cnodes: SurjectArena::new(),
            cedges: Arena::new(),
            top_level_cnodes: smallvec![],
            ensemble_backref_to_channeler_backref: OrdArena::new(),
        }
    }

    /*
    /// Starting from `p_cnode` assumed to contain `p_back`, this returns valid
    /// subnodes that still contain `ensemble::PBack`
    pub fn valid_cnode_descensions(&self, p_cnode: PCNode, p_back: ensemble::PBack)
    -> SmallVec<[PCNode; 4]> {
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
                Referent::CEdgeIncidence(p_cedge, i, is_sink) => {
                    if let Some(cedges) = self.cedges.get(*p_cedge) {
                        if *is_sink {
                            if *i > cedges.sinks().len() {
                                return Err(EvalError::OtherString(format!(
                                    "{referent:?} roundtrip out of bounds"
                                )))
                            }
                        } else if *i > cedges.sources().len() {
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
        for p_cedge in self.cedges.ptrs() {
            let cedge = self.cedges.get(p_cedge).unwrap();
            for p_cnode in cedge.sources().iter() {
                if !self.cnodes.contains(*p_cnode) {
                    return Err(EvalError::OtherString(format!(
                        "{cedge:?}.p_cnodes {p_cnode} is invalid",
                    )))
                }
            }
            for p_cnode in cedge.sinks().iter() {
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
                Referent::CEdgeIncidence(p_cedge, i, is_sink) => {
                    let cedge = self.cedges.get(*p_cedge).unwrap();
                    let mut res = false;
                    cedge.incidents(|incident| {
                        let p_cnode = cedge.sinks()[*i];
                        if let Referent::CEdgeIncidence(p_cedge1, i1, is_sink1) =
                            self.cnodes.get_key(p_cnode).unwrap()
                        {
                            if (*p_cedge != *p_cedge1) || (*i != *i1) || (*is_sink != *is_sink1) {
                                res = true;
                            }
                        } else {
                            res = true;
                        }
                    });
                    res
                }
                Referent::EnsembleBackRef(_) => todo!(),
            };
            if fail {
                return Err(EvalError::OtherString(format!(
                    "{referent:?} roundtrip fail"
                )))
            }
        }
        // non `Ptr` validities
        for p_cedge in self.cedges.ptrs() {
            let cedge = self.cedges.get(p_cedge).unwrap();
            let incidents_len = cedge.incidents_len();
            let sources_len = cedge.sources().len();
            let sinks_len = cedge.sinks().len();
            let ok = match cedge.programmability() {
                Programmability::Noop => incidents_len == 0,
                Programmability::StaticLut(lut) => {
                    // TODO find every place I did the trailing zeros thing and have a function that
                    // does the more efficient thing the core `lut_` function does
                    lut.bw().is_power_of_two()
                        && (lut.bw().trailing_zeros() as usize == sources_len)
                        && (sinks_len == 1)
                }
                Programmability::ArbitraryLut(lut) => {
                    lut.len().is_power_of_two()
                        && ((lut.len().trailing_zeros() as usize) == sources_len)
                        && (sinks_len == 1)
                }
                Programmability::SelectorLut(selector_lut) => {
                    selector_lut.verify_integrity(sources_len, sinks_len)?;
                    true
                }
                Programmability::Bulk(_) => todo!(),
            };
            if !ok {
                return Err(EvalError::OtherString(format!(
                    "{cedge:?} an invariant is broken"
                )))
            }
        }
        Ok(())
    }
}
