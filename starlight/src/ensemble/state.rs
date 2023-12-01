use std::num::NonZeroUsize;

use awint::awint_dag::{
    lowering::{lower_state, LowerManagement},
    smallvec::{smallvec, SmallVec},
    triple_arena::{Advancer, Arena},
    EvalError, Location,
    Op::{self, *},
    PState,
};

use super::Value;
use crate::{
    awi,
    ensemble::{Ensemble, PBack},
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

    /// Used for forbidden meta psuedo-DSL techniques in which a single state is
    /// replaced by more basic states.
    pub fn graft(&mut self, p_state: PState, operands: &[PState]) -> Result<(), EvalError> {
        #[cfg(debug_assertions)]
        {
            if (self.stator.states[p_state].op.operands_len() + 1) != operands.len() {
                return Err(EvalError::WrongNumberOfOperands)
            }
            for (i, op) in self.stator.states[p_state].op.operands().iter().enumerate() {
                let current_nzbw = self.stator.states[operands[i + 1]].nzbw;
                let current_is_opaque = self.stator.states[operands[i + 1]].op.is_opaque();
                if self.stator.states[op].nzbw != current_nzbw {
                    return Err(EvalError::OtherString(format!(
                        "operand {}: a bitwidth of {:?} is trying to be grafted to a bitwidth of \
                         {:?}",
                        i, current_nzbw, self.stator.states[op].nzbw
                    )))
                }
                if !current_is_opaque {
                    return Err(EvalError::ExpectedOpaque)
                }
            }
            if self.stator.states[p_state].nzbw != self.stator.states[operands[0]].nzbw {
                return Err(EvalError::WrongBitwidth)
            }
        }

        // TODO what do we do when we make multi-output things
        // graft input
        for i in 1..operands.len() {
            let grafted = operands[i];
            let graftee = self.stator.states.get(p_state).unwrap().op.operands()[i - 1];
            if let Some(grafted) = self.stator.states.get_mut(grafted) {
                // change the grafted `Opaque` into a `Copy` that routes to the graftee instead
                // of needing to change all the operands of potentially many nodes
                grafted.op = Copy([graftee]);
            } else {
                // else the operand is not used because it was optimized away, this is removing
                // a tree outside of the grafted part
                self.dec_rc(graftee).unwrap();
            }
        }

        // graft output
        let grafted = operands[0];
        self.stator.states.get_mut(p_state).unwrap().op = Copy([grafted]);
        self.stator.states[grafted].rc = self.stator.states[grafted].rc.checked_add(1).unwrap();

        Ok(())
    }

    pub fn lower_state(epoch_shared: &EpochShared, p_state: PState) -> Result<bool, EvalError> {
        // TODO optimization to remove unused nodes early
        //let epoch = StateEpoch::new();
        struct Tmp<'a> {
            ptr: PState,
            epoch_shared: &'a EpochShared,
        }
        impl<'a> LowerManagement<PState> for Tmp<'a> {
            fn graft(&mut self, operands: &[PState]) {
                self.epoch_shared
                    .epoch_data
                    .borrow_mut()
                    .ensemble
                    .graft(self.ptr, operands)
                    .unwrap();
            }

            fn get_nzbw(&self, p: PState) -> NonZeroUsize {
                self.epoch_shared
                    .epoch_data
                    .borrow()
                    .ensemble
                    .stator
                    .states
                    .get(p)
                    .unwrap()
                    .nzbw
            }

            fn is_literal(&self, p: PState) -> bool {
                self.epoch_shared
                    .epoch_data
                    .borrow()
                    .ensemble
                    .stator
                    .states
                    .get(p)
                    .unwrap()
                    .op
                    .is_literal()
            }

            fn usize(&self, p: PState) -> usize {
                if let Literal(ref lit) = self
                    .epoch_shared
                    .epoch_data
                    .borrow()
                    .ensemble
                    .stator
                    .states
                    .get(p)
                    .unwrap()
                    .op
                {
                    if lit.bw() != 64 {
                        panic!()
                    }
                    lit.to_usize()
                } else {
                    panic!()
                }
            }

            fn bool(&self, p: PState) -> bool {
                if let Literal(ref lit) = self
                    .epoch_shared
                    .epoch_data
                    .borrow()
                    .ensemble
                    .stator
                    .states
                    .get(p)
                    .unwrap()
                    .op
                {
                    if lit.bw() != 1 {
                        panic!()
                    }
                    lit.to_bool()
                } else {
                    panic!()
                }
            }

            fn dec_rc(&mut self, p: PState) {
                self.epoch_shared
                    .epoch_data
                    .borrow_mut()
                    .ensemble
                    .dec_rc(p)
                    .unwrap()
            }
        }
        let lock = epoch_shared.epoch_data.borrow();
        let state = lock.ensemble.stator.states.get(p_state).unwrap();
        let start_op = state.op.clone();
        let out_w = state.nzbw;
        drop(lock);
        lower_state(start_op, out_w, Tmp {
            ptr: p_state,
            epoch_shared,
        })
    }

    /// Lowers the rootward tree from `p_state` down to the elementary `Op`s
    pub fn dfs_lower_states_to_elementary(
        epoch_shared: &EpochShared,
        p_state: PState,
    ) -> Result<(), EvalError> {
        let mut unimplemented = false;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        if lock.ensemble.stator.states[p_state].lowered_to_elementary {
            return Ok(())
        }
        lock.ensemble.stator.states[p_state].lowered_to_elementary = true;

        // NOTE be sure to reset this before returning from the function
        lock.keep_flag = false;
        drop(lock);
        let mut path: Vec<(usize, PState)> = vec![(0, p_state)];
        loop {
            let (i, p_state) = path[path.len() - 1];
            let mut lock = epoch_shared.epoch_data.borrow_mut();
            let state = &lock.ensemble.stator.states[p_state];
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
                let needs_lower = match lock.ensemble.stator.states[p_state].op {
                    Opaque(..) | Literal(_) | Copy(_) | StaticGet(..) | StaticSet(..)
                    | StaticLut(..) => false,
                    Lut([lut, inx]) => {
                        if let Literal(ref lit) = lock.ensemble.stator.states[lut].op {
                            let lit = lit.clone();
                            let out_w = lock.ensemble.stator.states[p_state].nzbw.get();
                            let inx_w = lock.ensemble.stator.states[inx].nzbw.get();
                            let no_op = if let Ok(inx_w) = u32::try_from(inx_w) {
                                if let Some(num_entries) = 1usize.checked_shl(inx_w) {
                                    (out_w * num_entries) != lit.bw()
                                } else {
                                    true
                                }
                            } else {
                                true
                            };
                            if no_op {
                                // TODO should I add the extra arg to `Lut` to fix this edge case?
                                lock.ensemble.stator.states[p_state].op =
                                    Opaque(smallvec![], None);
                                lock.ensemble.dec_rc(inx).unwrap();
                            } else {
                                lock.ensemble.stator.states[p_state].op = StaticLut([inx], lit);
                            }
                            lock.ensemble.dec_rc(lut).unwrap();
                            false
                        } else {
                            true
                        }
                    }
                    Get([bits, inx]) => {
                        if let Literal(ref lit) = lock.ensemble.stator.states[inx].op {
                            let lit = lit.clone();
                            let lit_u = lit.to_usize();
                            if lit_u >= lock.ensemble.stator.states[bits].nzbw.get() {
                                // TODO I realize now that no-op `get` specifically is fundamentally
                                // ill-defined to some extend because it returns `Option<bool>`, it
                                // must be asserted against, this
                                // provides the next best thing
                                lock.ensemble.stator.states[p_state].op =
                                    Opaque(smallvec![], None);
                                lock.ensemble.dec_rc(bits).unwrap();
                            } else {
                                lock.ensemble.stator.states[p_state].op = StaticGet([bits], lit_u);
                            }
                            lock.ensemble.dec_rc(inx).unwrap();
                            false
                        } else {
                            true
                        }
                    }
                    Set([bits, inx, bit]) => {
                        if let Literal(ref lit) = lock.ensemble.stator.states[inx].op {
                            let lit = lit.clone();
                            let lit_u = lit.to_usize();
                            if lit_u >= lock.ensemble.stator.states[bits].nzbw.get() {
                                // no-op
                                lock.ensemble.stator.states[p_state].op = Copy([bits]);
                                lock.ensemble.dec_rc(bit).unwrap();
                            } else {
                                lock.ensemble.stator.states[p_state].op =
                                    StaticSet([bits, bit], lit.to_usize());
                            }
                            lock.ensemble.dec_rc(inx).unwrap();
                            false
                        } else {
                            true
                        }
                    }
                    _ => true,
                };
                drop(lock);
                let lowering_done = if needs_lower {
                    // this is used to be able to remove ultimately unused temporaries
                    let mut temporary = EpochShared::shared_with(epoch_shared);
                    temporary.set_as_current();
                    let lowering_done = match Ensemble::lower_state(&temporary, p_state) {
                        Ok(lowering_done) => lowering_done,
                        Err(EvalError::Unimplemented) => {
                            // finish lowering as much as possible
                            unimplemented = true;
                            true
                        }
                        Err(e) => {
                            temporary.remove_as_current();
                            let mut lock = epoch_shared.epoch_data.borrow_mut();
                            lock.ensemble.stator.states[p_state].err = Some(e.clone());
                            lock.keep_flag = true;
                            return Err(e)
                        }
                    };
                    // shouldn't be adding additional assertions
                    // TODO after migrating the old lowering tests to a starlight-like system, make
                    // sure there are none using assertions assert!(temporary.
                    // assertions_empty());
                    let states = temporary.take_states_added();
                    temporary.remove_as_current();
                    let mut lock = epoch_shared.epoch_data.borrow_mut();
                    for p_state in states {
                        let state = &lock.ensemble.stator.states[p_state];
                        if (!state.keep) && (state.rc == 0) {
                            lock.ensemble.remove_state(p_state).unwrap();
                        }
                    }
                    lowering_done
                } else {
                    true
                };
                if lowering_done {
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                } else {
                    // else do not call `path.pop`, restart the DFS here
                    path.last_mut().unwrap().0 = 0;
                }
            } else {
                let mut p_next = ops[i];
                if lock.ensemble.stator.states[p_next].lowered_to_elementary {
                    // do not visit
                    path.last_mut().unwrap().0 += 1;
                } else {
                    while let Copy([a]) = lock.ensemble.stator.states[p_next].op {
                        // special optimization case: forward Copies
                        lock.ensemble.stator.states[p_state].op.operands_mut()[i] = a;
                        let rc = &mut lock.ensemble.stator.states[a].rc;
                        *rc = (*rc).checked_add(1).unwrap();
                        lock.ensemble.dec_rc(p_next).unwrap();
                        p_next = a;
                    }
                    lock.ensemble.stator.states[p_next].lowered_to_elementary = true;
                    path.push((0, p_next));
                }
                drop(lock);
            }
        }

        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.keep_flag = true;

        if unimplemented {
            Err(EvalError::Unimplemented)
        } else {
            Ok(())
        }
    }

    /// Assuming that the rootward tree from `p_state` is lowered down to the
    /// elementary `Op`s, this will create the `TNode` network
    pub fn dfs_lower_elementary_to_tnodes(&mut self, p_state: PState) -> Result<(), EvalError> {
        if self.stator.states[p_state].lowered_to_tnodes {
            return Ok(())
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
                    StaticSet([bits, bit], inx) => {
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let len = self.stator.states[p_state].p_self_bits.len();
                        assert_eq!(len, self.stator.states[bits].p_self_bits.len());
                        // this must be handled upstream
                        assert!(inx < len);
                        for i in 0..len {
                            let p_equiv0 = self.stator.states[p_state].p_self_bits[i].unwrap();
                            if i == inx {
                                let p_bit = &self.stator.states[bit].p_self_bits;
                                assert_eq!(p_bit.len(), 1);
                                let p_equiv1 = p_bit[0].unwrap();
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            } else {
                                let p_equiv1 = self.stator.states[bits].p_self_bits[i].unwrap();
                                self.union_equiv(p_equiv0, p_equiv1).unwrap();
                            };
                        }
                    }
                    StaticLut([inx], ref table) => {
                        let table = table.clone();
                        self.initialize_state_bits_if_needed(p_state).unwrap();
                        let inx_bits = self.stator.states[inx].p_self_bits.clone();
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
        let res = epoch_shared
            .epoch_data
            .borrow_mut()
            .ensemble
            .dfs_lower_elementary_to_tnodes(p_state);
        res.unwrap();
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
