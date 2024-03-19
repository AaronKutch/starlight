use std::num::NonZeroU64;

use awint::awint_dag::triple_arena::{Arena, OrdArena, Recast, Recaster};

use crate::{
    ensemble::{Ensemble, PBack, PEquiv},
    route::{CEdge, CNode, PBackToCnode, PCEdge, PCNode, Programmability},
    utils::binary_search_similar_by,
    Error,
};

/// A channeling graph for a target
#[derive(Debug, Clone)]
pub struct Channeler {
    pub cnodes: Arena<PCNode, CNode>,
    pub cedges: Arena<PCEdge, CEdge>,
    pub(crate) p_back_to_cnode: OrdArena<PBackToCnode, PBack, PCNode>,
    // used by algorithms to avoid `OrdArena`s
    pub alg_visit: NonZeroU64,
}

impl Recast<PCNode> for Channeler {
    fn recast<R: Recaster<Item = PCNode>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.cedges.recast(recaster)?;
        self.p_back_to_cnode.recast(recaster)
    }
}

impl Channeler {
    pub fn empty() -> Self {
        Self {
            cnodes: Arena::new(),
            cedges: Arena::new(),
            p_back_to_cnode: OrdArena::new(),
            alg_visit: NonZeroU64::new(2).unwrap(),
        }
    }

    pub fn next_alg_visit(&mut self) -> NonZeroU64 {
        self.alg_visit = self.alg_visit.checked_add(1).unwrap();
        self.alg_visit
    }

    /// Finds the base level `PCNode` corresponding to a `PEquiv` from the
    /// target
    pub fn translate_equiv(&self, p_equiv: PEquiv) -> Option<PCNode> {
        let p0 = self.p_back_to_cnode.find_key(&p_equiv.into())?;
        Some(*self.p_back_to_cnode.get_val(p0).unwrap())
    }

    /// Finds the base level `PCNode` corresponding to any `PBack` from the
    /// target
    pub fn translate_backref(
        &self,
        ensemble: &Ensemble,
        p_back: PBack,
    ) -> Option<(PEquiv, PCNode)> {
        let p_equiv = ensemble.get_p_equiv(p_back)?;
        let p0 = self.p_back_to_cnode.find_key(&p_equiv.into())?;
        Some((p_equiv, *self.p_back_to_cnode.get_val(p0).unwrap()))
    }

    /// Sets the correspondence to a node
    pub fn set_translation(&mut self, p_equiv: PEquiv, p_forward: PCNode) -> Option<()> {
        let p0 = self.p_back_to_cnode.find_key(&p_equiv.into())?;
        *self.p_back_to_cnode.get_val_mut(p0).unwrap() = p_forward;
        Some(())
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
        // return errors in order of most likely to be root cause

        // make sure some things are sorted
        for (p_cnode, cnode) in &self.cnodes {
            for i in 1..cnode.p_subnodes.len() {
                if cnode.p_subnodes[i - 1] >= cnode.p_subnodes[i] {
                    return Err(Error::OtherString(format!(
                        "{p_cnode} {cnode:?}.p_subnodes is unsorted or not hereditary"
                    )))
                }
            }
            for i in 1..cnode.source_incidents.len() {
                if cnode.source_incidents[i - 1].0 >= cnode.source_incidents[i].0 {
                    return Err(Error::OtherString(format!(
                        "{p_cnode} {cnode:?}.source_incidents is unsorted or not hereditary"
                    )))
                }
            }
        }
        for (p_cnode, cnode) in &self.cnodes {
            if let Some(p_supernode) = cnode.p_supernode {
                if let Some(supernode) = self.cnodes.get(p_supernode) {
                    if supernode.p_subnodes.binary_search(&p_cnode).is_err() {
                        return Err(Error::OtherString(format!(
                            "{p_cnode} {cnode:?}.p_supernode could not roundtrip"
                        )))
                    }
                }
            }
            // both directions
            for p_subnode in cnode.p_subnodes.iter().copied() {
                if let Some(subnode) = self.cnodes.get(p_subnode) {
                    if subnode.p_supernode != Some(p_cnode) {
                        return Err(Error::OtherString(format!(
                            "{p_cnode} {cnode:?}.p_subnode could not roundtrip"
                        )))
                    }
                }
            }
        }
        for (p_cnode, cnode) in &self.cnodes {
            if let Some(p_sink) = cnode.sink_incident {
                if let Some(cedge) = self.cedges.get(p_sink) {
                    if cedge.sink() != p_cnode {
                        return Err(Error::OtherString(format!(
                            "{p_cnode} {cnode:?}.sink_incident could not roundtrip"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_cnode} {cnode:?}.sink_incident is invalid"
                    )))
                }
            }
            for (p_source, i) in cnode.source_incidents.iter().copied() {
                if let Some(cedge) = self.cedges.get(p_source) {
                    if let Some(source) = cedge.sources().get(i) {
                        if source.p_cnode != p_cnode {
                            return Err(Error::OtherString(format!(
                                "{p_cnode} {cnode:?}.source_incidents[{i}] could not roundtrip"
                            )))
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "{p_cnode} {cnode:?}.source_incidents[{i}] out of range"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_cnode} {cnode:?}.source_incidents[{i}] is invalid"
                    )))
                }
            }
        }
        for (p_cedge, cedge) in &self.cedges {
            for source in cedge.sources().iter().copied() {
                if let Some(cnode) = self.cnodes.get(source.p_cnode) {
                    if binary_search_similar_by(&cnode.source_incidents, |(p_cedge1, _)| {
                        p_cedge1.cmp(&p_cedge)
                    })
                    .1
                    .is_ne()
                    {
                        return Err(Error::OtherString(format!(
                            "{p_cedge} {cedge:?} source {source:?} could not roundtrip"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_cedge} {cedge:?} source {source:?} is invalid",
                    )))
                }
            }
            if let Some(cnode) = self.cnodes.get(cedge.sink()) {
                if cnode.sink_incident != Some(p_cedge) {
                    return Err(Error::OtherString(format!(
                        "{p_cedge} {cedge:?} sink could not roundtrip"
                    )))
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{cedge:?} sink {:?} is invalid",
                    cedge.sink()
                )))
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
                Programmability::ArbitraryLut(arbitrary_lut) => {
                    arbitrary_lut.verify_integrity(sources_len)?;
                    true
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
        // insure `CEdge`s are only between nodes on the same level
        for (p_cedge, cedge) in &self.cedges {
            if cedge.sources().is_empty() {
                return Err(Error::OtherString(format!(
                    "{p_cedge:?} edge has no sources",
                )));
            }
            let mut lvl = None;
            let mut res = Ok(());
            cedge.incidents(|p_cnode| {
                let other_lvl = self.cnodes.get(p_cnode).unwrap().lvl;
                if let Some(lvl) = lvl {
                    if lvl != other_lvl {
                        res = Err(Error::OtherString(format!(
                            "{p_cedge:?} incidents not all on same level",
                        )));
                    }
                } else {
                    lvl = Some(other_lvl);
                }
            });
            res?;
        }
        Ok(())
    }
}
