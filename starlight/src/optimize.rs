use crate::{
    triple_arena::{ptr_struct, OrdArena},
    TDag,
};

ptr_struct!(POpt);

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptimizeKind {
    // these fields must occur generally in order of easiest and most affecting to hardest, so
    // that things like removing unused nodes happens before wasting time on the harder
    // optimizations that may be wastes of something that can be handled better by a simpler one
    RemoveUnused,
    ConstPropogate,
    // the default state that nodes start with or are set to after being modified
    Investigate,
}

#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Optimization {
    pub kind: OptimizeKind,
}

/// This struct implements a queue for simple simplifications of `TDag`s
pub struct Optimizer {
    pub gas: u64,
    pub optimizations: OrdArena<POpt, Optimization, ()>,
    pub t_dag: TDag,
}

impl Optimizer {
    pub fn new(t_dag: TDag, gas: u64) -> Self {
        // TODO get simplifications for all nodes.
        Self {
            gas,
            optimizations: OrdArena::new(),
            t_dag,
        }
    }
}

// SurjectArena<PBack, PTNode, ()>
// SurjectArena<PTNode, TNode, Equiv>

// do we need the `OrdArena`?
// OrdArena<P, TNode, PTNode>
