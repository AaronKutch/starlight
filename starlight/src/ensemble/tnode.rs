use awint::awint_dag::triple_arena::{ptr_struct, Recast, Recaster};

use crate::ensemble::PBack;

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode);

#[derive(Debug, Clone)]
pub struct Delay {
    pub amount: u64,
}

impl Delay {
    pub fn zero() -> Self {
        Self { amount: 0 }
    }

    pub fn from_amount(amount: u64) -> Self {
        Self { amount }
    }

    pub fn is_zero(&self) -> bool {
        self.amount == 0
    }

    pub fn amount(&self) -> u64 {
        self.amount
    }
}

/// A temporal node, currently just used for loopbacks
#[derive(Debug, Clone)]
pub struct TNode {
    pub p_self: PBack,
    pub p_driver: PBack,
    pub delay: Delay,
}

impl Recast<PBack> for TNode {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_self.recast(recaster)?;
        self.p_driver.recast(recaster)
    }
}

impl TNode {
    pub fn new(p_self: PBack, p_driver: PBack, delay: Delay) -> Self {
        Self {
            p_self,
            p_driver,
            delay,
        }
    }

    pub fn delay(&self) -> &Delay {
        &self.delay
    }
}
