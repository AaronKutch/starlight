use std::{fmt, num::NonZeroU64};

use awint::awint_dag::triple_arena::{Recast, Recaster};

use crate::{
    ensemble::{PBack, Value},
    route::PNodeEmbed,
};

/// A guard around a `PBack` indicating that this uniquely corresponds to a
/// `Referent::ThisEquiv` key
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PEquiv(PBack);

impl PEquiv {
    pub fn into_p_back(self) -> PBack {
        self.0
    }
}

impl From<PEquiv> for PBack {
    fn from(value: PEquiv) -> Self {
        value.0
    }
}

impl fmt::Debug for PEquiv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

impl fmt::Display for PEquiv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, f)
    }
}

#[derive(Debug, Clone)]
pub struct Equiv {
    /// `Ptr` back to this equivalence through a `Referent::ThisEquiv` in the
    /// backref surject associated with this `Equiv`
    pub p_self_equiv: PEquiv,
    /// Output of the equivalence surject
    pub val: Value,
    /// Used by the evaluator
    pub evaluator_partial_order: NonZeroU64,
    /// Algorithm visit number
    pub alg_visit: NonZeroU64,
    pub p_node_embed: Option<PNodeEmbed>,
}

impl Recast<PBack> for Equiv {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_self_equiv.0.recast(recaster)
    }
}

impl Equiv {
    pub fn new(p_self_equiv: PBack, val: Value) -> Self {
        Self {
            p_self_equiv: PEquiv(p_self_equiv),
            val,
            evaluator_partial_order: NonZeroU64::new(1).unwrap(),
            alg_visit: NonZeroU64::new(1).unwrap(),
            p_node_embed: None,
        }
    }
}
