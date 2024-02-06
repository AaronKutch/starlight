use std::fmt::Write;

use awint::awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena, Ptr};

use crate::{
    ensemble::{Ensemble, PBack, PExternal},
    route::{Channeler, Edge, EdgeKind, Embedding, HyperPath, PEmbedding, PHyperPath, Path},
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
                    .zip(target_rnode_bits.iter())
                    .enumerate()
                {
                    match the_two {
                        (Some(program_bit), Some(target_bit)) => {
                            let program_p_equiv = self
                                .program_ensemble
                                .backrefs
                                .get_val(*program_bit)
                                .unwrap()
                                .p_self_equiv;
                            let target_p_equiv = self
                                .target_ensemble
                                .backrefs
                                .get_val(*target_bit)
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

    pub fn verify_integrity(&self) -> Result<(), Error> {
        self.target_ensemble.verify_integrity()?;
        self.target_channeler.verify_integrity()?;
        self.program_ensemble.verify_integrity()?;
        self.program_channeler.verify_integrity()?;
        Ok(())
    }

    pub fn route(&mut self) -> Result<(), Error> {
        self.initialize_embeddings()
    }
}

fn route(router: &mut Router) -> Result<(), Error> {
    if router.mappings.is_empty() {
        // nothing to route
        return Ok(())
    }
    // see cnode.rs for the overall idea

    // property: if a program CNode is embedded in a certain target CNode, the
    // supernodes of the program CNode should be embedded somewhere in the
    // supernode chain of the target CNode including itself. Node and edge
    // embeddings should be in a ladder like ordering

    // Observe what happens if we are programming some routing fabric with the
    // simplest possible program, a bit copy, and the input and output are `CNode`
    // mapped to nodes on opposite sides of the fabric. The program hierarchy will
    // consist of a base source cnode, a base sink cnode, a

    // Embed all supernodes of the absolute embeddings in the common `CNode`, and
    // make the paths between them all

    // TODO ?

    // in order to program a target CEdge, the incidents of a base level program
    // CEdge must be compatible with their embedded incidents in the target.
    // Only then is it known to be possible to embed an edge (for bulk edges the
    // substructure might not allow it when we then try to dilute, the only thing we
    // can tell for sure is that a given embedding is not possible if the incidents
    // are not compatible).

    // If a program `CEdge` currently has all of its incidents already embedded, it
    // should be embedded now and conflicts resolved (requiring in general that
    // dilution happen until the `CEdge` and its incidents are embedded together in
    // one target `CNode`). We started with embedding some `CNode`s only because we
    // knew they were absolutely required, but after this we want to orient mainly
    // around embedding `CEdge`s, because they introduce the most constraints first
    // and should be resolved first.

    // TODO

    // Way of viewing hyperpaths: the paths can be ordered in order of which ones
    // stay on the same path the longest. The first and second path stay together
    // the longest before diverging, then the third diverges earlier, etc.
    // A straightforward optimization then is to start from any endpoint and see if
    // there is a shorter overall path to another, rebasing the divergence at that
    // point. If it was close to breakeven by some measure, then do a finding
    // triangle median like thing where different points in the triangle are
    // branched off from, then finding a center close to those.
    // With the hierarchy, we can try a new kind of hyperpath finding that is
    // perhaps based purely on finding the immediate best local routing in each
    // dilution.

    // Note: I suspect we need 4 "colors" of Lagrangian pressure in order to do a
    // constraint violation cleanup
    /*
        return Ok(());

        let mut gas = 100u64;
        loop {
            if route_step(router)? {
                break
            }
            gas = gas.saturating_sub(1);
            if gas == 0 {
                return Err(Error::OtherStr("ran out of gas while routing"));
            }
        }
    */
    Ok(())
}

/*
fn route_step(router: &mut Router) -> Result<bool, Error> {
    // look at all CEdges that aren't embedded to the base level
    let mut adv = router.embeddings.advancer();
    while let Some(p_embedding) = adv.advance(&router.embeddings) {
        let embedding = router.embeddings.get(p_embedding).unwrap();
        match embedding.program {
            EmbeddingKind::Edge(p_cedge) => {
                let edge = router.program_channeler().cedges.get(p_cedge).unwrap();
                if edge.is_base() {
                    continue
                }
                //edge.
            }
            EmbeddingKind::Node(_) => (),
        }
    }
    Ok(false)
}
*/
