use std::num::NonZeroUsize;

use rand_xoshiro::{
    rand_core::{RngCore, SeedableRng},
    Xoshiro128StarStar,
};
use starlight::{
    awint::{
        awi,
        awint_dag::{EvalError, Op, OpDag, StateEpoch},
        dag,
    },
    triple_arena::{ptr_struct, Arena},
    PTNode, TDag,
};

#[cfg(debug_assertions)]
const N: (usize, usize) = (30, 100);

#[cfg(not(debug_assertions))]
const N: (usize, usize) = (50, 1000);

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
            lit.rand_(&mut self.rng).unwrap();
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

    pub fn verify_equivalence(&mut self, epoch: &StateEpoch) -> Result<(), EvalError> {
        let (mut op_dag, res) = OpDag::from_epoch(epoch);
        res?;

        // randomly replace literals with opaques, because lower_all_noted can evaluate
        // and simplify
        let mut replacements = vec![];
        let (mut p, mut b) = op_dag.a.first_ptr();
        loop {
            if b {
                break
            }
            if op_dag[p].op.is_literal() {
                if (self.rng.next_u32() & 1) == 0 {
                    if let Op::Literal(lit) = op_dag[p].op.take() {
                        replacements.push((op_dag.note_pnode(p).unwrap(), lit));
                        op_dag[p].op = Op::Opaque(vec![]);
                    } else {
                        unreachable!()
                    }
                } else {
                    op_dag.note_pnode(p).unwrap();
                }
            }
            op_dag.a.next_ptr(&mut p, &mut b);
        }

        op_dag.lower_all().unwrap();

        let (mut t_dag, res) = TDag::<PTNode>::from_op_dag(&mut op_dag);
        res.unwrap();

        t_dag.verify_integrity().unwrap();

        // restore literals and evaluate on both sides

        for (p_note, lit) in replacements.into_iter() {
            let len = t_dag.notes[p_note].bits.len();
            assert_eq!(lit.bw(), len);
            for i in 0..len {
                t_dag.a[t_dag.notes[p_note].bits[i]].val = Some(lit.get(i).unwrap());
            }
            op_dag.pnote_get_mut_node(p_note).unwrap().op = Op::Literal(lit);
        }

        op_dag.eval_all().unwrap();
        t_dag.eval();

        t_dag.verify_integrity().unwrap();

        for (p_note, p_node) in &op_dag.note_arena {
            let op_node = &op_dag[p_node];
            let note = &t_dag.notes[p_note];
            if let Op::Literal(ref lit) = op_node.op {
                let len = note.bits.len();
                assert_eq!(lit.bw(), len);
                for i in 0..len {
                    assert_eq!(t_dag.a[note.bits[i]].val.unwrap(), lit.get(i).unwrap());
                    // check the reference count is 1 or 2
                    let rc = t_dag.a[note.bits[i]].rc;
                    assert!((rc == 1) || (rc == 2));
                }
            } else {
                unreachable!();
            }
        }
        Ok(())
    }

    // TODO better code and execution reuse while still being able to test for one
    // thing at a time

    // FIXME simplifying version
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
                to.copy_(from).unwrap();
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
            m.a[out].lut_(&lut_a, &inx_a).unwrap();
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
        let res = m.verify_equivalence(&epoch);
        res.unwrap();
        // FIXME
        //let res = m.verify_equivalence_basic_simplify(&epoch);
        //res.unwrap();
        drop(epoch);
        m.clear();
    }
}
