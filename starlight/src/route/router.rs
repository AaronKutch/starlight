use awint::awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena, Ptr};

use crate::{
    ensemble::{self, Ensemble, PExternal},
    route::{Channeler, HyperPath, PHyperPath},
    triple_arena::Arena,
    Error, EvalAwi, LazyAwi, SuspendedEpoch,
};

ptr_struct!(PBack; PCEdge; QBack; QCEdge);
ptr_struct!(PMapping; PEmbedding);

#[derive(Debug, Clone)]
pub struct Mapping {
    program_p_external: PExternal,
    target_p_external: PExternal,
    target_p_equiv: ensemble::PBack,
    bit_i: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmbeddingKind<PBack: Ptr, PCEdge: Ptr> {
    Edge(PCEdge),
    Node(PBack),
}

#[derive(Debug, Clone, Copy)]
pub struct Embedding<PBack: Ptr, PCEdge: Ptr> {
    /// If it is not possible for the embedding to be another way and have a
    /// valid routing
    absolute: bool,
    target: EmbeddingKind<PBack, PCEdge>,
}

impl<PBack: Ptr, PCEdge: Ptr> Embedding<PBack, PCEdge> {
    pub fn absolute_cnode(target_cnode: PBack) -> Self {
        Self {
            absolute: true,
            target: EmbeddingKind::Node(target_cnode),
        }
    }

    pub fn cnode(target_cnode: PBack) -> Self {
        Self {
            absolute: false,
            target: EmbeddingKind::Node(target_cnode),
        }
    }

