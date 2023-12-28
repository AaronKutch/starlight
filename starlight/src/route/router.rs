use awint::awint_dag::EvalError;

use crate::{
    ensemble::{self, Ensemble},
    route::{Channeler, HyperPath, PHyperPath},
    triple_arena::Arena,
    Epoch, EvalAwi, LazyAwi, SuspendedEpoch,
};

#[derive(Debug, Clone)]
pub struct Router {
    target_ensemble: Ensemble,
    target_channeler: Channeler,
    program_ensemble: Ensemble,
    program_channeler: Channeler,
    hyperpaths: Arena<PHyperPath, HyperPath>,
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
        }
    }

    /*
    // TODO current plan is to have a corresponding function on the target `Epoch`
    // that calls this. May want some kind of `Epoch` restating system (or use
    // shared `Epoch`s?). The routing info is generated, then one or more other
    // `Epoch`s that have the programs can each have their programs routed.
    pub fn from_epoch(epoch: &Epoch) -> Self {
        let mut res = Self::new();
        res
    }
    */

    /// Tell the router what bits it can use for programming the target
    pub fn map_config(&mut self, config: &LazyAwi) -> Result<(), EvalError> {
        Ok(())
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits
    pub fn map_lazy(&mut self, target: &LazyAwi, program: &LazyAwi) -> Result<(), EvalError> {
        Ok(())
    }

    /// Tell the router what program output bits we want to map to what target
    /// output bits
    pub fn map_eval(&mut self, target: &EvalAwi, program: &EvalAwi) -> Result<(), EvalError> {
        Ok(())
    }
}
