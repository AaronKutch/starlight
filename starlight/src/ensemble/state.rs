use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{
        lowering::{lower_state, LowerManagement},
        smallvec::SmallVec,
        EvalError, Location,
        Op::{self, *},
        PState,
    },
    Bits,
};

use super::Value;
use crate::{
    awi,
    ensemble::{Ensemble, PBack},
};

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
    pub lower_visit: NonZeroU64,
    pub keep: bool,
    pub lowered: bool,
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
            ensemble: &'a mut Ensemble,
        }
        impl<'a> LowerManagement<PState> for Tmp<'a> {
            fn graft(&mut self, operands: &[PState]) {
                self.ensemble.graft(self.ptr, operands).unwrap()
            }

            fn get_nzbw(&self, p: PState) -> NonZeroUsize {
                self.ensemble.states.get(p).unwrap().nzbw
            }

            fn get_op(&self, p: PState) -> &Op<PState> {
                &self.ensemble.states.get(p).unwrap().op
            }

            fn get_op_mut(&mut self, p: PState) -> &mut Op<PState> {
                &mut self.ensemble.states.get_mut(p).unwrap().op
            }

            fn lit(&self, p: PState) -> &Bits {
                if let Op::Literal(ref lit) = self.ensemble.states.get(p).unwrap().op {
                    lit
                } else {
                    panic!()
                }
            }

            fn usize(&self, p: PState) -> usize {
                if let Op::Literal(ref lit) = self.ensemble.states.get(p).unwrap().op {
                    if lit.bw() != 64 {
                        panic!()
                    }
                    lit.to_usize()
                } else {
                    panic!()
                }
            }

            fn bool(&self, p: PState) -> bool {
                if let Op::Literal(ref lit) = self.ensemble.states.get(p).unwrap().op {
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
            ensemble: self,
        })?;
        Ok(())
    }

    /// Lowers the rootward tree from `p_state` down to `TNode`s
    pub fn dfs_lower(&mut self, p_state: PState) -> Result<(), EvalError> {
        self.dfs_lower_states_to_elementary(p_state).unwrap();
        self.dfs_lower_elementary_to_tnodes(p_state).unwrap();
        Ok(())
    }

    /// Lowers the rootward tree from `p_state` down to the elementary `Op`s
    pub fn dfs_lower_states_to_elementary(&mut self, p_state: PState) -> Result<(), EvalError> {
        let mut state_list = vec![p_state];
        let visit = NonZeroU64::new(self.lower_visit.get().checked_add(1).unwrap()).unwrap();
        self.lower_visit = visit;
        while let Some(leaf) = state_list.pop() {
            if self.states[leaf].lower_visit == visit {
                continue
            }
            let mut path: Vec<(usize, PState)> = vec![(0, leaf)];
            loop {
                let (i, p_state) = path[path.len() - 1];
                let state = &self.states[p_state];
                let nzbw = state.nzbw;
                let ops = state.op.operands();
                if ops.is_empty() {
                    // reached a root
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                    path.last_mut().unwrap().0 += 1;
                } else if i >= ops.len() {
                    // checked all sources
                    match self.states[p_state].op {
                        Copy(_) | StaticGet(..) | StaticSet(..) | StaticLut(..) | Opaque(..) => (),
                        ref op => {
                            self.lower_state(p_state).unwrap();
                        }
                    }
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                } else {
                    let p_next = ops[i];
                    if self.states[p_next].lower_visit == visit {
                        // do not visit
                        path.last_mut().unwrap().0 += 1;
                    } else {
                        self.states[p_next].lower_visit = visit;
                        path.push((0, p_next));
                    }
                }
            }
        }
        Ok(())
    }

    /// Assuming that the rootward tree from `p_state` is lowered down to the
    /// elementary `Op`s, this will create the `TNode` network
    pub fn dfs_lower_elementary_to_tnodes(&mut self, p_state: PState) -> Result<(), EvalError> {
        let mut state_list = vec![p_state];
        let visit = NonZeroU64::new(self.lower_visit.get().checked_add(1).unwrap()).unwrap();
        self.lower_visit = visit;
        while let Some(leaf) = state_list.pop() {
            if self.states[leaf].lower_visit == visit {
                continue
            }
            let mut path: Vec<(usize, PState)> = vec![(0, leaf)];
            loop {
                let (i, p_state) = path[path.len() - 1];
                let state = &self.states[p_state];
                let nzbw = state.nzbw;
                let ops = state.op.operands();
                if ops.is_empty() {
                    // reached a root
                    match self.states[p_state].op {
                        Literal(ref lit) => {
                            assert_eq!(lit.nzbw(), nzbw);
                            self.initialize_state_bits_if_needed(p_state);
                        }
                        Opaque(_, name) => {
                            if let Some(name) = name {
                                return Err(EvalError::OtherString(format!(
                                    "cannot lower root opaque with name {name}"
                                )))
                            }
                            self.initialize_state_bits_if_needed(p_state);
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
                    match self.states[p_state].op {
                        Copy([x]) => {
                            // this is the only foolproof way of doing this, at least without more
                            // branches
                            self.initialize_state_bits_if_needed(p_state);
                            let len = self.states[p_state].p_self_bits.len();
                            assert_eq!(len, self.states[x].p_self_bits.len());
                            for i in 0..len {
                                let p_equiv0 = self.states[p_state].p_self_bits[i];
                                let p_equiv1 = self.states[x].p_self_bits[i];
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            }
                        }
                        StaticGet([bits], inx) => {
                            self.initialize_state_bits_if_needed(p_state);
                            let p_self_bits = &self.states[p_state].p_self_bits;
                            assert_eq!(p_self_bits.len(), 1);
                            let p_equiv0 = p_self_bits[0];
                            let p_equiv1 = self.states[bits].p_self_bits[inx];
                            self.union_equiv(p_equiv0, p_equiv1).unwrap();
                        }
                        StaticSet([bits, bit], inx) => {
                            self.initialize_state_bits_if_needed(p_state);
                            let len = self.states[p_state].p_self_bits.len();
                            assert_eq!(len, self.states[bits].p_self_bits.len());
                            for i in 0..len {
                                let p_equiv0 = self.states[p_state].p_self_bits[i];
                                let p_equiv1 = self.states[bits].p_self_bits[i];
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            }
                            let p_self_bits = &self.states[bit].p_self_bits;
                            assert_eq!(p_self_bits.len(), 1);
                            let p_equiv0 = p_self_bits[0];
                            let p_equiv1 = self.states[p_state].p_self_bits[inx];
                            self.union_equiv(p_equiv0, p_equiv1).unwrap();
                        }
                        StaticLut([inx], ref table) => {
                            let table = table.clone();
                            self.initialize_state_bits_if_needed(p_state);
                            let inx_bits = self.states[inx].p_self_bits.clone();
                            let inx_len = inx_bits.len();
                            let out_bw = self.states[p_state].p_self_bits.len();
                            let num_entries = 1 << inx_len;
                            assert_eq!(out_bw * num_entries, table.bw());
                            // convert from multiple out to single out bit lut
                            for bit_i in 0..out_bw {
                                let single_bit_table = if out_bw == 1 {
                                    table.clone()
                                } else {
                                    let mut val =
                                        awi::Awi::zero(NonZeroUsize::new(num_entries).unwrap());
                                    for i in 0..num_entries {
                                        val.set(i, table.get((i * out_bw) + bit_i).unwrap())
                                            .unwrap();
                                    }
                                    val
                                };
                                let p_equiv0 = self.make_lut(&inx_bits, &single_bit_table).unwrap();
                                let p_equiv1 = self.states[p_state].p_self_bits[bit_i];
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            }
                        }
                        Opaque(ref v, name) => {
                            if name == Some("LoopHandle") {
                                if v.len() != 2 {
                                    return Err(EvalError::OtherStr(
                                        "LoopHandle `Opaque` does not have 2 arguments",
                                    ))
                                }
                                let v0 = v[0];
                                let v1 = v[1];
                                let w = self.states[v0].p_self_bits.len();
                                if w != self.states[v1].p_self_bits.len() {
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
                                    let p_looper = self.states[v0].p_self_bits[i];
                                    let p_driver = self.states[v1].p_self_bits[i];
                                    self.make_loop(p_looper, p_driver, Value::Dynam(false))
                                        .unwrap();
                                }
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
                    if self.states[p_next].lower_visit == visit {
                        // do not visit
                        path.last_mut().unwrap().0 += 1;
                    } else {
                        self.states[p_next].lower_visit = visit;
                        path.push((0, p_next));
                    }
                }
            }
        }
        Ok(())
    }
}
