use awint::ExtAwi;
use smallvec::SmallVec;
use triple_arena::Ptr;

/// A "table" node meant to evoke some kind of one-way table in a DAG.
#[derive(Debug, Clone)]
pub struct TNode<P: Ptr> {
    /// Inputs
    pub inp: SmallVec<[P; 4]>,
    /// Outputs, the value of which will all be the same
    pub out: SmallVec<[P; 4]>,
    /// Lookup Table that outputs one bit
    // TODO make a SmallAwi
    pub lut: Option<ExtAwi>,
    /// The value of the output
    pub val: Option<bool>,
    /// Used in evaluation to check if a lookup table has all needed inputs
    pub inp_rc: u8,
    /// reference count
    pub rc: u64,
    /// visit number
    pub visit: u64,
}

impl<P: Ptr> TNode<P> {
    pub fn new(visit: u64) -> Self {
        Self {
            inp: SmallVec::new(),
            out: SmallVec::new(),
            lut: None,
            val: None,
            inp_rc: 0,
            rc: 0,
            visit,
        }
    }
}
