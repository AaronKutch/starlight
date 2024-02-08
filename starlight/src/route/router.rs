use std::fmt::Write;

use awint::awint_dag::triple_arena::{ptr_struct, OrdArena};

use super::route;
use crate::{
    ensemble::{Ensemble, PBack, PExternal},
    route::{Channeler, EdgeKind, Embedding, EmbeddingKind, PEmbedding},
    triple_arena::Arena,
    Error, EvalAwi, LazyAwi, SuspendedEpoch,
};

ptr_struct!(PCNode; PCEdge; QCNode; QCEdge);
ptr_struct!(PMapping);

#[derive(Debug, Clone)]
pub struct MappingTarget {
    pub target_p_external: PExternal,
    pub target_bit_i: usize,
    pub target_p_equiv: PBack,
}

/// The corresponding program `PBack` is in the key that this `Mapping` should
/// be uniquely associated with.
#[derive(Debug, Clone)]
pub struct Mapping {
    pub program_p_external: PExternal,
    pub program_bit_i: usize,
    // Usually, only one of the following has a single `MappingTarget`, but there are cases like
    // copying a bit that all happens in a single program `CNode`, but needs to be mapped to
    // differing target `CNode`s, so in general it can map to a single target source and multiple
    // target sinks.
    pub target_source: Option<MappingTarget>,
    pub target_sinks: Vec<MappingTarget>,
}

#[derive(Debug, Clone)]
pub struct Router {
    target_ensemble: Ensemble,
    pub(crate) target_channeler: Channeler<QCNode, QCEdge>,
    program_ensemble: Ensemble,
    pub(crate) program_channeler: Channeler<PCNode, PCEdge>,
    // `ThisEquiv` `PBack` mapping from program to target
    pub(crate) mappings: OrdArena<PMapping, PBack, Mapping>,
    // routing embedding of part of the program in the target
    pub(crate) embeddings: Arena<PEmbedding, Embedding<PCNode, PCEdge, QCNode, QCEdge>>,
}

impl Router {
    pub fn new(
        target_epoch: &SuspendedEpoch,
        target_channeler: Channeler<QCNode, QCEdge>,
        program_epoch: &SuspendedEpoch,
        program_channeler: Channeler<PCNode, PCEdge>,
    ) -> Self {
        // TODO may want the primary user function to take ownership of epoch, or maybe
        // always for memory reasons
        Self {
            target_ensemble: target_epoch.ensemble(|ensemble| ensemble.clone()),
            target_channeler,
            program_ensemble: program_epoch.ensemble(|ensemble| ensemble.clone()),
            program_channeler,
            mappings: OrdArena::new(),
            embeddings: Arena::new(),
        }
    }

    pub fn target_ensemble(&self) -> &Ensemble {
        &self.target_ensemble
    }

    pub fn program_ensemble(&self) -> &Ensemble {
        &self.program_ensemble
    }

    pub fn target_channeler(&self) -> &Channeler<QCNode, QCEdge> {
        &self.target_channeler
    }

    pub fn program_channeler(&self) -> &Channeler<PCNode, PCEdge> {
        &self.program_channeler
    }

    pub fn mappings(&self) -> &OrdArena<PMapping, PBack, Mapping> {
        &self.mappings
    }

    pub fn embeddings(&self) -> &Arena<PEmbedding, Embedding<PCNode, PCEdge, QCNode, QCEdge>> {
        &self.embeddings
    }

