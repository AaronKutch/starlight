use awint::awint_dag::triple_arena::{ptr_struct, OrdArena};

use crate::{
    ensemble::{self, Ensemble, PExternal},
    route::{Channeler, HyperPath, PHyperPath},
    triple_arena::Arena,
    EvalAwi, EvalError, LazyAwi, SuspendedEpoch,
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

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_rnodes(&mut self, program: PExternal, target: PExternal) -> Result<(), EvalError> {
        if let Some((_, program_rnode)) = self.program_ensemble.notary.get_rnode(program) {
            if let Some((_, target_rnode)) = self.target_ensemble.notary.get_rnode(target) {
                let len0 = program_rnode.bits.len();
                let len1 = target_rnode.bits.len();
                if len0 != len1 {
                    return Err(EvalError::OtherString(format!(
                        "when mapping bits, found that the bitwidths of {program:?} ({len0}) and \
                         {target:?} ({len1}) differ"
                    )));
                }
                for (bit_i, the_two) in program_rnode
                    .bits
                    .iter()
                    .zip(target_rnode.bits.iter())
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
                            return Err(EvalError::OtherString(format!(
                                "when mapping bits {program:?} and {target:?}, one or the other \
                                 bits were optimized away inconsistently"
                            )));
                        }
                    }
                }
                Ok(())
            } else {
                Err(EvalError::OtherString(format!(
                    "when mapping bits, could not find {target:?} in the target `Ensemble`"
                )))
            }
        } else {
            Err(EvalError::OtherString(format!(
                "when mapping bits, could not find {program:?} in the program `Ensemble`"
            )))
        }
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_lazy(&mut self, program: &LazyAwi, target: &LazyAwi) -> Result<(), EvalError> {
        self.map_rnodes(program.p_external(), target.p_external())
    }

    /// Tell the router what program output bits we want to map to what target
    /// output bits
    pub fn map_eval(&mut self, program: &EvalAwi, target: &EvalAwi) -> Result<(), EvalError> {
        self.map_rnodes(program.p_external(), target.p_external())
    }

    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        self.target_ensemble.verify_integrity()?;
        self.target_channeler.verify_integrity()?;
        self.program_ensemble.verify_integrity()?;
        self.program_channeler.verify_integrity()?;
        Ok(())
    }
}
