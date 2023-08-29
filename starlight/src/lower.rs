use std::{collections::HashMap, num::NonZeroUsize};

use awint::{
    awint_dag::{
        lowering::{OpDag, PNode},
        EvalError,
        Op::*,
    },
    awint_macro_internals::triple_arena::Advancer,
    ExtAwi,
};

use crate::{Note, PTNode, TDag};

impl TDag {
    pub(crate) fn add_op_dag(&mut self, op_dag: &mut OpDag) -> Result<(), EvalError> {
        // TODO private currently because we need to think about how conflicting
        // `PNote`s work, maybe they do need to be external. Perhaps go straight from
        // state to TDag?
        #[cfg(debug_assertions)]
        {
            // this is in case users are triggering problems such as with epochs
            let res = op_dag.verify_integrity();
            if res.is_err() {
                return Err(EvalError::OtherString(format!(
                    "verification error before adding `OpDag` group to `TDag`: {res:?}"
                )))
            }
        }
        self.notes
            .clone_from_with(&op_dag.note_arena, |_, _| Note { bits: vec![] });
        op_dag.visit_gen += 1;
        let gen = op_dag.visit_gen;

        // TODO this is quadratically suboptimal
        // we definitely need a static concat operation
        let mut map = HashMap::<PNode, Vec<PTNode>>::new();
        let mut adv = op_dag.a.advancer();
        while let Some(leaf) = adv.advance(&op_dag.a) {
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
                                v.push(self.make_literal(Some(lit.get(i).unwrap())));
                            }
                            map.insert(p, v);
                        }
                        Opaque(_, name) => {
                            if let Some(name) = name {
                                return Err(EvalError::OtherString(format!(
                                    "cannot lower root opaque with name {name}"
                                )))
                            }
                            let bw = op_dag.get_bw(p).get();
                            let mut v = vec![];
                            for _ in 0..bw {
                                v.push(self.make_literal(None));
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
                                v.push(self.make_copy(*bit).unwrap());
                            }
                            map.insert(p, v);
                        }
                        StaticGet([bits], inx) => {
                            let bit = map[&bits][inx];
                            map.insert(p, vec![self.make_copy(bit).unwrap()]);
                        }
                        StaticSet([bits, bit], inx) => {
                            let bit = &map[&bit];
                            if bit.len() != 1 {
                                return Err(EvalError::OtherStr(
                                    "`StaticSet` has a bit input that is not of bitwidth 1",
                                ))
                            }
                            let bit = bit[0];
                            let bits = &map[&bits];
                            // TODO this is inefficient
                            let mut v = bits.clone();
                            // no need to rekey
                            v[inx] = bit;
                            map.insert(p, v);
                        }
                        StaticLut([inx], ref table) => {
                            let inxs = &map[&inx];
                            let num_entries = 1 << inxs.len();
                            if (table.bw() % num_entries) != 0 {
                                return Err(EvalError::OtherStr(
                                    "`StaticLut` index and table sizes are not correct",
                                ))
                            }
                            let out_bw = table.bw() / num_entries;
                            let mut v = vec![];
                            // convert from multiple out to single out bit lut
                            for i_bit in 0..out_bw {
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
                                v.push(self.make_lut(inxs, &single_bit_table).unwrap());
                            }
                            map.insert(p, v);
                        }
                        Opaque(ref v, name) => {
                            if name == Some("LoopHandle") {
                                if v.len() != 2 {
                                    return Err(EvalError::OtherStr(
                                        "LoopHandle `Opaque` does not have 2 arguments",
                                    ))
                                }
                                let w = map[&v[0]].len();
                                if w != map[&v[1]].len() {
                                    return Err(EvalError::OtherStr(
                                        "LoopHandle `Opaque` has a bitwidth mismatch of looper \
                                         and driver",
                                    ))
                                }
                                // Loops work by an initial `Opaque` that gets registered earlier
                                // and is used by things that use the loop value. A second
                                // LoopHandle Opaque references the first with `p_looper` and
                                // supplies a driver.
                                for i in 0..w {
                                    let p_looper = map[&v[0]][i];
                                    let p_driver = map[&v[1]][i];
                                    self.make_loop(p_looper, p_driver).unwrap();
                                    self.a.get_val_mut(p_looper).unwrap().val = Some(false);
                                }
                                // map the handle to the looper
                                map.insert(p, map[&v[0]].clone());
                            } else if let Some(name) = name {
                                return Err(EvalError::OtherString(format!(
                                    "cannot lower opaque with name {name}"
                                )))
                            } else {
                                return Err(EvalError::OtherStr("cannot lower opaque with no name"))
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
        }
        // handle the noted
        for (p_note, p_node) in &op_dag.note_arena {
            let mut note = vec![];
            for bit in &map[p_node] {
                note.push(self.make_extra_reference(*bit).unwrap());
            }
            self.notes[p_note] = Note { bits: note };
        }
        Ok(())
    }
}
