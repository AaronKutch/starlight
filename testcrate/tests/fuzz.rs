use std::num::NonZeroUsize;

use awint::{
    awi,
    awint_dag::{Dag, EvalError, Lineage},
    dag,
};
use rand_xoshiro::{
    rand_core::{RngCore, SeedableRng},
    Xoshiro128StarStar,
};
use starlight::{Perm, PermDag};
use triple_arena::{ptr_struct, Arena};

ptr_struct!(P0);

#[derive(Debug)]
struct Mem {
    a: Arena<P0, dag::ExtAwi>,
    // the outer Vec has 5 vecs for all supported bitwidths plus one dummy 0 bitwidth vec, the
    // inner vecs are unsorted and used for random querying
    v: Vec<Vec<P0>>,
    rng: Xoshiro128StarStar,
}

impl Mem {
    pub fn new() -> Self {
        let mut v = vec![];
        for _ in 0..5 {
            v.push(vec![]);
        }
        Self {
            a: Arena::new(),
            v,
            rng: Xoshiro128StarStar::seed_from_u64(0),
        }
    }

    pub fn clear(&mut self) {
        self.a.clear();
        self.v.clear();
        for _ in 0..5 {
            self.v.push(vec![]);
        }
    }

    pub fn next(&mut self, w: usize) -> P0 {
        let try_query = (self.rng.next_u32() % 4) != 0;
        if try_query && (!self.v[w].is_empty()) {
            self.v[w][(self.rng.next_u32() as usize) % self.v[w].len()]
        } else {
            let mut lit = awi::ExtAwi::zero(NonZeroUsize::new(w).unwrap());
            lit.rand_assign_using(&mut self.rng).unwrap();
            let p = self.a.insert(dag::ExtAwi::from(lit.as_ref()));
            self.v[w].push(p);
            p
        }
    }

    pub fn next1_5(&mut self) -> (usize, P0) {
        let w = ((self.rng.next_u32() as usize) % 4) + 1;
        (w, self.next(w))
    }

    pub fn verify_equivalence(&mut self) -> Result<(), EvalError> {
        for node in self.a.vals() {
            let (mut op_dag, res) = Dag::new(&[node.state()], &[node.state()]);
            res?;
            op_dag.lower_all_noted();
            let (mut perm_dag, res) = PermDag::new(&mut op_dag);
            res?;

            op_dag.lower_all_noted();
            //perm_dag.eval_tree();
        }
        Ok(())
    }
}

// FIXME get, set, lut, use awi:: for static
