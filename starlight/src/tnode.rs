use std::num::NonZeroU64;

use awint::{awint_dag::smallvec, Awi};
use smallvec::SmallVec;

use crate::triple_arena::ptr_struct;

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode; PBack);

/// A "table" node meant to evoke some kind of one-way table in a DAG.
#[derive(Debug, Clone)]
pub struct TNode {
    pub p_self: PBack,
    /// Inputs
    pub inp: SmallVec<[PBack; 4]>,
    /// Lookup Table that outputs one bit
    // TODO make a SmallAwi
    pub lut: Option<Awi>,
    // If the value cannot be temporally changed with respect to what the
    // simplification algorithms can assume.
    //pub is_permanent: bool,
    /// If the value is temporally driven by a `Loop`
    pub loop_driver: Option<PBack>,
    /// Used in algorithms
    pub alg_rc: u64,
    /// visit number
    pub visit: NonZeroU64,
}

impl TNode {
    pub fn new(p_self: PBack) -> Self {
        Self {
            p_self,
            inp: SmallVec::new(),
            lut: None,
            loop_driver: None,
            alg_rc: 0,
            visit: NonZeroU64::new(2).unwrap(),
        }
    }
}
