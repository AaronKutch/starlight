use std::num::NonZeroUsize;

use awint::{
    awi,
    awint_dag::{Dag, EvalError, Lineage, Op, StateEpoch},
    dag,
};
use rand_xoshiro::{
    rand_core::{RngCore, SeedableRng},
    Xoshiro128StarStar,
};
use starlight::PermDag;
use triple_arena::{ptr_struct, Arena};

#[cfg(debug_assertions)]
const N: (usize, usize) = (30, 1000);

#[cfg(not(debug_assertions))]
const N: (usize, usize) = (50, 10000);

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
        for _ in 0..65 {
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
        for _ in 0..65 {
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

    pub fn get_op(&self, inx: P0) -> dag::ExtAwi {
        self.a[inx].clone()
    }

    pub fn verify_equivalence(&mut self) -> Result<(), EvalError> {
        for node in self.a.vals() {
            let (mut op_dag, res) = Dag::new(&[node.state()], &[node.state()]);
            res?;

            let op_dag_ptrs = op_dag.ptrs();
            // randomly replace literals with opaques, because lower_all_noted can evaluate
            // and simplify
            let mut replacements = vec![];
            for p in op_dag_ptrs {
                if op_dag[p].op.is_literal() && ((self.rng.next_u32() & 1) == 0) {
                    if let Op::Literal(lit) = op_dag[p].op.take() {
                        replacements.push((p, lit));
                        op_dag[p].op = Op::Opaque(vec![]);
                    } else {
                        unreachable!()
                    }
                }
            }

            op_dag.lower_all_noted().unwrap();

            for (op_ptr, _) in replacements.iter() {
                op_dag.mark_noted(*op_ptr);
            }

            let (mut perm_dag, res) = PermDag::from_op_dag(&mut op_dag);
            let note_map = res?;

            // restore literals and evaluate on both sides

            for ((op_ptr, lit), note_ptr) in replacements.into_iter().zip(note_map.iter().skip(1)) {
                let len = perm_dag.notes[note_ptr].bits.len();
                assert_eq!(lit.bw(), len);
                for i in 0..len {
                    perm_dag.bits[perm_dag.notes[note_ptr].bits[i]].state =
                        Some(lit.get(i).unwrap());
                }
                op_dag[op_ptr].op = Op::Literal(lit);
            }

            op_dag.eval_all_noted().unwrap();
            perm_dag.eval();
            perm_dag.verify_integrity().unwrap();

            let p_node = op_dag.noted[0].unwrap();
            if let Op::Literal(ref lit) = op_dag[p_node].op {
                let len = perm_dag.notes[note_map[0]].bits.len();
                assert_eq!(lit.bw(), len);
                for i in 0..len {
                    assert_eq!(
                        perm_dag.bits[perm_dag.notes[note_map[0]].bits[i]]
                            .state
                            .unwrap(),
                        lit.get(i).unwrap()
                    );
                }
            } else {
                unreachable!();
            }
        }
        Ok(())
    }
}

fn op_perm_duo(rng: &mut Xoshiro128StarStar, m: &mut Mem) {
    let next_op = rng.next_u32() % 3;
    match next_op {
        // Copy
        0 => {
            let (w, from) = m.next1_5();
            let to = m.next(w);
            if to != from {
                let (to, from) = m.a.get2_mut(to, from).unwrap();
                to.copy_assign(from).unwrap();
            }
        }
        // Get-Set
        1 => {
            let (w0, from) = m.next1_5();
            let (w1, to) = m.next1_5();
            let b = m.a[from].get((rng.next_u32() as usize) % w0).unwrap();
            m.a[to].set((rng.next_u32() as usize) % w1, b).unwrap();
        }
        // Lut
        2 => {
            let (out_w, out) = m.next1_5();
            let (inx_w, inx) = m.next1_5();
            let lut = m.next(out_w * (1 << inx_w));
            let lut_a = m.get_op(lut);
            let inx_a = m.get_op(inx);
            m.a[out].lut_assign(&lut_a, &inx_a).unwrap();
        }
        _ => unreachable!(),
    }
}

#[test]
fn fuzz_lower_and_eval() {
    let mut rng = Xoshiro128StarStar::seed_from_u64(0);
    let mut m = Mem::new();

    for _ in 0..N.1 {
        let epoch = StateEpoch::new();
        for _ in 0..N.0 {
            op_perm_duo(&mut rng, &mut m)
        }
        let res = m.verify_equivalence();
        res.unwrap();
        drop(epoch);
        m.clear();
    }
}
