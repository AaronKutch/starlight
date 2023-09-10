use awint::awint_dag::triple_arena::Advancer;

use crate::{
    triple_arena::{ptr_struct, OrdArena},
    PBack, Referent, TDag,
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
    pub p_this: PBack,
}

impl Optimization {
    pub fn unused(p_this: PBack) -> Self {
        Self {
            kind: OptimizeKind::RemoveUnused,
            p_this,
        }
    }
}

/// This struct implements a queue for simple simplifications of `TDag`s
pub struct Optimizer {
    pub gas: u64,
    pub optimizations: OrdArena<POpt, Optimization, ()>,
}

impl Optimizer {
    pub fn new(gas: u64) -> Self {
        // TODO get simplifications for all nodes.
        Self {
            gas,
            optimizations: OrdArena::new(),
        }
    }

    pub fn optimize(&mut self, t_dag: &mut TDag) {
        for equiv in t_dag.tnodes.vals() {
            let mut adv = t_dag.backrefs.advancer_surject(equiv.p_self);
            let mut non_self_rc = 0usize;
            while let Some(p_back) = adv.advance(&t_dag.backrefs) {
                match t_dag.backrefs.get_key(p_back).unwrap() {
                    Referent::ThisEquiv => (),
                    Referent::ThisTNode(_) => (),
                    Referent::Input(_) => non_self_rc += 1,
                    Referent::LoopDriver(p_driver) => {
                        // the way `LoopDriver` networks with no real dependencies will work, is
                        // that const propogation and other simplifications will eventually result
                        // in a single node equivalence that drives itself, which we can remove
                        let p_back_driver = t_dag.tnodes.get(*p_driver).unwrap().p_self;
                        if !t_dag.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
                            non_self_rc += 1;
                        }
                    }
                    Referent::Note(_) => non_self_rc += 1,
                }
            }
            if non_self_rc == 0 {
                let _ = self
                    .optimizations
                    .insert(Optimization::unused(equiv.p_self), ());
            } else {
                todo!()
            }
        }
    }
}
