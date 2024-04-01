use std::num::NonZeroU64;

use awint::awint_dag::triple_arena::{Arena, OrdArena, Recast, Recaster};

use super::{MapPoint, PMapPointToCnode};
use crate::{
    ensemble::PExternal,
    route::{CNode, PCNode, Programmability},
    Error,
};

/// A channeling graph for a target
#[derive(Debug, Clone)]
pub struct Channeler {
    pub cnodes: Arena<PCNode, CNode>,
    pub map_point_to_p_cnode: OrdArena<PMapPointToCnode, MapPoint, PCNode>,
    // used by algorithms to avoid `OrdArena`s
    pub alg_visit: NonZeroU64,
}

impl Recast<PCNode> for Channeler {
    fn recast<R: Recaster<Item = PCNode>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.cnodes.recast(recaster)
    }
}

impl Channeler {
    pub fn empty() -> Self {
        Self {
            cnodes: Arena::new(),
            map_point_to_p_cnode: OrdArena::new(),
            alg_visit: NonZeroU64::new(2).unwrap(),
        }
    }

    pub fn next_alg_visit(&mut self) -> NonZeroU64 {
        self.alg_visit = self.alg_visit.checked_add(1).unwrap();
        self.alg_visit
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
            for (i, sink) in cnode.sinks.iter().enumerate() {
                if let Some(other) = self.cnodes.get(sink.p_cnode) {
                    if let Some(source) = other.sources.get(sink.source_i) {
                        if source.p_cnode != p_cnode {
                            return Err(Error::OtherString(format!(
                                "{p_cnode} {cnode:?}.sinks[{i}] could not roundtrip"
                            )))
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "{p_cnode} {cnode:?}.sinks[{i}] could not get source_i"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_cnode} {cnode:?}.sinks[{i}].p_cnode is invalid"
                    )))
                }
            }
            for (i, source) in cnode.sources.iter().enumerate() {
                if let Some(other) = self.cnodes.get(source.p_cnode) {
                    if let Some(sink) = other.sinks.get(source.sink_i) {
                        if sink.p_cnode != p_cnode {
                            return Err(Error::OtherString(format!(
                                "{p_cnode} {cnode:?}.sources[{i}] could not roundtrip"
                            )))
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "{p_cnode} {cnode:?}.sources[{i}] could not get sink_i"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_cnode} {cnode:?}.sources[{i}].p_cnode is invalid"
                    )))
                }
            }
        }
        // non `Ptr` validities
        for (p_cnode, cnode) in &self.cnodes {
            let sources_len = cnode.sources().len();
            let ok = match cnode.programmability() {
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
                Programmability::Bulk(_) => true,
            };
            if !ok {
                return Err(Error::OtherString(format!(
                    "{p_cnode} {cnode:?} an invariant is broken"
                )))
            }
        }
        Ok(())
    }
}
