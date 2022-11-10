use std::{collections::HashMap, num::NonZeroUsize};

use awint::{
    awint_dag::{
        lowering::{Dag, PNode},
        EvalError,
        Op::*,
    },
    ExtAwi,
};
use smallvec::{smallvec, SmallVec};
use triple_arena::{Arena, Ptr};

use crate::{Note, PNote, TDag, TNode};

impl<PTNode: Ptr> TDag<PTNode> {
    /// Constructs a directed acyclic graph of permutations from an
    /// `awint_dag::Dag`. `op_dag.noted` are translated as bits in lsb to msb
    /// order.
    ///
    /// If an error occurs, the DAG (which may be in an unfinished or completely
    /// broken state) is still returned along with the error enum, so that debug
    /// tools like `render_to_svg_file` can be used.
    pub fn from_op_dag(op_dag: &mut Dag) -> (Self, Result<Vec<PNote>, EvalError>) {
        let mut res = Self {
            a: Arena::new(),
            visit_gen: 0,
            notes: Arena::new(),
        };
        let err = res.add_group(op_dag);
        (res, err)
    }

    pub fn add_group(&mut self, op_dag: &mut Dag) -> Result<Vec<PNote>, EvalError> {
        op_dag.visit_gen += 1;
        let gen = op_dag.visit_gen;
        let mut map = HashMap::<PNode, Vec<PTNode>>::new();
        // DFS
        let noted_len = op_dag.noted.len();
        for j in 0..noted_len {
            if let Some(leaf) = op_dag.noted[j] {
                if op_dag[leaf].visit == gen {
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
                                return Err(EvalError::OtherString(format!("cannot lower {:?}", op)))
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
                            ref op => {
                                return Err(EvalError::OtherString(format!("cannot lower {:?}", op)))
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
            }
        }
        let mut note_map = vec![];
        // handle the noted
        for noted in op_dag.noted.iter().flatten() {
            let mut note = vec![];
            for bit in &map[noted] {
                self.a[bit].inc_rc().unwrap();
                note.push(*bit);
            }
            note_map.push(self.notes.insert(Note { bits: note }));
        }
        Ok(note_map)
    }
}
