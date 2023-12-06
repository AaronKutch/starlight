use std::{fmt::Write, num::NonZeroUsize};

use awint::awint_dag::{
    smallvec::{smallvec, SmallVec},
    triple_arena::{Advancer, Arena},
    EAwi, EvalError, EvalResult, Location,
    Op::{self, *},
    PState,
};

use crate::{
    awi,
    ensemble::{
        value::{Change, Eval},
        Ensemble, PBack, Referent, Value,
    },
    epoch::EpochShared,
};

/// Represents a single state that `awint_dag::mimick::Bits` is in at one point
/// in evaluation. The operands point to other `State`s. `Bits` and `*Awi` use
/// `Ptr`s to `States` in a thread local arena, so that they can change their
/// state without borrowing issues or mutating `States` (which could be used as
/// operands by other `States` and in `Copy` types).
#[derive(Debug, Clone)]
pub struct State {
    pub nzbw: NonZeroUsize,
    /// This either has zero length or has a length equal to `nzbw`
    pub p_self_bits: SmallVec<[Option<PBack>; 4]>,
    /// Operation
    pub op: Op<PState>,
    /// Location where this state is derived from
    pub location: Option<Location>,
    pub err: Option<EvalError>,
    /// The number of other `State`s, and only other `State`s, that reference
    /// this one through the `Op`s
    pub rc: usize,
    pub keep: bool,
    /// If the `State` has been lowered to elementary `State`s (`Static-`
    /// operations and roots). Note that a DFS might set this before actually
    /// being lowered.
    pub lowered_to_elementary: bool,
    /// If the `State` has been lowered from elementary `State`s to `TNode`s.
    /// Note that a DFS might set this before actually being lowered.
    pub lowered_to_tnodes: bool,
}

#[derive(Debug, Clone)]
pub struct Stator {
    pub states: Arena<PState, State>,
    pub states_to_lower: Vec<PState>,
}

impl Stator {
    pub fn new() -> Self {
        Self {
            states: Arena::new(),
            states_to_lower: vec![],
        }
    }
}

impl Ensemble {
    pub fn get_state_debug(&self, p_state: PState) -> Option<String> {
        self.stator
            .states
            .get(p_state)
            .map(|state| format!("{p_state} {state:#?}"))
    }

    pub fn dec_rc(&mut self, p_state: PState) -> Result<(), EvalError> {
        if let Some(state) = self.stator.states.get_mut(p_state) {
            state.rc = if let Some(x) = state.rc.checked_sub(1) {
                x
            } else {
                return Err(EvalError::OtherStr("tried to subtract a 0 reference count"))
            };
            if (state.rc == 0) && (!state.keep) {
                self.remove_state(p_state)?;
            }
            Ok(())
        } else {
            Err(EvalError::InvalidPtr)
        }
    }

