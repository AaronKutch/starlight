use awint::awint_dag::triple_arena::ptr_struct;

use super::PBack;

// We use this because our algorithms depend on generation counters
ptr_struct!(PLNode);

/// A temporal loopback node
#[derive(Debug, Clone)]
pub struct LNode {
    pub p_self: PBack,
    pub p_driver: PBack,
}

impl LNode {
    pub fn new(p_self: PBack, p_driver: PBack) -> Self {
        Self { p_self, p_driver }
    }
}
