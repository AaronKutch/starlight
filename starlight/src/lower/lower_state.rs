use std::num::NonZeroUsize;

use awint::{
    awint_dag::{smallvec::smallvec, ConcatFieldsType, ConcatType, Op::*, PState},
    bw,
};

use crate::{
    ensemble::Ensemble,
    epoch::EpochShared,
    lower::{lower_op, LowerManagement},
    Error,
};

impl Ensemble {
    /// Used for forbidden meta psuedo-DSL techniques in which a single state is
    /// replaced by more basic states.
    pub fn graft(&mut self, p_state: PState, operands: &[PState]) -> Result<(), Error> {
        #[cfg(debug_assertions)]
        {
            if (self.stator.states[p_state].op.operands_len() + 1) != operands.len() {
                return Err(Error::OtherStr(
                    "wrong number of operands for the `graft` function",
                ))
            }
            for (i, op) in self.stator.states[p_state].op.operands().iter().enumerate() {
                let current_nzbw = self.stator.states[operands[i + 1]].nzbw;
                let current_is_opaque = self.stator.states[operands[i + 1]].op.is_opaque();
                if self.stator.states[op].nzbw != current_nzbw {
                    return Err(Error::OtherString(format!(
                        "operand {}: a bitwidth of {:?} is trying to be grafted to a bitwidth of \
                         {:?}",
                        i, current_nzbw, self.stator.states[op].nzbw
                    )))
                }
                if !current_is_opaque {
                    return Err(Error::OtherStr(
                        "expected an `Opaque` for the `graft` function",
                    ))
                }
            }
            let lhs_w = self.stator.states[p_state].nzbw.get();
            let rhs_w = self.stator.states[operands[0]].nzbw.get();
            if lhs_w != rhs_w {
                return Err(Error::BitwidthMismatch(lhs_w, rhs_w))
            }
        }

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
                self.state_dec_rc(graftee).unwrap();
            }
        }

        // graft output
        let grafted = operands[0];
        self.stator.states.get_mut(p_state).unwrap().op = Copy([grafted]);
        self.stator.states[grafted].inc_rc();

        Ok(())
    }

    pub fn lower_op(epoch_shared: &EpochShared, p_state: PState) -> Result<bool, Error> {
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
                    .state_dec_rc(p)
                    .unwrap()
            }
        }
        let lock = epoch_shared.epoch_data.borrow();
        let state = lock.ensemble.stator.states.get(p_state).unwrap();
        let start_op = state.op.clone();
        let out_w = state.nzbw;
        drop(lock);
        lower_op(start_op, out_w, Tmp {
            ptr: p_state,
            epoch_shared,
        })
    }

    /// Lowers the rootward tree from `p_state` down to the elementary `Op`s
    pub fn dfs_lower_states_to_elementary(
        epoch_shared: &EpochShared,
        p_state: PState,
    ) -> Result<(), Error> {
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        if let Some(state) = lock.ensemble.stator.states.get(p_state) {
            if state.lowered_to_elementary {
                return Ok(())
            }
        } else {
            return Err(Error::InvalidPtr)
        }
        lock.ensemble.stator.states[p_state].lowered_to_elementary = true;

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
                // checked all sources, attempt evaluation first, this is crucial in preventing
                // wasted work in multiple layer lowerings
                match lock.ensemble.eval_state(p_state) {
                    Ok(()) => {
                        path.pop().unwrap();
                        if path.is_empty() {
                            break
                        } else {
                            continue
                        }
                    }
                    // Continue on to lowering
                    Err(Error::Unevaluatable) => (),
                    Err(e) => {
                        lock.ensemble.stator.states[p_state].err = Some(e.clone());
                        return Err(e)
                    }
                }
                let needs_lower = match lock.ensemble.stator.states[p_state].op {
                    Opaque(..) | Argument(_) | Literal(_) | Assert(_) | Copy(_) | StaticGet(..)
                    | Repeat(_) | StaticLut(..) => false,
                    // for dynamic LUTs
                    Mux(_) => false,
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
                                // or `Unknown` it?
                                lock.ensemble.stator.states[p_state].op = Opaque(smallvec![], None);
                                lock.ensemble.state_dec_rc(inx).unwrap();
                            } else {
                                lock.ensemble.stator.states[p_state].op =
                                    StaticLut(ConcatType::from_iter([inx]), lit);
                            }
                            lock.ensemble.state_dec_rc(lut).unwrap();
                        }
                        // else it is a dynamic LUT that could be lowered on the
                        // `LNode` side if needed
                        false
                    }
                    Get([bits, inx]) => {
                        if let Literal(ref lit) = lock.ensemble.stator.states[inx].op {
                            let lit = lit.clone();
                            let lit_u = lit.to_usize();
                            if lit_u >= lock.ensemble.stator.states[bits].nzbw.get() {
                                // TODO I realize now that no-op `get` specifically is fundamentally
                                // ill-defined to some extent because it returns `Option<bool>`, it
                                // must be asserted against, this
                                // provides the next best thing

                                // or TODO does it just cause `Unknown`?
                                lock.ensemble.stator.states[p_state].op = Opaque(smallvec![], None);
                                lock.ensemble.state_dec_rc(bits).unwrap();
                            } else {
                                lock.ensemble.stator.states[p_state].op = ConcatFields(
                                    ConcatFieldsType::from_iter([(bits, lit_u, bw(1))]),
                                );
                            }
                            lock.ensemble.state_dec_rc(inx).unwrap();
                            false
                        } else {
                            true
                        }
                    }
                    Set([bits, inx, bit]) => {
                        if let Literal(ref lit) = lock.ensemble.stator.states[inx].op {
                            let lit = lit.clone();
                            let lit_u = lit.to_usize();
                            let bits_w = lock.ensemble.stator.states[bits].nzbw.get();
                            if lit_u >= bits_w {
                                // no-op
                                lock.ensemble.stator.states[p_state].op = Copy([bits]);
                                lock.ensemble.state_dec_rc(bit).unwrap();
                            } else if let Some(lo_rem) = NonZeroUsize::new(lit_u) {
                                if let Some(hi_rem) = NonZeroUsize::new(bits_w - 1 - lit_u) {
                                    lock.ensemble.stator.states[p_state].op =
                                        ConcatFields(ConcatFieldsType::from_iter([
                                            (bits, 0, lo_rem),
                                            (bit, 0, bw(1)),
                                            (bits, lit_u + 1, hi_rem),
                                        ]));
                                } else {
                                    // setting the last bit
                                    lock.ensemble.stator.states[p_state].op =
                                        ConcatFields(ConcatFieldsType::from_iter([
                                            (bits, 0, lo_rem),
                                            (bit, 0, bw(1)),
                                        ]));
                                }
                            } else if let Some(rem) = NonZeroUsize::new(bits_w - 1) {
                                // setting the first bit
                                lock.ensemble.stator.states[p_state].op =
                                    ConcatFields(ConcatFieldsType::from_iter([
                                        (bit, 0, bw(1)),
                                        (bits, 1, rem),
                                    ]));
                            } else {
                                // setting a single bit
                                lock.ensemble.stator.states[p_state].op = Copy([bit]);
                                lock.ensemble.state_dec_rc(bits).unwrap();
                            }
                            lock.ensemble.state_dec_rc(inx).unwrap();
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
                    let lowering_done = match Ensemble::lower_op(&temporary, p_state) {
                        Ok(lowering_done) => lowering_done,
                        Err(e) => {
                            temporary.remove_as_current().unwrap();
                            let mut lock = epoch_shared.epoch_data.borrow_mut();
                            lock.ensemble.stator.states[p_state].err = Some(e.clone());
                            return Err(e)
                        }
                    };
                    // shouldn't be adding additional assertions
                    // TODO make sure there is no meta lowering using assertions assert!(temporary.
                    // assertions_empty());
                    let states = temporary.take_states_added();
                    temporary.remove_as_current().unwrap();
                    let mut lock = epoch_shared.epoch_data.borrow_mut();
                    for p_state in states {
                        lock.ensemble
                            .remove_state_if_pruning_allowed(p_state)
                            .unwrap();
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
                        lock.ensemble.stator.states[a].inc_rc();
                        lock.ensemble.state_dec_rc(p_next).unwrap();
                        p_next = a;
                    }
                    lock.ensemble.stator.states[p_next].lowered_to_elementary = true;
                    path.push((0, p_next));
                }
                drop(lock);
            }
        }

        Ok(())
    }
}
