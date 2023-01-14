use std::{collections::HashMap, num::NonZeroUsize};

use awint::{
    awint_dag::{
        lowering::{OpDag, PNode},
        EvalError,
        Op::*,
    },
    ExtAwi,
};
use smallvec::{smallvec, SmallVec};

use crate::{triple_arena::Ptr, Note, TDag, TNode};

impl<PTNode: Ptr> TDag<PTNode> {
    pub(crate) fn add_op_dag(&mut self, op_dag: &mut OpDag) -> Result<(), EvalError> {
        // TODO private currently because we need to think about how conflicting
        // `PNote`s work, maybe they do need to be external. Perhaps go straight from
        // state to TDag?
        self.notes
            .clone_from_with(&op_dag.note_arena, |_, _| Note { bits: vec![] });
        #[cfg(debug_assertions)]
        {
            // this is in case users are triggering problems such as with epochs
            let res = op_dag.verify_integrity();
            if res.is_err() {
                return Err(EvalError::OtherString(format!(
                    "verification error adding `OpDag` group to `TDag`: {res:?}"
                )))
            }
        }
        op_dag.visit_gen += 1;
        let gen = op_dag.visit_gen;
        let mut map = HashMap::<PNode, Vec<PTNode>>::new();
        let (mut leaf, mut b) = op_dag.a.first_ptr();
        loop {
            if b {
                break
            }
            if op_dag[leaf].visit == gen {
                op_dag.a.next_ptr(&mut leaf, &mut b);
                continue
            }
            let mut path: Vec<(usize, PNode)> = vec![(0, leaf)];
            loop {
                let (i, p) = path[path.len() - 1];
                let ops = op_dag[p].op.operands();
                if ops.is_empty() {
                    // reached a root
                    match op_dag[p].op {
                        Literal(ref lit) => {
                            let mut v = vec![];
                            for i in 0..lit.bw() {
                                let mut tnode = TNode::new(0);
                                tnode.val = Some(lit.get(i).unwrap());
                                v.push(self.a.insert(tnode));
                            }
                            map.insert(p, v);
                        }
                        Opaque(_) => {
                            let bw = op_dag.get_bw(p).get();
                            let mut v = vec![];
                            for _ in 0..bw {
                                v.push(self.a.insert(TNode::new(0)));
                            }
                            map.insert(p, v);
                        }
                        ref op => {
                            return Err(EvalError::OtherString(format!("cannot lower {op:?}")))
                        }
                    }
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                    path.last_mut().unwrap().0 += 1;
                } else if i >= ops.len() {
                    // checked all sources
                    match op_dag[p].op {
                        Copy([x]) => {
                            let source_bits = &map[&x];
                            let mut v = vec![];
                            for bit in source_bits {
                                let mut tnode = TNode::new(0);
                                tnode.inp = smallvec!(*bit);
                                let p_new = self.a.insert(tnode);
                                self.a[bit].out.push(p_new);
                                v.push(p_new);
                            }
                            map.insert(p, v);
                        }
                        StaticGet([bits], inx) => {
                            let bit = map[&bits][inx];
                            let mut tnode = TNode::new(0);
                            tnode.inp = smallvec!(bit);
                            let p_new = self.a.insert(tnode);
                            self.a[bit].out.push(p_new);
                            map.insert(p, vec![p_new]);
                        }
                        StaticSet([bits, bit], inx) => {
                            let bit = &map[&bit];
                            assert_eq!(bit.len(), 1);
                            let bit = bit[0];
                            let bits = &map[&bits];
                            // TODO this is inefficient
                            let mut v = bits.clone();
                            v[inx] = bit;
                            map.insert(p, v);
                        }
                        StaticLut([inx], ref table) => {
                            let inxs = &map[&inx];
                            let num_entries = 1 << inxs.len();
                            assert_eq!(table.bw() % num_entries, 0);
                            let out_bw = table.bw() / num_entries;
                            let mut v = vec![];
                            // convert from multiple out to single out bit lut
                            for i_bit in 0..out_bw {
                                let mut tnode = TNode::new(0);
                                tnode.inp = SmallVec::from_slice(inxs);
                                let single_bit_table = if out_bw == 1 {
                                    table.clone()
                                } else {
                                    let mut awi =
                                        ExtAwi::zero(NonZeroUsize::new(num_entries).unwrap());
                                    for i in 0..num_entries {
                                        awi.set(i, table.get((i * out_bw) + i_bit).unwrap())
                                            .unwrap();
                                    }
                                    awi
                                };
                                tnode.lut = Some(single_bit_table);
                                let p_new = self.a.insert(tnode);
                                for inx in inxs {
                                    self.a[inx].out.push(p_new);
                                }
                                v.push(p_new);
                            }
                            map.insert(p, v);
                        }
                        Opaque(ref v) => {
                            if v.len() == 2 {
                                // special case for `Loop`
                                let w = map[&v[0]].len();
                                assert_eq!(w, map[&v[1]].len());
                                for i in 0..w {
                                    let looper = map[&v[0]][i];
                                    let driver = map[&v[1]][i];
                                    // temporal optimizers can subtract one for themselves,
                                    // other optimizers don't have to do extra tracking
                                    self.a[looper].rc += 1;
                                    self.a[looper].val = Some(false);
                                    self.a[looper].loop_driver = Some(driver);
                                    self.a[driver].rc += 1;
                                }
                                // map the handle to the looper
                                map.insert(p, map[&v[0]].clone());
                            } else {
                                return Err(EvalError::OtherStr(
                                    "cannot lower opaque with number of arguments not equal to 0 \
                                     or 2",
                                ))
                            }
                        }
                        ref op => {
                            return Err(EvalError::OtherString(format!("cannot lower {op:?}")))
                        }
                    }
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                } else {
                    let p_next = ops[i];
                    if op_dag[p_next].visit == gen {
                        // do not visit
                        path.last_mut().unwrap().0 += 1;
                    } else {
                        op_dag[p_next].visit = gen;
                        path.push((0, p_next));
                    }
                }
            }
            op_dag.a.next_ptr(&mut leaf, &mut b);
        }
        // handle the noted
        for (p_note, p_node) in &op_dag.note_arena {
            let mut note = vec![];
            for bit in &map[p_node] {
                self.a[bit].inc_rc().unwrap();
                note.push(*bit);
            }
            self.notes[p_note] = Note { bits: note };
        }
        self.mark_nonloop_roots_permanent();
        self.propogate_permanence();
        Ok(())
    }
}
