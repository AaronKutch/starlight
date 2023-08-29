use std::collections::BinaryHeap;

use crate::TDag;

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum SimplifyKind {
    // these fields must occur generally in order of easiest and most affecting to hardest, so
    // that things like removing unused nodes happens before wasting time on the harder
    // optimizations that may be wastes of something that can be handled better by a simpler one
    RemoveUnused,
    ConstPropogate,
}

#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Simplification {
    pub kind: SimplifyKind,
}

/// This struct implements a queue for simple simplifications of `TDag`s
pub struct Simplifier {
    pub gas: u64,
    pub priority_simplifications: BinaryHeap<Simplification>,
    pub t_dag: TDag,
}

impl Simplifier {
    pub fn new(t_dag: TDag, gas: u64) -> Self {
        // TODO get simplifications for all nodes.
        Self {
            gas,
            priority_simplifications: BinaryHeap::new(),
            t_dag,
        }
    }
}