    pub fn cedge(target_cedge: PCEdge) -> Self {
        Self {
            absolute: false,
            target: EmbeddingKind::Edge(target_cedge),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Router {
    target_ensemble: Ensemble,
    target_channeler: Channeler<QBack, QCEdge>,
    program_ensemble: Ensemble,
    program_channeler: Channeler<PBack, PCEdge>,
    hyperpaths: Arena<PHyperPath, HyperPath<QBack, QCEdge>>,
    // `ThisEquiv` `PBack` mapping from program to target
    mappings: OrdArena<PMapping, ensemble::PBack, Mapping>,
    embeddings: OrdArena<PEmbedding, EmbeddingKind<PBack, PCEdge>, Embedding<QBack, QCEdge>>,
}

impl Router {
    pub fn new(
        target_epoch: &SuspendedEpoch,
        target_channeler: Channeler<QBack, QCEdge>,
        program_epoch: &SuspendedEpoch,
        program_channeler: Channeler<PBack, PCEdge>,
    ) -> Self {
        // TODO may want the primary user function to take ownership of epoch, or maybe
        // always for memory reasons
        Self {
            target_ensemble: target_epoch.ensemble(|ensemble| ensemble.clone()),
            target_channeler,
            program_ensemble: program_epoch.ensemble(|ensemble| ensemble.clone()),
            program_channeler,
            hyperpaths: Arena::new(),
            mappings: OrdArena::new(),
            embeddings: OrdArena::new(),
        }
    }

    pub fn target_ensemble(&self) -> &Ensemble {
        &self.target_ensemble
    }

    pub fn program_ensemble(&self) -> &Ensemble {
        &self.program_ensemble
    }

    pub fn target_channeler(&self) -> &Channeler<QBack, QCEdge> {
        &self.target_channeler
    }

    pub fn program_channeler(&self) -> &Channeler<PBack, PCEdge> {
        &self.program_channeler
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_rnodes(&mut self, program: PExternal, target: PExternal) -> Result<(), Error> {
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
                            let (_, replaced) = self.mappings.insert(program_p_equiv, Mapping {
                                program_p_external: program,
                                target_p_external: target,
                                target_p_equiv,
                                bit_i,
                            });
                            // we may want to allow this, have some mechanism to be able to
                            // configure multiple to the same thing as long as constraints are
                            // satisfied
                            if replaced.is_some() {
                                todo!()
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
        self.map_rnodes(program.p_external(), target.p_external())
    }

    /// Tell the router what program output bits we want to map to what target
    /// output bits
    pub fn map_eval(&mut self, program: &EvalAwi, target: &EvalAwi) -> Result<(), Error> {
        self.map_rnodes(program.p_external(), target.p_external())
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
        self.target_ensemble.verify_integrity()?;
        self.target_channeler.verify_integrity()?;
        self.program_ensemble.verify_integrity()?;
        self.program_channeler.verify_integrity()?;
        Ok(())
    }

    pub fn route(&mut self) -> Result<(), Error> {
        route(self)
    }
}

fn route(router: &mut Router) -> Result<(), Error> {
    // see cnode.rs for the overall idea

    // initialization of hyperpaths
    assert_eq!(router.target_channeler().top_level_cnodes.len(), 1);
    assert_eq!(router.program_channeler().top_level_cnodes.len(), 1);
    let p = router.target_channeler().top_level_cnodes.min().unwrap();
    let root_level_target_cnode = *router
        .target_channeler()
        .top_level_cnodes
        .get_key(p)
        .unwrap();
    for (_, program_p_equiv, mapping) in &router.mappings {
        let program_p_equiv = *program_p_equiv;
        let target_p_equiv = mapping.target_p_equiv;
        let program_base_cnode = router
            .program_channeler()
            .find_channeler_backref(program_p_equiv)
            .unwrap();
        let target_base_cnode = router
            .target_channeler()
            .find_channeler_backref(target_p_equiv)
            .unwrap();

        let replaced = router
            .embeddings
            .insert(
                EmbeddingKind::Node(program_base_cnode),
                Embedding::absolute_cnode(target_base_cnode),
            )
            .1;
        assert!(replaced.is_none());
    }
    for cnode in router.program_channeler.cnodes.vals() {
        if router
            .embeddings
            .find_key(&EmbeddingKind::Node(cnode.p_this_cnode))
            .is_none()
        {
            // all CNodes that weren't embedded in guaranteed places are now embedded in the
            // root level target node
            let _ = router.embeddings.insert(
                EmbeddingKind::Node(cnode.p_this_cnode),
                Embedding::cnode(root_level_target_cnode),
            );
        }
    }
    // same for edges
    // TODO support custom absolute `CEdge` mappings
    for p_cedge in router.program_channeler.cedges.ptrs() {
        if router
            .embeddings
            .find_key(&EmbeddingKind::Edge(p_cedge))
            .is_none()
        {
            let _ = router.embeddings.insert(
                EmbeddingKind::Edge(p_cedge),
                Embedding::cnode(root_level_target_cnode),
            );
        }
    }

    // property: if a program CNode is embedded in a certain target CNode, the
    // supernodes of the program CNode should be embedded somewhere in the
    // supernode chain of the target CNode including itself. Embeddings should
    // be in a ladder like ordering

    // in order to program a target CEdge, the incidents of a base level program
    // CEdge must be compatible with their embedded incidents in the target.
    // Then the edge is embedded.

    // current idea: orient around embedding the CEdges first. The CEdges enforce
    // CNode embeddings except around permutability. We should focus on the
    // parts that introduce the most constraints first, otherwise conflicts will
    // be too great

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

    Ok(())
}

fn route_step(router: &mut Router) -> Result<bool, Error> {
    // look at all CEdges that aren't embedded to the base level
    let mut adv = router.embeddings.advancer();
    while let Some(p_embedding) = adv.advance(&router.embeddings) {
        let (embedded, embedding) = router.embeddings.get(p_embedding).unwrap();
        match *embedded {
            EmbeddingKind::Edge(p_cedge) => {
                if embedding.absolute {
                    continue
                }
            }
            EmbeddingKind::Node(_) => (),
        }
    }
    Ok(false)
}

/// Given a CEdge that has been embedded, this updates all the embeddings that
/// are implied, also checking for absolute conflicts
fn update_implied_embeddings(router: &mut Router, p_cedge: PCEdge) -> Result<(), Error> {
    let p_embedding = router
        .embeddings
        .find_key(&EmbeddingKind::Edge(p_cedge))
        .unwrap();
    let embedding = router.embeddings.get_val(p_embedding).unwrap();
    match embedding.target {
        EmbeddingKind::Edge(p_cedge) => {
            let cedge = router.target_channeler.cedges.get(p_cedge).unwrap();
            //let p_sink_embedding =
            // router.embeddings.find_key(&EmbeddingKind::Node(cedge.sink())).unwrap();

            if cedge.is_base() {
                // upgraded strictness
                //
            } else {
                todo!()
            }
        }
        EmbeddingKind::Node(p_cnode) => (),
    }
    Ok(())
}
