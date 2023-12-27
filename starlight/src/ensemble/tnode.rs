use awint::awint_dag::triple_arena::{ptr_struct, Recast, Recaster};

use crate::ensemble::PBack;

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode);

/// A temporal node, currently just used for loopbacks
#[derive(Debug, Clone)]
pub struct TNode {
    pub p_self: PBack,
    pub p_driver: PBack,
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
    pub fn new(p_self: PBack, p_driver: PBack) -> Self {
        Self { p_self, p_driver }
    }
}
