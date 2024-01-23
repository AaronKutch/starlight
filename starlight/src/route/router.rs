use awint::awint_dag::triple_arena::{ptr_struct, OrdArena};

use crate::{
    ensemble::{self, Ensemble, PExternal},
    route::{Channeler, HyperPath, PBack, PCEdge, PHyperPath},
    triple_arena::Arena,
    Error, EvalAwi, LazyAwi, SuspendedEpoch,
};

ptr_struct!(PMapping);

#[derive(Debug, Clone)]
pub struct Mapping {
    program_p_external: PExternal,
    target_p_external: PExternal,
    target_p_equiv: ensemble::PBack,
    bit_i: usize,
}

#[derive(Debug, Clone)]
pub struct Router {
    target_ensemble: Ensemble,
    target_channeler: Channeler,
    program_ensemble: Ensemble,
    program_channeler: Channeler,
    hyperpaths: Arena<PHyperPath, HyperPath>,
    // `ThisEquiv` `PBack` mapping from program to target
    mappings: OrdArena<PMapping, ensemble::PBack, Mapping>,
}

impl Router {
    pub fn new(
        target_epoch: &SuspendedEpoch,
        target_channeler: Channeler,
        program_epoch: &SuspendedEpoch,
        program_channeler: Channeler,
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
        }
    }

    pub fn target_ensemble(&self) -> &Ensemble {
        &self.target_ensemble
    }

    pub fn program_ensemble(&self) -> &Ensemble {
        &self.program_ensemble
    }

    pub fn target_channeler(&self) -> &Channeler {
        &self.target_channeler
    }

    pub fn program_channeler(&self) -> &Channeler {
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
    ptr_struct!(P0; P1);
    let p = router.target_channeler().top_level_cnodes.min().unwrap();
    let root_level_target_cnode = *router
        .target_channeler()
        .top_level_cnodes
        .get_key(p)
        .unwrap();
    let mut cnode_embeddings = OrdArena::<P0, PBack, PBack>::new();
    let mut cedge_embeddings = OrdArena::<P1, PCEdge, PCEdge>::new();
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

        let replaced = cnode_embeddings
            .insert(program_base_cnode, target_base_cnode)
            .1;
        assert!(replaced.is_none());
    }
    for cnode in router.program_channeler().cnodes.vals() {
        if cnode_embeddings.find_key(&cnode.p_this_cnode).is_none() {
            // all CNodes that weren't embedded in guaranteed places are now embedded in the
            // root level target node
            cnode_embeddings.insert(cnode.p_this_cnode, root_level_target_cnode);
        }
    }

    // property: if a program CNode is embedded in a certain target CNode, the
    // supernodes of the program CNode should be embedded somewhere in the
    // supernode chain of the target CNode including itself. Embeddings should
    // be in a ladder like ordering

    // in order to program a target CEdge, the incidents of a base level program
    // CEdge must be compatible with their embedded incidents in the target.
    // Then the edge is embedded.

    Ok(())
}