    // TODO need to slightly rethink the PState/PNode system.
    // For now, we just prune states if any of their bits shares a surject with a
    // note.
    pub fn prune_unnoted_states(&mut self) -> Result<(), EvalError> {
        let mut adv = self.stator.states.advancer();
        while let Some(p_state) = adv.advance(&self.stator.states) {
            let state = &self.stator.states[p_state];
            let mut remove = true;
            'outer: for p_bit in &state.p_self_bits {
                if let Some(p_bit) = p_bit {
                    let mut equiv_adv = self.backrefs.advancer_surject(*p_bit);
                    while let Some(p_back) = equiv_adv.advance(&self.backrefs) {
                        if let Referent::Note(_) = self.backrefs.get_key(p_back).unwrap() {
                            remove = false;
                            break 'outer
                        }
                    }
                }
            }
            if remove {
                self.stator.states.get_mut(p_state).unwrap().keep = false;
                self.remove_state(p_state).unwrap();
            }
        }
        Ok(())
    }

    pub fn eval_state(&mut self, p_state: PState) -> Result<(), EvalError> {
        let state = &self.stator.states[p_state];
        let self_w = state.nzbw;
        let lit_op: Op<EAwi> = Op::translate(&state.op, |lhs: &mut [EAwi], rhs: &[PState]| {
            for (lhs, rhs) in lhs.iter_mut().zip(rhs.iter()) {
                let rhs = &self.stator.states[rhs];
                if let Op::Literal(ref lit) = rhs.op {
                    *lhs = EAwi::KnownAwi(lit.to_owned());
                } else {
                    *lhs = EAwi::Bitwidth(rhs.nzbw);
                }
            }
        });
        match lit_op.eval(self_w) {
            EvalResult::Valid(x) | EvalResult::Pass(x) => {
                let len = state.op.operands_len();
                for i in 0..len {
                    let source = self.stator.states[p_state].op.operands()[i];
                    self.dec_rc(source).unwrap();
                }
                // if the `op` is manually replaced outside of the specially handled lowering
                // `Copy` replacements, we need to check the values or else this change could be
                // lost if this was done after initializing `p_self_bits`
                let state = &mut self.stator.states[p_state];
                if !state.p_self_bits.is_empty() {
                    assert_eq!(state.p_self_bits.len(), x.bw());
                    for i in 0..x.bw() {
                        if let Some(p_bit) = state.p_self_bits[i] {
                            let p_equiv = self.backrefs.get_val(p_bit).unwrap().p_self_equiv;
                            self.evaluator.insert(Eval::Change(Change {
                                depth: 0,
                                p_equiv,
                                value: Value::Const(x.get(i).unwrap()),
                            }));
                        }
                    }
                }
                self.stator.states[p_state].op = Literal(x);
                Ok(())
            }
            EvalResult::Noop => {
                let operands = state.op.operands();
                let mut s = String::new();
                for op in operands {
                    writeln!(s, "{:#?},", self.stator.states[op]).unwrap();
                }
                Err(EvalError::OtherString(format!(
                    "`EvalResult::Noop` evaluation failure on state {} {:#?}\narguments: (\n{})",
                    p_state, state, s
                )))
            }
            EvalResult::Unevaluatable | EvalResult::PassUnevaluatable => {
                Err(EvalError::Unevaluatable)
            }
            EvalResult::AssertionSuccess => {
                if let Assert([_]) = state.op {
                    // this can be done because `Assert` is a sink that should not be used by
                    // anything
                    let state = self.stator.states.get_mut(p_state).unwrap();
                    assert_eq!(state.rc, 0);
                    // FIXME we definitely need to go through Notes for assertions,
                    // doc example fails otherwise on release
                    //state.keep = false;
                    //self.remove_state(p_state).unwrap();
                    Ok(())
                } else {
                    unreachable!()
                }
            }
            EvalResult::AssertionFailure => Err(EvalError::OtherString(format!(
                "`EvalResult::AssertionFailure` when evaluating state {} {:?}",
                p_state, state
            ))),
            EvalResult::Error(e) => {
                let operands = state.op.operands();
                let mut s = String::new();
                for op in operands {
                    writeln!(s, "{:?},", self.stator.states[op]).unwrap();
                }
                Err(EvalError::OtherString(format!(
                    "`EvalResult::Error` evaluation failure (\n{:#?}\n) on state {} \
                     {:#?}\narguments: (\n{})",
                    e, p_state, state, s
                )))
            }
        }
    }

    /// Assuming that the rootward tree from `p_state` is lowered down to the
    /// elementary `Op`s, this will create the `TNode` network
    pub fn dfs_lower_elementary_to_tnodes(&mut self, p_state: PState) -> Result<(), EvalError> {
        if let Some(state) = self.stator.states.get(p_state) {
            if state.lowered_to_tnodes {
                return Ok(())
            }
        } else {
            return Err(EvalError::InvalidPtr)
        }
        self.stator.states[p_state].lowered_to_tnodes = true;
        let mut path: Vec<(usize, PState)> = vec![(0, p_state)];
        loop {
            let (i, p_state) = path[path.len() - 1];
            let state = &self.stator.states[p_state];
            let nzbw = state.nzbw;
            let ops = state.op.operands();
            if ops.is_empty() {
                // reached a root
                match self.stator.states[p_state].op {
                    Literal(ref lit) => {
                        assert_eq!(lit.nzbw(), nzbw);
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                    }
                    Opaque(_, name) => {
                        if let Some(name) = name {
                            return Err(EvalError::OtherString(format!(
                                "cannot lower root opaque with name {name}"
                            )))
                        }
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                    }
                    ref op => return Err(EvalError::OtherString(format!("cannot lower {op:?}"))),
                }
                path.pop().unwrap();
                if path.is_empty() {
                    break
                }
                path.last_mut().unwrap().0 += 1;
            } else if i >= ops.len() {
                // checked all sources
                match self.stator.states[p_state].op {
                    Assert([x]) => {
                        // this is the only foolproof way of doing this, at least without more
                        // branches
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let len = self.stator.states[p_state].p_self_bits.len();
                        assert_eq!(len, self.stator.states[x].p_self_bits.len());
                        for i in 0..len {
                            let p_equiv0 = self.stator.states[p_state].p_self_bits[i].unwrap();
                            let p_equiv1 = self.stator.states[x].p_self_bits[i].unwrap();
                            self.union_equiv(p_equiv0, p_equiv1).unwrap();
                        }
                    }
                    Copy([x]) => {
                        // this is the only foolproof way of doing this, at least without more
                        // branches
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let len = self.stator.states[p_state].p_self_bits.len();
                        assert_eq!(len, self.stator.states[x].p_self_bits.len());
                        for i in 0..len {
                            let p_equiv0 = self.stator.states[p_state].p_self_bits[i].unwrap();
                            let p_equiv1 = self.stator.states[x].p_self_bits[i].unwrap();
                            self.union_equiv(p_equiv0, p_equiv1).unwrap();
                        }
                    }
                    StaticGet([bits], inx) => {
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let len = self.stator.states[bits].p_self_bits.len();
                        assert!(inx < len);
                        let p_self_bits = &self.stator.states[p_state].p_self_bits;
                        assert_eq!(p_self_bits.len(), 1);
                        let p_equiv0 = p_self_bits[0].unwrap();
                        let p_equiv1 = self.stator.states[bits].p_self_bits[inx].unwrap();
                        self.union_equiv(p_equiv0, p_equiv1).unwrap();
                    }
                    Concat(ref concat) => {
                        let concat_len = concat.len();
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let total_len = self.stator.states[p_state].p_self_bits.len();
                        let mut to = 0;
                        for c_i in 0..concat_len {
                            let c = if let Concat(ref concat) = self.stator.states[p_state].op {
                                concat.as_slice()[c_i]
                            } else {
                                unreachable!()
                            };
                            let len = self.stator.states[c].p_self_bits.len();
                            for i in 0..len {
                                let p_equiv0 =
                                    self.stator.states[p_state].p_self_bits[to + i].unwrap();
                                let p_equiv1 = self.stator.states[c].p_self_bits[i].unwrap();
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            }
                            to += len;
                        }
                        assert_eq!(total_len, to);
                    }
                    ConcatFields(ref concat) => {
                        let concat_len = concat.len();
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let total_len = self.stator.states[p_state].p_self_bits.len();
                        let mut to = 0;
                        for c_i in 0..concat_len {
                            let (c, (from, width)) =
                                if let ConcatFields(ref concat) = self.stator.states[p_state].op {
                                    (concat.t_as_slice()[c_i], concat.field_as_slice()[c_i])
                                } else {
                                    unreachable!()
                                };
                            let len = width.get();
                            for i in 0..len {
                                let p_equiv0 =
                                    self.stator.states[p_state].p_self_bits[to + i].unwrap();
                                let p_equiv1 = self.stator.states[c].p_self_bits[from + i].unwrap();
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            }
                            to += len;
                        }
                        assert_eq!(total_len, to);
                    }
                    Repeat([x]) => {
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let len = self.stator.states[p_state].p_self_bits.len();
                        let x_w = self.stator.states[x].p_self_bits.len();
                        assert!((len % x_w) == 0);
                        let mut from = 0;
                        for to in 0..len {
                            if from >= x_w {
                                from = 0;
                            }
                            let p_equiv0 = self.stator.states[p_state].p_self_bits[to].unwrap();
                            let p_equiv1 = self.stator.states[x].p_self_bits[from].unwrap();
                            self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            from += 1;
                        }
                    }
                    StaticLut(ref concat, ref table) => {
                        let table = table.clone();
                        let concat_len = concat.len();
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let mut inx_bits: SmallVec<[Option<PBack>; 8]> = smallvec![];
                        for c_i in 0..concat_len {
                            let c = if let StaticLut(ref concat, _) = self.stator.states[p_state].op
                            {
                                concat.as_slice()[c_i]
                            } else {
                                unreachable!()
                            };
                            let bits = &self.stator.states[c].p_self_bits;
                            inx_bits.extend(bits.iter().cloned());
                        }

                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let inx_len = inx_bits.len();
                        let out_bw = self.stator.states[p_state].p_self_bits.len();
                        let num_entries =
                            1usize.checked_shl(u32::try_from(inx_len).unwrap()).unwrap();
                        // this must be handled upstream
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
                            let p_equiv0 = self
                                .make_lut(&inx_bits, &single_bit_table, Some(p_state))
                                .unwrap();
                            let p_equiv1 = self.stator.states[p_state].p_self_bits[bit_i].unwrap();
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
                            let w = self.stator.states[v0].p_self_bits.len();
                            if w != self.stator.states[v1].p_self_bits.len() {
                                return Err(EvalError::OtherStr(
                                    "LoopHandle `Opaque` has a bitwidth mismatch of looper and \
                                     driver",
                                ))
                            }
                            // Loops work by an initial `Opaque` that gets registered earlier
                            // and is used by things that use the loop value. A second
                            // LoopHandle Opaque references the first with `p_looper` and
                            // supplies a driver.
                            for i in 0..w {
                                let p_looper = self.stator.states[v0].p_self_bits[i].unwrap();
                                let p_driver = self.stator.states[v1].p_self_bits[i].unwrap();
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
                    ref op => return Err(EvalError::OtherString(format!("cannot lower {op:?}"))),
                }
                path.pop().unwrap();
                if path.is_empty() {
                    break
                }
            } else {
                let p_next = ops[i];
                if self.stator.states[p_next].lowered_to_tnodes {
                    // do not visit
                    path.last_mut().unwrap().0 += 1;
                } else {
                    self.stator.states[p_next].lowered_to_tnodes = true;
                    path.push((0, p_next));
                }
            }
        }
        Ok(())
    }

    /// Lowers the rootward tree from `p_state` down to `TNode`s
    pub fn dfs_lower(epoch_shared: &EpochShared, p_state: PState) -> Result<(), EvalError> {
        Ensemble::dfs_lower_states_to_elementary(epoch_shared, p_state)?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        // the state can get removed by the above step
        if lock.ensemble.stator.states.contains(p_state) {
            let res = lock.ensemble.dfs_lower_elementary_to_tnodes(p_state);
            res.unwrap();
        }
        Ok(())
    }

    pub fn lower_all(epoch_shared: &EpochShared) -> Result<(), EvalError> {
        let lock = epoch_shared.epoch_data.borrow();
        let mut adv = lock.ensemble.stator.states.advancer();
        drop(lock);
        loop {
            let lock = epoch_shared.epoch_data.borrow();
            if let Some(p_state) = adv.advance(&lock.ensemble.stator.states) {
                // only do this to roots
                let state = &lock.ensemble.stator.states[p_state];
                if state.rc == 0 {
                    drop(lock);
                    Ensemble::dfs_lower(epoch_shared, p_state)?;
                } else {
                    drop(lock);
                }
            } else {
                break
            }
        }

        Ok(())
    }
}

impl Default for Stator {
    fn default() -> Self {
        Self::new()
    }
}
