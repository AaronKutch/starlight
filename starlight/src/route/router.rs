use awint::awint_dag::{
    triple_arena::{ptr_struct, ArenaTrait, OrdArena, Ptr},
    EvalError,
};

use crate::{
    ensemble::{self, Ensemble, PExternal, PRNode, Value},
    route::{Channeler, HyperPath, PHyperPath},
    triple_arena::Arena,
    Epoch, EvalAwi, LazyAwi, SuspendedEpoch,
};

ptr_struct!(PConfig; PMapping);

#[derive(Debug, Clone)]
pub struct Configuration {
    /// stable `Ptr` for the target
    p_external: PExternal,
    /// The index in the `RNode`
    bit_i: usize,
    /// The bit value the configuration wants. `None` is for not yet determined
    /// or for if the value can be set to `Value::Unknown`.
    value: Option<bool>,
}

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
    // `ThisEquiv` `PBack` to `PExternal` mapping for bits we are allowed to configure
    configurations: OrdArena<PConfig, ensemble::PBack, Configuration>,
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
            configurations: OrdArena::new(),
            mappings: OrdArena::new(),
        }
    }

    /// Tell the router what bits it can use for programming the target
    pub fn map_config(&mut self, config: &LazyAwi) -> Result<(), EvalError> {
        let p_external = config.p_external();
        if let Some((p_rnode, rnode)) = self.target_ensemble.notary.get_rnode(p_external) {
            for (bit_i, bit) in rnode.bits.iter().enumerate() {
                if let Some(bit) = bit {
                    let (_, replaced) = self.configurations.insert(*bit, Configuration {
                        p_external,
                        bit_i,
                        value: None,
                    });
                    // we may want to allow this, have some mechanism to be able to configure
                    // multiple to the same thing
                    if replaced.is_some() {
                        return Err(EvalError::OtherString(format!(
                            "when mapping {config:?} as configurable, found that the same bit as \
                             a previous one is mapped, this may be because it was mapped twice or \
                             the bit is equivalent to another somehow"
                        )));
                    }
                }
            }
            Ok(())
        } else {
            Err(EvalError::OtherString(format!(
                "when mapping configurable bits, could not find {config:?} in the target \
                 `Ensemble`"
            )))
        }
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_rnodes(&mut self, program: PExternal, target: PExternal) -> Result<(), EvalError> {
        if let Some((program_p_rnode, program_rnode)) =
            self.program_ensemble.notary.get_rnode(program)
        {
            if let Some((target_p_rnode, target_rnode)) =
                self.target_ensemble.notary.get_rnode(target)
            {
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
                            let (_, replaced) = self.mappings.insert(*program_bit, Mapping {
                                program_p_external: program,
                                target_p_external: target,
                                target_p_equiv: *target_bit,
                                bit_i,
                            });
                            // we may want to allow this, have some mechanism to be able to
                            // configure multiple to the same thing
                            if replaced.is_some() {
                                todo!()
                            }
                        }
                        _ => {
                            // maybe it should just be a no-op? haven't encountered a case yet
                            return Err(EvalError::OtherString(format!(
                                "when mapping bits, one or the other bits were optimized away \
                                 inconsistently"
                            )));
                        }
                        (None, None) => (),
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
}
