use std::{
    collections::HashMap,
    num::{NonZeroU64, NonZeroUsize},
};

use awint::{
    awint_dag::{
        lowering::{lower_state, LowerManagement, OpDag, PNode},
        smallvec::SmallVec,
        EvalError, Location,
        Op::{self, *},
        PState,
    },
    awint_macro_internals::triple_arena::Advancer,
    Awi, Bits,
};

use crate::ensemble::{Ensemble, Note, PBack, Value};

/// Represents the state resulting from a mimicking operation
#[derive(Debug, Clone)]
pub struct State {
    pub nzbw: NonZeroUsize,
    /// This either has zero length or has a length equal to `nzbw`
    pub p_self_bits: SmallVec<[PBack; 4]>,
    /// Operation
    pub op: Op<PState>,
    /// Location where this state is derived from
    pub location: Option<Location>,
    /// Used in algorithms for DFS tracking and to allow multiple DAG
    /// constructions from same nodes
    pub visit: NonZeroU64,
}

impl Ensemble {
    /// Used for forbidden meta psuedo-DSL techniques in which a single state is
    /// replaced by more basic states.
    pub fn graft(&mut self, p_state: PState, operands: &[PState]) -> Result<(), EvalError> {
        #[cfg(debug_assertions)]
        {
            if (self.states[p_state].op.operands_len() + 1) != operands.len() {
                return Err(EvalError::WrongNumberOfOperands)
            }
            for (i, op) in self.states[p_state].op.operands().iter().enumerate() {
                let current_nzbw = operands[i + 1].get_nzbw();
                let current_is_opaque = operands[i + 1].get_op().is_opaque();
                if self.states[op].nzbw != current_nzbw {
                    return Err(EvalError::OtherString(format!(
                        "operand {}: a bitwidth of {:?} is trying to be grafted to a bitwidth of \
                         {:?}",
                        i, current_nzbw, self.states[op].nzbw
                    )))
                }
                if !current_is_opaque {
                    return Err(EvalError::ExpectedOpaque)
                }
            }
            if self.states[p_state].nzbw != operands[0].get_nzbw() {
                return Err(EvalError::WrongBitwidth)
            }
        }

        // TODO what do we do when we make multi-output things
        // graft input
        for i in 1..operands.len() {
            let grafted = operands[i];
            let graftee = self.states.get(p_state).unwrap().op.operands()[i - 1];
            if let Some(grafted) = self.states.get_mut(grafted) {
                // change the grafted `Opaque` into a `Copy` that routes to the graftee instead
                // of needing to change all the operands of potentially many nodes
                grafted.op = Copy([graftee]);
            } else {
                // dec graftee rc
            }
        }

        // graft output
        let grafted = operands[0];
        self.states.get_mut(p_state).unwrap().op = Copy([grafted]);
        // dec grafted rc?

        Ok(())
    }

    pub fn lower_state(&mut self, p_state: PState) -> Result<(), EvalError> {
        // TODO optimization to remove unused nodes early
        //let epoch = StateEpoch::new();
        struct Tmp<'a> {
            ptr: PState,
            tdag: &'a mut Ensemble,
        }
        impl<'a> LowerManagement<PState> for Tmp<'a> {
            fn graft(&mut self, operands: &[PState]) {
                self.tdag.graft(self.ptr, operands).unwrap()
            }

            fn get_nzbw(&self, p: PState) -> NonZeroUsize {
                self.tdag.states.get(p).unwrap().nzbw
            }

            fn get_op(&self, p: PState) -> &Op<PState> {
                &self.tdag.states.get(p).unwrap().op
            }

            fn get_op_mut(&mut self, p: PState) -> &mut Op<PState> {
                &mut self.tdag.states.get_mut(p).unwrap().op
            }

            fn lit(&self, p: PState) -> &Bits {
                if let Op::Literal(ref lit) = self.tdag.states.get(p).unwrap().op {
                    lit
                } else {
                    panic!()
                }
            }

            fn usize(&self, p: PState) -> usize {
                if let Op::Literal(ref lit) = self.tdag.states.get(p).unwrap().op {
                    if lit.bw() != 64 {
                        panic!()
                    }
                    lit.to_usize()
                } else {
                    panic!()
                }
            }

            fn bool(&self, p: PState) -> bool {
                if let Op::Literal(ref lit) = self.tdag.states.get(p).unwrap().op {
                    if lit.bw() != 1 {
                        panic!()
                    }
                    lit.to_bool()
                } else {
                    panic!()
                }
            }

            fn dec_rc(&mut self, _p: PState) {
                //
            }
        }
        let state = self.states.get(p_state).unwrap();
        let start_op = state.op.clone();
        let out_w = state.nzbw;
        lower_state(p_state, start_op, out_w, Tmp {
            ptr: p_state,
            tdag: self,
        })?;
        Ok(())
    }

    pub fn lower_state_to_tnodes(&mut self, p_state: PState) -> Result<(), EvalError> {
        //
        Ok(())
    }

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
        op_dag.visit_gen = op_dag.visit_gen.checked_add(1).unwrap();
        let gen = op_dag.visit_gen;

        // TODO this is quadratically suboptimal
        // we definitely need a static concat operation
        let mut map = HashMap::<PNode, Vec<PBack>>::new();
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
                                v.push(*bit);
                            }
                            map.insert(p, v);
                        }
                        StaticGet([bits], inx) => {
                            let bit = map[&bits][inx];
                            map.insert(p, vec![bit]);
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
                                    let mut val =
                                        Awi::zero(NonZeroUsize::new(num_entries).unwrap());
                                    for i in 0..num_entries {
                                        val.set(i, table.get((i * out_bw) + i_bit).unwrap())
                                            .unwrap();
                                    }
                                    val
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
                                    self.make_loop(
                                        p_looper,
                                        p_driver,
                                        Value::Dynam(false, self.visit_gen()),
                                    )
                                    .unwrap();
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
                note.push(self.make_note(p_note, *bit).unwrap());
            }
            self.notes[p_note] = Note { bits: note };
        }
        Ok(())
    }
}
