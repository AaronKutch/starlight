use crate::{
    route::{HyperPath, PHyperPath},
    triple_arena::Arena,
    Epoch,
};

#[derive(Debug, Clone)]
pub struct Router {
    hyperpaths: Arena<PHyperPath, HyperPath>,
}

impl Router {
    pub fn new() -> Self {
        Self {
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

    // TODO are the target and program both on channeling graphs, what assymetries
    // are there?
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}