    fn verify_integrity_of_mapping_target(
        &self,
        mapping_target: &MappingTarget,
    ) -> Result<(), Error> {
        if let Some((_, rnode)) = self
            .target_ensemble
            .notary
            .get_rnode(mapping_target.target_p_external)
        {
            if let Some(bits) = rnode.bits() {
                let mut ok = false;
                if let Some(Some(bit)) = bits.get(mapping_target.target_bit_i) {
                    if *bit == mapping_target.target_p_equiv {
                        ok = true;
                    }
                }
                if !ok {
                    return Err(Error::OtherString(format!(
                        "{mapping_target:?} rnode validity issue"
                    )));
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{mapping_target:?} rnode is unlowered"
                )));
            }
        } else {
            return Err(Error::OtherString(format!(
                "{mapping_target:?}.target_p_external is invalid"
            )))
        }
        Ok(())
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
        // check substituent validities first
        self.target_ensemble.verify_integrity()?;
        self.target_channeler.verify_integrity()?;
        self.program_ensemble.verify_integrity()?;
        self.program_channeler.verify_integrity()?;
        // mapping validities
        for (p_mapping, program_p_equiv, mapping) in self.mappings() {
            if let Some((_, rnode)) = self
                .program_ensemble
                .notary
                .get_rnode(mapping.program_p_external)
            {
                if let Some(bits) = rnode.bits() {
                    let mut ok = false;
                    if let Some(Some(bit)) = bits.get(mapping.program_bit_i) {
                        if *bit == *program_p_equiv {
                            ok = true;
                        }
                    }
                    if !ok {
                        return Err(Error::OtherString(format!(
                            "{p_mapping} {mapping:?} rnode validity issue"
                        )));
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_mapping} {mapping:?} rnode is unlowered"
                    )));
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{p_mapping} {mapping:?}.program_p_external is invalid"
                )))
            }

            if let Some(ref mapping_target) = mapping.target_source {
                self.verify_integrity_of_mapping_target(mapping_target)?;
            }
            for mapping_target in &mapping.target_sinks {
                self.verify_integrity_of_mapping_target(mapping_target)?;
            }
        }
        // embedding validities
        for (p_embedding, embedding) in self.embeddings() {
            match embedding.program {
                EmbeddingKind::Edge(p_cedge) => {
                    if !self.program_channeler().cedges.contains(p_cedge) {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:?}.program is invalid"
                        )))
                    }
                }
                EmbeddingKind::Node(p_cnode) => {
                    if !self.program_channeler().cnodes.contains(p_cnode) {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:?}.program is invalid"
                        )))
                    }
                }
            }
            let hyperpath = &embedding.target_hyperpath;
            if !self.target_channeler().cnodes.contains(hyperpath.source()) {
                return Err(Error::OtherString(format!(
                    "{p_embedding} {embedding:?}.target_hyperpath.source is invalid"
                )))
            }
            for path in hyperpath.paths() {
                if !self.target_channeler().cnodes.contains(path.sink()) {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:?} path sink is invalid"
                    )))
                }
                for edge in path.edges() {
                    if !self.target_channeler().cnodes.contains(edge.to) {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:?} path edge.to is invalid"
                        )))
                    }
                    match edge.kind {
                        EdgeKind::Transverse(q_cedge, source_i) => {
                            if let Some(cedge) = self.target_channeler().cedges.get(q_cedge) {
                                if cedge.sources().get(source_i).is_none() {
                                    return Err(Error::OtherString(format!(
                                        "{p_embedding} {embedding:?} path sink source_i is out of \
                                         range"
                                    )))
                                }
                            } else {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:?} path edge.kind is invalid"
                                )))
                            }
                        }
                        EdgeKind::Concentrate => (),
                        EdgeKind::Dilute => (),
                    }
                }
            }
            // check path continuity
            for (i, path) in hyperpath.paths().iter().enumerate() {
                let mut q = hyperpath.source();
                for (j, edge) in path.edges().iter().enumerate() {
                    match edge.kind {
                        EdgeKind::Transverse(q_cedge, source_i) => {
                            let cedge = self.target_channeler().cedges.get(q_cedge).unwrap();
                            q = cedge.sources()[source_i];
                            if q != edge.to {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:?} path {i} is broken at traversal \
                                     edge {j}"
                                )))
                            }
                        }
                        EdgeKind::Concentrate => {
                            q = self.target_channeler().get_supernode(q).unwrap();
                            if q != edge.to {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:?} path {i} is broken at \
                                     concentration edge {j}"
                                )))
                            }
                        }
                        EdgeKind::Dilute => {
                            let supernode = self.target_channeler().get_supernode(edge.to).unwrap();
                            if q != supernode {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:?} path {i} is broken at dilution \
                                     edge {j}"
                                )))
                            }
                            q = edge.to;
                        }
                    }
                }
                if q != path.sink() {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:?} path {i} ending does not match sink"
                    )))
                }
            }
        }
        Ok(())
    }

    pub fn debug_mapping(&self, p_mapping: PMapping) -> String {
        let (p_back, mapping) = self.mappings().get(p_mapping).unwrap();
        let mut s = String::new();
        let rnode = self
            .program_ensemble()
            .notary
            .get_rnode(mapping.program_p_external)
            .unwrap()
            .1;
        if let Some(location) = rnode.location {
            writeln!(s, "program side of mapping originates from {location:#?}").unwrap();
        }
        if let Some(ref source) = mapping.target_source {
            let rnode = self
                .target_ensemble()
                .notary
                .get_rnode(source.target_p_external)
                .unwrap()
                .1;
            if let Some(location) = rnode.location {
                writeln!(
                    s,
                    "target source side of mapping originates from {location:#?}"
                )
                .unwrap();
            }
        }
        for (i, sink) in mapping.target_sinks.iter().enumerate() {
            let rnode = self
                .target_ensemble()
                .notary
                .get_rnode(sink.target_p_external)
                .unwrap()
                .1;
            if let Some(location) = rnode.location {
                writeln!(
                    s,
                    "target sink {i} side of mapping originates from {location:#?}"
                )
                .unwrap();
            }
        }
        writeln!(
            s,
            "other mapping details: {p_mapping:?} {p_back:?} {mapping:#?}"
        )
        .unwrap();
        s
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_rnodes(
        &mut self,
        program: PExternal,
        target: PExternal,
        is_driver: bool,
    ) -> Result<(), Error> {
        if let Some((_, program_rnode)) = self.program_ensemble.notary.get_rnode(program) {
            let program_rnode_bits = if let Some(bits) = program_rnode.bits() {
                bits
            } else {
                return Err(Error::OtherString(
                    "when mapping bits, found that the program epoch has not been lowered or \
                     preferably optimized"
                        .to_owned(),
                ));
            };
            if let Some((_, target_rnode)) = self.target_ensemble.notary.get_rnode(target) {
                let target_rnode_bits = if let Some(bits) = target_rnode.bits() {
                    bits
                } else {
                    return Err(Error::OtherString(
                        "when mapping bits, found that the target epoch has not been lowered or \
                         preferably optimized"
                            .to_owned(),
                    ));
                };
                let len0 = program_rnode_bits.len();
                let len1 = target_rnode_bits.len();
                if len0 != len1 {
                    return Err(Error::OtherString(format!(
                        "when mapping bits, found that the bitwidths of {program:?} ({len0}) and \
                         {target:?} ({len1}) differ"
                    )));
                }
                for (bit_i, the_two) in program_rnode_bits
                    .iter()
                    .copied()
                    .zip(target_rnode_bits.iter().copied())
                    .enumerate()
                {
                    match the_two {
                        (Some(program_bit), Some(target_bit)) => {
                            let program_p_equiv = self
                                .program_ensemble
                                .backrefs
                                .get_val(program_bit)
                                .unwrap()
                                .p_self_equiv;
                            let target_p_equiv = self
                                .target_ensemble
                                .backrefs
                                .get_val(target_bit)
                                .unwrap()
                                .p_self_equiv;

                            // insert new mapping target
                            let mapping_target = MappingTarget {
                                target_p_external: target,
                                target_bit_i: bit_i,
                                target_p_equiv,
                            };
                            if let Some(p_map) = self.mappings.find_key(&program_p_equiv) {
                                let mapping = self.mappings.get_val_mut(p_map).unwrap();
                                if is_driver {
                                    if mapping.target_source.is_some() {
                                        return Err(Error::OtherString(format!(
                                            "Tried to map multiple program drivers for the same \
                                             program `RNode` {:?}, probably called \
                                             `Router::map_*` twice on the same program `LazyAwi`",
                                            program
                                        )));
                                    }
                                    mapping.target_source = Some(mapping_target);
                                } else {
                                    for target_sink in &mapping.target_sinks {
                                        if target_sink.target_p_equiv
                                            == mapping_target.target_p_equiv
                                        {
                                            return Err(Error::OtherString(format!(
                                                "Tried to map multiple program value sinks for \
                                                 the same program `RNode` {:?}, probably called \
                                                 `Router::map_*` twice on the same program \
                                                 `EvalAwi`",
                                                program
                                            )));
                                        }
                                    }
                                    mapping.target_sinks.push(mapping_target);
                                }
                            } else {
                                let mapping = if is_driver {
                                    Mapping {
                                        program_p_external: program,
                                        program_bit_i: bit_i,
                                        target_source: Some(mapping_target),
                                        target_sinks: vec![],
                                    }
                                } else {
                                    Mapping {
                                        program_p_external: program,
                                        program_bit_i: bit_i,
                                        target_source: None,
                                        target_sinks: vec![mapping_target],
                                    }
                                };
                                let _ = self.mappings.insert(program_p_equiv, mapping);
                            }
                        }
                        (None, None) => (),
                        _ => {
                            // maybe it should just be a no-op? haven't encountered a case yet
                            return Err(Error::OtherString(format!(
                                "when mapping bits {program:?} and {target:?}, one or the other \
                                 bits were optimized away inconsistently"
                            )));
                        }
                    }
                }
                Ok(())
            } else {
                Err(Error::OtherString(format!(
                    "when mapping bits, could not find {target:?} in the target `Ensemble`"
                )))
            }
        } else {
            Err(Error::OtherString(format!(
                "when mapping bits, could not find {program:?} in the program `Ensemble`"
            )))
        }
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_lazy(&mut self, program: &LazyAwi, target: &LazyAwi) -> Result<(), Error> {
        self.map_rnodes(program.p_external(), target.p_external(), true)
    }

    /// Tell the router what program output bits we want to map to what target
    /// output bits
    pub fn map_eval(&mut self, program: &EvalAwi, target: &EvalAwi) -> Result<(), Error> {
        self.map_rnodes(program.p_external(), target.p_external(), false)
    }

    pub fn route(&mut self) -> Result<(), Error> {
        self.initialize_embeddings()?;
        route(self)
    }
}
