use std::num::NonZeroUsize;

use starlight::{
    awint::{
        awi,
        awint_dag::{basic_state_epoch::StateEpoch, EvalError, Op, OpDag},
        dag,
    },
    awint_dag::smallvec::smallvec,
    triple_arena::{ptr_struct, Advancer, Arena},
    StarRng, TDag, Value,
};

#[cfg(debug_assertions)]
const N: (usize, usize) = (30, 100);

#[cfg(not(debug_assertions))]
const N: (usize, usize) = (50, 1000);

ptr_struct!(P0);

#[derive(Debug)]
struct Mem {
    a: Arena<P0, dag::Awi>,
    // the outer Vec has 5 vecs for all supported bitwidths plus one dummy 0 bitwidth vec, the
    // inner vecs are unsorted and used for random querying
    v: Vec<Vec<P0>>,
    rng: StarRng,
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
            rng: StarRng::new(0),
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
            let mut lit = awi::Awi::zero(NonZeroUsize::new(w).unwrap());
            lit.rand_(&mut self.rng).unwrap();
            let p = self.a.insert(dag::Awi::from(lit.as_ref()));
            self.v[w].push(p);
            p
        }
    }

    pub fn next1_5(&mut self) -> (usize, P0) {
        let w = ((self.rng.next_u8() as usize) % 4) + 1;
        (w, self.next(w))
    }

    pub fn get_op(&self, inx: P0) -> dag::Awi {
        self.a[inx].clone()
    }

    pub fn verify_equivalence<F: FnMut(&mut TDag)>(
        &mut self,
        mut f: F,
        epoch: &StateEpoch,
    ) -> Result<(), EvalError> {
        let (mut op_dag, res) = OpDag::from_epoch(epoch);
        res?;

        // randomly replace literals with opaques, because lower_all can evaluate
        // and simplify
        let mut replacements = vec![];
        let mut adv = op_dag.a.advancer();
        while let Some(p) = adv.advance(&op_dag.a) {
            if op_dag[p].op.is_literal() {
                if self.rng.next_bool() {
                    if let Op::Literal(lit) = op_dag[p].op.take() {
                        replacements.push((op_dag.note_pnode(p).unwrap(), lit));
                        op_dag[p].op = Op::Opaque(smallvec![], None);
                    } else {
                        unreachable!()
                    }
                } else {
                    op_dag.note_pnode(p).unwrap();
                }
            }
        }

        op_dag.lower_all().unwrap();

        let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
        res.unwrap();

        f(&mut t_dag);

        t_dag.verify_integrity().unwrap();

        // restore literals and evaluate on both sides

        for (p_note, lit) in replacements.into_iter() {
            let len = t_dag.notes[p_note].bits.len();
            assert_eq!(lit.bw(), len);
            for i in 0..len {
                let p_bit = t_dag.notes[p_note].bits[i];
                t_dag.backrefs.get_val_mut(p_bit).unwrap().val = Value::Const(lit.get(i).unwrap());
            }
            op_dag.pnote_get_mut_node(p_note).unwrap().op = Op::Literal(lit);
        }

        op_dag.eval_all().unwrap();
        t_dag.eval_all().unwrap();

        t_dag.verify_integrity().unwrap();

        for (p_note, p_node) in &op_dag.note_arena {
            let op_node = &op_dag[p_node];
            let note = &t_dag.notes[p_note];
            if let Op::Literal(ref lit) = op_node.op {
                let len = note.bits.len();
                assert_eq!(lit.bw(), len);
                for i in 0..len {
                    let p_bit = note.bits[i];
                    let equiv = t_dag.backrefs.get_val(p_bit).unwrap();
                    match equiv.val {
                        Value::Unknown => panic!(),
                        Value::Const(val) => {
                            assert_eq!(val, lit.get(i).unwrap());
                        }
                        Value::Dynam(val, _) => {
                            assert_eq!(val, lit.get(i).unwrap());
                        }
                    }
                }
            } else {
                unreachable!();
            }
        }
        Ok(())
    }
}

fn op_perm_duo(rng: &mut StarRng, m: &mut Mem) {
    let next_op = rng.next_u8() % 3;
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
    let mut rng = StarRng::new(0);
    let mut m = Mem::new();

    for _ in 0..N.1 {
        let epoch = StateEpoch::new();
        for _ in 0..N.0 {
            op_perm_duo(&mut rng, &mut m)
        }
        let res = m.verify_equivalence(|_| {}, &epoch);
        res.unwrap();
        // TODO verify stable optimization
        let res = m.verify_equivalence(|t_dag| t_dag.optimize_basic(), &epoch);
        res.unwrap();
        drop(epoch);
        m.clear();
    }
}

// TODO need a version with loops and random notes
