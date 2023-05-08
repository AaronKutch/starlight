use awint::{awint_dag::smallvec, ExtAwi};
use smallvec::SmallVec;

use crate::triple_arena::ptr_struct;

// UnionArena<PEClass, EClass>
// BTreeMap<ENode, PEClass>

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode);

/// A "table" node meant to evoke some kind of one-way table in a DAG.
#[derive(Debug, Clone)]
pub struct TNode {
    /// Inputs
    pub inp: SmallVec<[PTNode; 4]>,
    /// Outputs, the value of which will all be the same
    pub out: SmallVec<[PTNode; 4]>,
    /// Lookup Table that outputs one bit
    // TODO make a SmallAwi
    pub lut: Option<ExtAwi>,
    /// The value of the output
    pub val: Option<bool>,
    /// If the value cannot be temporally changed with respect to what the
    /// simplification algorithms can assume.
    pub is_permanent: bool,
    /// If the value is temporally driven by a `Loop`
    pub loop_driver: Option<PTNode>,
    /// Used in algorithms
    pub alg_rc: u64,
    /// reference count
    pub rc: u64,
    /// visit number
    pub visit: u64,
}

impl TNode {
    pub fn new(visit: u64) -> Self {
        Self {
            inp: SmallVec::new(),
            out: SmallVec::new(),
            lut: None,
            val: None,
            is_permanent: false,
            loop_driver: None,
            alg_rc: 0,
            rc: 0,
            visit,
        }
    }

    #[must_use]
    pub fn inc_rc(&mut self) -> Option<()> {
        self.rc = self.rc.checked_add(1)?;
        Some(())
    }

    #[must_use]
    pub fn dec_rc(&mut self) -> Option<()> {
        self.rc = self.rc.checked_sub(1)?;
        Some(())
    }

    /// Returns `true` if decremented to zero
    #[must_use]
    pub fn dec_alg_rc(&mut self) -> Option<bool> {
        self.alg_rc = self.alg_rc.checked_sub(1)?;
        Some(self.alg_rc == 0)
    }

    /// Returns the value of this node if it is both non-opaque and permanent
    #[must_use]
    pub fn permanent_val(&self) -> Option<bool> {
        if self.is_permanent {
            self.val
        } else {
            None
        }
    }
}
