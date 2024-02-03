use awint::awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena, Ptr};

use crate::{
    ensemble::{Ensemble, PBack, PExternal},
    route::{Channeler, Edge, HyperPath, PHyperPath, Path},
    triple_arena::Arena,
    Error, EvalAwi, LazyAwi, SuspendedEpoch,
};

ptr_struct!(PCNode; PCEdge; QCNode; QCEdge);
ptr_struct!(PMapping; PEmbedding);

#[derive(Debug, Clone)]
pub struct MappingTarget {
    target_p_external: PExternal,
    target_bit_i: usize,
    target_p_equiv: PBack,
}

/// The corresponding program `PBack` is in the key that this `Mapping` should
/// be uniquely associated with.
#[derive(Debug, Clone)]
pub struct Mapping {
    program_p_external: PExternal,
    program_bit_i: usize,
    // Usually, only one of the following has a single `MappingTarget`, but there are cases like
    // copying a bit that all happens in a single program `CNode`, but needs to be mapped to
    // differing target `CNode`s, so in general it can map to a single target source and multiple
    // target sinks.
    target_source: Option<MappingTarget>,
    target_sinks: Vec<MappingTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmbeddingKind<PCNode: Ptr, PCEdge: Ptr> {
    Edge(PCEdge),
    Node(PCNode),
}

#[derive(Debug, Clone)]
pub struct Embedding<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    program: EmbeddingKind<PCNode, PCEdge>,
    target_hyperpath: HyperPath<QCNode, QCEdge>,
}

#[derive(Debug, Clone)]
pub struct Router {
    target_ensemble: Ensemble,
    target_channeler: Channeler<QCNode, QCEdge>,
    program_ensemble: Ensemble,
    program_channeler: Channeler<PCNode, PCEdge>,
    // `ThisEquiv` `PBack` mapping from program to target
    mappings: OrdArena<PMapping, PBack, Mapping>,
    // routing embedding of part of the program in the target
    embeddings: Arena<PEmbedding, Embedding<PCNode, PCEdge, QCNode, QCEdge>>,
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

    /// Given the completed `Embedding`, sets up the embedding edges
    /// automatically
    fn make_embedding0(
        &mut self,
        embedding: Embedding<PCNode, PCEdge, QCNode, QCEdge>,
    ) -> Result<PEmbedding, Error> {
        let program = embedding.program;
        let p_embedding = self.embeddings.insert(embedding);

        // NOTE: for now, we only put in a reference for an embedding into the program
        // channeler side and only allow at most one embedding per program `CNode`. If
        // we keep it this way then it should use an option, I suspect we may want to
        // register on both sides which will require a set for the target side.
        match program {
            EmbeddingKind::Edge(p_cedge) => {
                let embeddings = &mut self
                    .program_channeler
                    .cedges
                    .get_mut(p_cedge)
                    .unwrap()
                    .embeddings;
                if !embeddings.is_empty() {
                    return Err(Error::OtherString(format!(
                        "program edge {p_cedge:?} is already associated with an embedding"
                    )));
                }
                embeddings.insert(p_embedding);
            }
            EmbeddingKind::Node(p_cnode) => {
                let embeddings = &mut self
                    .program_channeler
                    .cnodes
                    .get_val_mut(p_cnode)
                    .unwrap()
                    .embeddings;
                if !embeddings.is_empty() {
                    return Err(Error::OtherString(format!(
                        "program node {p_cnode:?} is already associated with an embedding"
                    )));
                }
                embeddings.insert(p_embedding);
            }
        }
        Ok(p_embedding)
    }

    /// Makes a minimal embedding to express the given mapping.
    /// `common_target_root` needs to be the common supernode of all the nodes
    /// that can interact with this mapping
    fn make_embedding1(
        &mut self,
        /* common_target_root: PCNode, */ p_mapping: PMapping,
    ) -> Result<(), Error> {
        let (program_p_equiv, mapping) = self.mappings.get(p_mapping).unwrap();
        let program_p_equiv = *program_p_equiv;
        let program_cnode = self
            .program_channeler()
            .find_channeler_cnode(program_p_equiv)
            .unwrap();

        if mapping.target_source.is_some() && (!mapping.target_sinks.is_empty()) {
            // If a mapping has both a source and sinks, then we need an embedding of the
            // program cnode that embeds in a target cnode that can cover all the sources
            // and the sinks. The embedding then has a hyperpath that connects the sources
            // and sinks.

            // we are dealing with the single program node copying mapping case, which does
            // not interact with anything else directly so we only deal with the common
            // supernode of our source and sinks

            // find the corresponding `QCNode` for the source
            let target_source_p_equiv = mapping.target_source.as_ref().unwrap().target_p_equiv;
            let target_source_q_cnode = self
                .target_channeler()
                .find_channeler_cnode(target_source_p_equiv)
                .unwrap();

            // begin constructing hyperpath for the embedding
            let mut hyperpath = HyperPath::<QCNode, QCEdge>::new(target_source_q_cnode);

            // begin finding the common target cnode
            let mut root_common_target_q_cnode = target_source_q_cnode;

            // do the same for the sinks
            for mapping_target in &mapping.target_sinks {
                let target_sink_p_equiv = mapping_target.target_p_equiv;
                let target_sink_q_cnode = self
                    .target_channeler()
                    .find_channeler_cnode(target_sink_p_equiv)
                    .unwrap();
                let path = Path::<QCNode, QCEdge>::new(target_sink_q_cnode);
                hyperpath.push(path);
                root_common_target_q_cnode = self
                    .target_channeler()
                    .find_common_supernode(root_common_target_q_cnode, target_sink_q_cnode)
                    .unwrap();
            }

            self.make_embedding0(Embedding {
                program: EmbeddingKind::Node(program_cnode),
                target_hyperpath: hyperpath,
            })
            .unwrap();
        } else {
            // If the mapping has just a source, then a hyper path needs to go concentrating
            // to the common root node. If the mapping just has sinks, then a hyper path
            // needs to go from the common root node diluting to the sinks.
            todo!()
        }

        // TODO support custom `CEdge` mappings

        Ok(())
    }

    pub fn route(&mut self) -> Result<(), Error> {
        route(self)
    }
}

fn route(router: &mut Router) -> Result<(), Error> {
    if router.mappings.is_empty() {
        // nothing to route
        return Ok(())
    }
    // see cnode.rs for the overall idea

    // Mappings will stay static because they are used for figuring out translating
    // program IO to target IO. Embeddings will represent bulk programmings of the
    // hierarchy. However, we know that the mappings correspond to some embeddings
    // that are absolutely necessary for the routing to be possible, so we can start
    // by making those embeddings.
    let mut adv = router.mappings.advancer();
    while let Some(p_mapping) = adv.advance(&router.mappings) {
        router.make_embedding1(p_mapping).unwrap()
    }

    // TODO just complete the hyperpaths

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

/// Given a CEdge that has been embedded, this updates all the embeddings that
/// are implied, also checking for absolute conflicts
fn update_implied_embeddings(_router: &mut Router, _p_cedge: PCEdge) -> Result<(), Error> {
    /*let p_embedding =
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
    }*/
    Ok(())
}
