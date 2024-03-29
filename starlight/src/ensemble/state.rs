use std::{
    fmt::Write,
    num::{NonZeroU64, NonZeroUsize},
};

use awint::{
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        triple_arena::{Advancer, Arena},
        EAwi, EvalResult, Location,
        Op::{self, *},
        PState,
    },
    Awi,
};

use crate::{
    awi,
    awi_structs::{DELAY, DELAYED_LOOP_SOURCE, LOOP_SOURCE, UNDRIVEN_LOOP_SOURCE},
    ensemble::{ChangeKind, Delay, DynamicValue, Ensemble, Equiv, Event, PBack, Referent, Value},
    epoch::EpochShared,
    Error,
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
    pub err: Option<Error>,
    /// The number of other `State`s, and only other `State`s, that reference
    /// this one through the `Op`s
    pub rc: usize,
    /// The number of `RNode`s referencing this state
    pub extern_rc: usize,
    /// If the `State` has been lowered to elementary `State`s (`Static-`
    /// operations and roots). Note that a DFS might set this before actually
    /// being lowered.
    pub lowered_to_elementary: bool,
    /// If the `State` has been lowered from elementary `State`s to `LNode`s.
    /// Note that a DFS might set this before actually being lowered.
    pub lowered_to_lnodes: bool,
}

impl State {
    /// Returns if pruning this state is allowed. Internal or external
    /// references prevent pruning.
    pub fn pruning_allowed(&self) -> bool {
        (self.rc == 0) && (self.extern_rc == 0)
    }

    pub fn inc_rc(&mut self) {
        self.rc = self.rc.checked_add(1).unwrap()
    }

    #[must_use]
    pub fn dec_rc(&mut self) -> Option<()> {
        self.rc = self.rc.checked_sub(1)?;
        Some(())
    }

    pub fn inc_extern_rc(&mut self) {
        self.extern_rc = self.extern_rc.checked_add(1).unwrap()
    }

    pub fn dec_extern_rc(&mut self) {
        self.extern_rc = self.extern_rc.checked_sub(1).unwrap()
    }
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

    /// Checks that there are no remaining states, then shrinks allocations
    pub fn check_clear(&mut self) -> Result<(), Error> {
        if !self.states.is_empty() {
            return Err(Error::OtherStr("states need to be empty"));
        }
        self.states.clear_and_shrink();
        self.states_to_lower.clear();
        self.states_to_lower.shrink_to_fit();
        Ok(())
    }
}

impl Ensemble {
    pub fn make_state(
        &mut self,
        nzbw: NonZeroUsize,
        op: Op<PState>,
        location: Option<Location>,
    ) -> PState {
        for operand in op.operands() {
            let state = self.stator.states.get_mut(*operand).unwrap();
            state.rc = state.rc.checked_add(1).unwrap();
        }
        self.stator.states.insert(State {
            nzbw,
            p_self_bits: SmallVec::new(),
            op,
            location,
            err: None,
            rc: 0,
            extern_rc: 0,
            lowered_to_elementary: false,
            lowered_to_lnodes: false,
        })
    }

    /// If `p_state_bits.is_empty`, this will create new equivalences and
    /// `Referent::ThisStateBits`s needed for every self bit. Sets the values to
    /// a constant if the `Op` is a `Literal`, otherwise sets to unknown.
    #[allow(clippy::single_match)]
    pub fn initialize_state_bits_if_needed(&mut self, p_state: PState) -> Result<(), Error> {
        let state = if let Some(state) = self.stator.states.get(p_state) {
            state
        } else {
            return Err(Error::InvalidPtr);
        };
        if !state.p_self_bits.is_empty() {
            return Ok(())
        }
        let w = state.nzbw;
        // the corresponding bit values
        let mut vals = Awi::zero(w);
        // if known
        let mut known = false;
        // if const
        let mut is_const = false;
        match state.op {
            Op::Literal(ref awi) => {
                vals.copy_(awi).unwrap();
                known = true;
                is_const = true;
            }
            Op::Argument(_) => {
                return Ok(());
            }
            Op::Opaque(ref v, name) => {
                if name.is_none() {
                    assert!(v.is_empty());
                    is_const = true;
                }
            }
            _ => (),
        }
        let mut bits = smallvec![];
        for i in 0..state.nzbw.get() {
            let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
                (
                    Referent::ThisEquiv,
                    Equiv::new(
                        p_self_equiv,
                        if is_const {
                            if known {
                                Value::Const(vals.get(i).unwrap())
                            } else {
                                Value::ConstUnknown
                            }
                        } else if known {
                            Value::Dynam(vals.get(i).unwrap())
                        } else {
                            Value::Unknown
                        },
                    ),
                )
            });
            bits.push(Some(
                self.backrefs
                    .insert_key(p_equiv, Referent::ThisStateBit(p_state, i))
                    .unwrap(),
            ));
        }
        let state = self.stator.states.get_mut(p_state).unwrap();
        state.p_self_bits = bits;
        Ok(())
    }

    /// Triggers a cascade of state removals if `pruning_allowed()` and
    /// their reference counts are zero
    pub fn remove_state_if_pruning_allowed(&mut self, p_state: PState) -> Result<(), Error> {
        if !self.stator.states.contains(p_state) {
            return Err(Error::InvalidPtr);
        }
        let mut pstate_stack = vec![p_state];
        while let Some(p) = pstate_stack.pop() {
            let mut delete = false;
            if let Some(state) = self.stator.states.get(p) {
                if state.pruning_allowed() {
                    delete = true;
                }
            }
            if delete {
                for i in 0..self.stator.states[p].op.operands_len() {
                    let op = self.stator.states[p].op.operands()[i];
                    if self.stator.states[op].dec_rc().is_none() {
                        return Err(Error::OtherStr("tried to subtract a 0 reference count"))
                    };
                    pstate_stack.push(op);
                }
                let mut state = self.stator.states.remove(p).unwrap();
                for p_self_state in state.p_self_bits.drain(..) {
                    if let Some(p_self_state) = p_self_state {
                        self.backrefs.remove_key(p_self_state).unwrap();
                    }
                }
            }
        }
        Ok(())
    }

    pub fn force_remove_all_states(&mut self) -> Result<(), Error> {
        // set associated states to none to help prevent issues when there are no
        // generation counters
        self.remove_all_rnode_associated_states();
        for (_, mut state) in self.stator.states.drain() {
            for p_self_state in state.p_self_bits.drain(..) {
                if let Some(p_self_state) = p_self_state {
                    self.backrefs.remove_key(p_self_state).unwrap();
                }
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn get_state_debug(&self, p_state: PState) -> Option<String> {
        self.stator
            .states
            .get(p_state)
            .map(|state| format!("{p_state} {state:#?}"))
    }

    pub fn state_dec_extern_rc(&mut self, p_state: PState) -> Result<(), Error> {
        if let Some(state) = self.stator.states.get_mut(p_state) {
            state.extern_rc = if let Some(x) = state.extern_rc.checked_sub(1) {
                x
            } else {
                return Err(Error::OtherStr("tried to subtract a 0 reference count"))
            };
            self.remove_state_if_pruning_allowed(p_state)?;
            Ok(())
        } else {
            Err(Error::InvalidPtr)
        }
    }

    pub fn state_dec_rc(&mut self, p_state: PState) -> Result<(), Error> {
        if let Some(state) = self.stator.states.get_mut(p_state) {
            state.rc = if let Some(x) = state.rc.checked_sub(1) {
                x
            } else {
                return Err(Error::OtherStr("tried to subtract a 0 reference count"))
            };
            self.remove_state_if_pruning_allowed(p_state)?;
            Ok(())
        } else {
            Err(Error::InvalidPtr)
        }
    }

    /// Prunes all states with `pruning_allowed()`
    pub fn prune_unused_states(&mut self) -> Result<(), Error> {
        let mut adv = self.stator.states.advancer();
        while let Some(p_state) = adv.advance(&self.stator.states) {
            self.remove_state_if_pruning_allowed(p_state).unwrap();
        }
        Ok(())
    }

    pub fn eval_state(&mut self, p_state: PState) -> Result<(), Error> {
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
                    self.state_dec_rc(source).unwrap();
                }
                // if the `op` is manually replaced outside of the specially handled lowering
                // `Copy` replacements, we need to check the values or else this change could be
                // lost if this was done after initializing `p_self_bits`
                if !self.stator.states[p_state].p_self_bits.is_empty() {
                    debug_assert_eq!(self.stator.states[p_state].p_self_bits.len(), x.bw());
                    for i in 0..x.bw() {
                        if let Some(p_bit) = self.stator.states[p_state].p_self_bits[i] {
                            let p_equiv = self.backrefs.get_val(p_bit).unwrap().p_self_equiv;
                            // unwrap because this should never fail, events would process
                            // incorrectly
                            self.change_value(
                                p_equiv,
                                Value::Const(x.get(i).unwrap()),
                                NonZeroU64::new(1).unwrap(),
                            )
                            .unwrap();
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
                Err(Error::OtherString(format!(
                    "`EvalResult::Noop` evaluation failure on state {} {:#?}\narguments: (\n{})",
                    p_state, state, s
                )))
            }
            EvalResult::Unevaluatable | EvalResult::PassUnevaluatable => Err(Error::Unevaluatable),
            EvalResult::AssertionSuccess => {
                if let Assert([_]) = state.op {
                    // this can be done because `Assert` is a sink that should not be used by
                    // anything
                    let state = self.stator.states.get_mut(p_state).unwrap();
                    debug_assert_eq!(state.rc, 0);
                    self.remove_state_if_pruning_allowed(p_state).unwrap();
                    Ok(())
                } else {
                    unreachable!()
                }
            }
            EvalResult::AssertionFailure => Err(Error::OtherString(format!(
                "`EvalResult::AssertionFailure` when evaluating state {} {:?}",
                p_state, state
            ))),
            EvalResult::Error(e) => {
                let operands = state.op.operands();
                let mut s = String::new();
                for op in operands {
                    writeln!(s, "{:?},", self.stator.states[op]).unwrap();
                }
                Err(Error::OtherString(format!(
                    "`EvalResult::Error` evaluation failure (\n{:#?}\n) on state {} \
                     {:#?}\narguments: (\n{})",
                    e, p_state, state, s
                )))
            }
        }
    }

    /// Assuming that the rootward tree from `p_state` is lowered down to the
    /// elementary `Op`s, this will create the `LNode` network
    pub fn dfs_lower_elementary_to_lnodes(&mut self, p_state: PState) -> Result<(), Error> {
        if let Some(state) = self.stator.states.get(p_state) {
            if state.lowered_to_lnodes {
                return Ok(())
            }
        } else {
            return Err(Error::InvalidPtr)
        }
        self.stator.states[p_state].lowered_to_lnodes = true;
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
                        debug_assert_eq!(lit.nzbw(), nzbw);
                        self.initialize_state_bits_if_needed(p_state)?;
                    }
                    Argument(ref a) => {
                        debug_assert_eq!(a.nzbw(), nzbw);
                    }
                    Opaque(_, name) => {
                        if let Some(name) = name {
                            match name {
                                "LazyOpaque" => (),
                                "Delay" => {
                                    return Err(Error::OtherStr(
                                        "cannot lower delay opaque with no `Op` inputs, some \
                                         variant was violated",
                                    ))
                                }
                                "UndrivenLoopSource" | "LoopSource" | "DelayedLoopSource" => {
                                    return Err(Error::OtherStr(
                                        "cannot lower loop source opaque with no initial value, \
                                         some variant was violated",
                                    ))
                                }
                                name => {
                                    return Err(Error::OtherString(format!(
                                        "cannot lower root opaque with name {name}"
                                    )))
                                }
                            }
                        }
                        self.initialize_state_bits_if_needed(p_state)?;
                    }
                    ref op => return Err(Error::OtherString(format!("cannot lower {op:?}"))),
                }
                path.pop().unwrap();
                if path.is_empty() {
                    break
                }
                path.last_mut().unwrap().0 += 1;
            } else if i >= ops.len() {
                // checked all sources
                lower_elementary_to_lnodes_intermediate(self, p_state)?;
                path.pop().unwrap();
                if path.is_empty() {
                    break
                }
            } else {
                let p_next = ops[i];
                if self.stator.states[p_next].lowered_to_lnodes {
                    // in the case of circular cases with `Loop`s, if the DFS goes around and does
                    // not encounter a root, the argument needs to be initialized or else any branch
                    // of `lower_elementary_to_lnodes_intermediate` could fail
                    self.initialize_state_bits_if_needed(p_next)?;
                    // do not visit
                    path.last_mut().unwrap().0 += 1;
                } else {
                    self.stator.states[p_next].lowered_to_lnodes = true;
                    path.push((0, p_next));
                }
            }
        }
        Ok(())
    }

    /// Lowers the rootward tree from `p_state` down to `LNode`s
    pub fn dfs_lower(epoch_shared: &EpochShared, p_state: PState) -> Result<(), Error> {
        Ensemble::dfs_lower_states_to_elementary(epoch_shared, p_state)?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        // the state can get removed by the above step
        if lock.ensemble.stator.states.contains(p_state) {
            lock.ensemble.dfs_lower_elementary_to_lnodes(p_state)
        } else {
            Ok(())
        }
    }

    /// Lowers `RNode`s with the `lower_before_pruning` flag
    pub fn lower_for_rnodes(epoch_shared: &EpochShared) -> Result<(), Error> {
        let lock = epoch_shared.epoch_data.borrow();
        let mut adv = lock.ensemble.notary.rnodes().advancer();
        drop(lock);
        loop {
            let mut lock = epoch_shared.epoch_data.borrow_mut();
            if let Some(p_rnode) = adv.advance(lock.ensemble.notary.rnodes()) {
                // only lower state trees attached to rnodes that need lowering
                let rnode = lock.ensemble.notary.rnodes.get_val_mut(p_rnode).unwrap();
                if rnode.lower_before_pruning {
                    drop(lock);
                    Ensemble::initialize_rnode_if_needed(epoch_shared, p_rnode, true)?;
                } else {
                    lock.ensemble
                        .initialize_rnode_if_needed_no_lowering(p_rnode, true)?;
                    drop(lock);
                }
            } else {
                break
            }
        }

        Ok(())
    }

    pub fn handle_states_to_lower(epoch_shared: &EpochShared) -> Result<(), Error> {
        // empty `states_to_lower`
        loop {
            let mut lock = epoch_shared.epoch_data.borrow_mut();
            if let Some(p_state) = lock.ensemble.stator.states_to_lower.pop() {
                if let Some(state) = lock.ensemble.stator.states.get(p_state) {
                    // first check that it has not already been lowered
                    if !state.lowered_to_lnodes {
                        drop(lock);
                        Ensemble::dfs_lower(epoch_shared, p_state)?;
                    }
                }
            } else {
                break
            }
        }
        Ok(())
    }
}

fn lower_elementary_to_lnodes_intermediate(
    this: &mut Ensemble,
    p_state: PState,
) -> Result<(), Error> {
    this.initialize_state_bits_if_needed(p_state)?;
    match this.stator.states[p_state].op {
        Assert([x]) => {
            // this is the only foolproof way of doing this, at least without more
            // branches
            let len = this.stator.states[p_state].p_self_bits.len();
            debug_assert_eq!(len, this.stator.states[x].p_self_bits.len());
            for i in 0..len {
                let p_equiv0 = this.stator.states[p_state].p_self_bits[i].unwrap();
                let p_equiv1 = this.stator.states[x].p_self_bits[i].unwrap();
                this.union_equiv(p_equiv0, p_equiv1).unwrap();
            }
        }
        Copy([x]) => {
            // this is the only foolproof way of doing this, at least without more
            // branches
            let len = this.stator.states[p_state].p_self_bits.len();
            debug_assert_eq!(len, this.stator.states[x].p_self_bits.len());
            for i in 0..len {
                let p_equiv0 = this.stator.states[p_state].p_self_bits[i].unwrap();
                let p_equiv1 = this.stator.states[x].p_self_bits[i].unwrap();
                this.union_equiv(p_equiv0, p_equiv1).unwrap();
            }
        }
        StaticGet([bits], inx) => {
            let len = this.stator.states[bits].p_self_bits.len();
            debug_assert!(inx < len);
            let p_self_bits = &this.stator.states[p_state].p_self_bits;
            debug_assert_eq!(p_self_bits.len(), 1);
            let p_equiv0 = p_self_bits[0].unwrap();
            let p_equiv1 = this.stator.states[bits].p_self_bits[inx].unwrap();
            this.union_equiv(p_equiv0, p_equiv1).unwrap();
        }
        Concat(ref concat) => {
            let concat_len = concat.len();
            let total_len = this.stator.states[p_state].p_self_bits.len();
            let mut to = 0;
            for c_i in 0..concat_len {
                let c = if let Concat(ref concat) = this.stator.states[p_state].op {
                    concat.as_slice()[c_i]
                } else {
                    unreachable!()
                };
                let len = this.stator.states[c].p_self_bits.len();
                for i in 0..len {
                    let p_equiv0 = this.stator.states[p_state].p_self_bits[to + i].unwrap();
                    let p_equiv1 = this.stator.states[c].p_self_bits[i].unwrap();
                    this.union_equiv(p_equiv0, p_equiv1).unwrap();
                }
                to += len;
            }
            debug_assert_eq!(total_len, to);
        }
        ConcatFields(ref concat) => {
            let concat_len = concat.len();
            let total_len = this.stator.states[p_state].p_self_bits.len();
            let mut to = 0;
            for c_i in 0..concat_len {
                let (c, (from, width)) =
                    if let ConcatFields(ref concat) = this.stator.states[p_state].op {
                        (concat.t_as_slice()[c_i], concat.field_as_slice()[c_i])
                    } else {
                        unreachable!()
                    };
                let len = width.get();
                for i in 0..len {
                    let p_equiv0 = this.stator.states[p_state].p_self_bits[to + i].unwrap();
                    let p_equiv1 = this.stator.states[c].p_self_bits[from + i].unwrap();
                    this.union_equiv(p_equiv0, p_equiv1).unwrap();
                }
                to += len;
            }
            debug_assert_eq!(total_len, to);
        }
        Repeat([x]) => {
            let len = this.stator.states[p_state].p_self_bits.len();
            let x_w = this.stator.states[x].p_self_bits.len();
            let mut from = 0;
            for to in 0..len {
                if from >= x_w {
                    from = 0;
                }
                let p_equiv0 = this.stator.states[p_state].p_self_bits[to].unwrap();
                let p_equiv1 = this.stator.states[x].p_self_bits[from].unwrap();
                this.union_equiv(p_equiv0, p_equiv1).unwrap();
                from += 1;
            }
        }
        StaticLut(ref concat, ref lut) => {
            let lut = lut.clone();
            let concat_len = concat.len();
            let mut inx_bits: SmallVec<[Option<PBack>; 8]> = smallvec![];
            for c_i in 0..concat_len {
                let c = if let StaticLut(ref concat, _) = this.stator.states[p_state].op {
                    concat.as_slice()[c_i]
                } else {
                    unreachable!()
                };
                let bits = &this.stator.states[c].p_self_bits;
                inx_bits.extend(bits.iter().cloned());
            }

            let inx_len = inx_bits.len();
            let out_bw = this.stator.states[p_state].p_self_bits.len();
            let num_entries = 1usize.checked_shl(u32::try_from(inx_len).unwrap()).unwrap();
            // this must be handled upstream
            debug_assert_eq!(out_bw * num_entries, lut.bw());
            // convert from multiple out to single out bit lut
            for bit_i in 0..out_bw {
                let single_bit_lut = if out_bw == 1 {
                    lut.clone()
                } else {
                    let mut val = awi::Awi::zero(NonZeroUsize::new(num_entries).unwrap());
                    for i in 0..num_entries {
                        val.set(i, lut.get((i * out_bw) + bit_i).unwrap()).unwrap();
                    }
                    val
                };
                let p_equiv0 = this.make_lut(&inx_bits, &single_bit_lut, Some(p_state));
                let p_equiv1 = this.stator.states[p_state].p_self_bits[bit_i].unwrap();
                this.union_equiv(p_equiv0, p_equiv1).unwrap();
            }
        }
        Lut([lut, inx]) => {
            let inx_len = this.stator.states[inx].p_self_bits.len();
            let out_bw = this.stator.states[p_state].p_self_bits.len();
            let num_entries = 1usize.checked_shl(u32::try_from(inx_len).unwrap()).unwrap();
            // this must be handled upstream
            debug_assert_eq!(
                out_bw * num_entries,
                this.stator.states[lut].p_self_bits.len()
            );

            let out_bw = this.stator.states[p_state].p_self_bits.len();
            for bit_i in 0..out_bw {
                let mut p_lut_bits = vec![];
                let inx_bits = this.stator.states[inx].p_self_bits.clone();
                let lut_bits = &this.stator.states[lut].p_self_bits;
                for i in 0..num_entries {
                    if let Some(p_back) = lut_bits[(i * out_bw) + bit_i] {
                        p_lut_bits.push(DynamicValue::Dynam(p_back));
                    } else {
                        p_lut_bits.push(DynamicValue::ConstUnknown);
                    }
                }
                let p_equiv0 = this.make_dynamic_lut(&inx_bits, &p_lut_bits, Some(p_state));
                let p_equiv1 = this.stator.states[p_state].p_self_bits[bit_i].unwrap();
                this.union_equiv(p_equiv0, p_equiv1).unwrap();
            }
        }
        Mux([lhs, rhs, b]) => {
            let out_bw = this.stator.states[p_state].p_self_bits.len();
            let inx_bit = &this.stator.states[b].p_self_bits;
            debug_assert_eq!(inx_bit.len(), 1);
            debug_assert_eq!(out_bw, this.stator.states[lhs].p_self_bits.len());
            debug_assert_eq!(out_bw, this.stator.states[rhs].p_self_bits.len());
            let inx_bit = inx_bit[0];

            for bit_i in 0..out_bw {
                let lut0 = this.stator.states[lhs].p_self_bits[bit_i].unwrap();
                let lut1 = this.stator.states[rhs].p_self_bits[bit_i].unwrap();
                let p_equiv0 = this.make_dynamic_lut(
                    &[inx_bit],
                    &[DynamicValue::Dynam(lut0), DynamicValue::Dynam(lut1)],
                    Some(p_state),
                );
                let p_equiv1 = this.stator.states[p_state].p_self_bits[bit_i].unwrap();
                this.union_equiv(p_equiv0, p_equiv1).unwrap();
            }
        }
        Opaque(ref v, name) => {
            if let Some(name) = name {
                match name {
                    DELAY => {
                        if v.len() != 2 {
                            return Err(Error::OtherStr(
                                "`Delay` has an unexpected number of arguments",
                            ))
                        }
                        let w = this.stator.states[p_state].p_self_bits.len();
                        let p_driver_state = v[0];
                        let p_delay_state = v[1];
                        let delay =
                            if let Op::Argument(ref delay) = this.stator.states[p_delay_state].op {
                                // the delay should have had `shrink_to_msb` called on it
                                if delay.bw() > 128 {
                                    return Err(Error::OtherStr(
                                        "`Delay` delay amount is unexpectedly large",
                                    ))
                                }
                                if delay.is_zero() {
                                    // the function that creates `Delay` is supposed to do a no-op
                                    // or copy instead
                                    return Err(Error::OtherStr("`Delay` delay amount is zero"))
                                }
                                Delay::from_amount(delay.to_u128())
                            } else {
                                return Err(Error::OtherStr(
                                    "`Delay` does not use the correct `Op::Argument`",
                                ))
                            };
                        for i in 0..w {
                            let p_driver =
                                this.stator.states[p_driver_state].p_self_bits[i].unwrap();
                            // We could potentially set the initial value to the initial value of
                            // the driver, but I suspect that unlike the `LazyAwi` driving case,
                            // this is fundamentally an ill defined issue when zero delay loops are
                            // involved. I believe that for the `drive` function interface at least,
                            // the source should simply start as a dynamic unknown and let the
                            // delayed drive update it later.

                            // however we do want the initial value to detect immediate quiescence
                            // when the driver is already `Unknown`
                            let init_val = this.backrefs.get_val(p_driver).unwrap().val;
                            let p_source = this.stator.states[p_state].p_self_bits[i].unwrap();

                            let p_tnode = this.make_tnode(p_source, p_driver, delay);
                            if init_val != Value::Unknown {
                                // setup the delayed drive
                                this.eval_tnode(p_tnode).unwrap();
                            }
                        }
                    }
                    UNDRIVEN_LOOP_SOURCE => {
                        if v.len() != 1 {
                            return Err(Error::OtherStr(
                                "undriven loop source has an unexpected number of arguments",
                            ))
                        }
                        return Err(Error::OtherString(format!(
                            "cannot lower an undriven `Loop` or `Net`, some `drive_*` function \
                             has not been called on a loop source with state {p_state}"
                        )))
                    }
                    LOOP_SOURCE => {
                        if v.len() != 2 {
                            return Err(Error::OtherStr(
                                "loop source has an unexpected number of arguments",
                            ))
                        }
                        let w = this.stator.states[p_state].p_self_bits.len();
                        let p_initial_state = v[0];
                        let p_driver_state = v[1];
                        if w != this.stator.states[p_initial_state].p_self_bits.len() {
                            return Err(Error::OtherStr(
                                "`Loop` has a bitwidth mismatch of looper and initial state",
                            ))
                        }
                        if w != this.stator.states[p_driver_state].p_self_bits.len() {
                            return Err(Error::OtherStr(
                                "`Loop` has a bitwidth mismatch of looper and driver",
                            ))
                        }
                        for i in 0..w {
                            let p_looper = this.stator.states[p_state].p_self_bits[i].unwrap();
                            let p_driver =
                                this.stator.states[p_driver_state].p_self_bits[i].unwrap();
                            let p_initial =
                                this.stator.states[p_initial_state].p_self_bits[i].unwrap();
                            let init_val = this.backrefs.get_val(p_initial).unwrap().val;
                            // the loop source is an internal `Opaque` root at this point, we
                            // initiate the initial event chain ourselves.

                            let p_tnode = this.make_tnode(p_looper, p_driver, Delay::zero());

                            // In most cases, the initial loop value ends up looping around to
                            // overwrite whatever the source was, however if it does not do so for
                            // zero delay nodes, we need to have a backup event with the lowest
                            // priority
                            this.evaluator.push_event(Event {
                                partial_ord_num: NonZeroU64::MAX,
                                change_kind: ChangeKind::TNode(p_tnode),
                            });

                            // an interesting thing that falls out is that a const value downcasts
                            // to a dynamic value, perhaps there should
                            // be an integer level of constness?

                            let init_val = match init_val {
                                Value::ConstUnknown => Value::Unknown,
                                Value::Const(b) => Value::Dynam(b),
                                Value::Unknown | Value::Dynam(_) => {
                                    return Err(Error::OtherStr(
                                        "A `Loop`'s initial value could not be calculated as a \
                                         constant known or constant unknown in lowering, the \
                                         argument to `Loop::from_*` needs to evaluate to a \
                                         constant",
                                    ));
                                }
                            };
                            // initial event for the initial value, need to do this in general
                            // because the state bit can get optimized away before we actually use
                            // it
                            let p_back = this.backrefs.get_val(p_looper).unwrap().p_self_equiv;
                            this.evaluator.push_event(Event {
                                partial_ord_num: NonZeroU64::new(1).unwrap(),
                                change_kind: ChangeKind::Manual(p_back, init_val),
                            });
                        }
                    }
                    DELAYED_LOOP_SOURCE => {
                        if v.len() != 3 {
                            return Err(Error::OtherStr(
                                "delayed loop source has an unexpected number of arguments",
                            ))
                        }
                        let w = this.stator.states[p_state].p_self_bits.len();
                        let p_initial_state = v[0];
                        let p_driver_state = v[1];
                        let p_delay_state = v[2];
                        if w != this.stator.states[p_initial_state].p_self_bits.len() {
                            return Err(Error::OtherStr(
                                "`Loop` has a bitwidth mismatch of looper and initial state",
                            ))
                        }
                        if w != this.stator.states[p_driver_state].p_self_bits.len() {
                            return Err(Error::OtherStr(
                                "`Loop` has a bitwidth mismatch of looper and driver",
                            ))
                        }
                        let delay =
                            if let Op::Argument(ref delay) = this.stator.states[p_delay_state].op {
                                // the delay should have had `shrink_to_msb` called on it
                                if delay.bw() > 128 {
                                    return Err(Error::OtherStr(
                                        "`Delay` delay amount is unexpectedly large",
                                    ))
                                }
                                if delay.is_zero() {
                                    // the function that creates `Delay` is supposed to do a no-op
                                    // or copy instead
                                    return Err(Error::OtherStr("`Delay` delay amount is zero"))
                                }
                                Delay::from_amount(delay.to_u128())
                            } else {
                                return Err(Error::OtherStr(
                                    "`Delay` does not use the correct `Op::Argument`",
                                ))
                            };
                        if delay.is_zero() {
                            // the function that creates DELAYED_LOOP_SOURCE is supposed to do a
                            // LOOP_SOURCE instead
                            return Err(Error::OtherStr("delayed loop source delay amount is zero"))
                        }
                        for i in 0..w {
                            let p_looper = this.stator.states[p_state].p_self_bits[i].unwrap();
                            let p_driver =
                                this.stator.states[p_driver_state].p_self_bits[i].unwrap();
                            let p_initial =
                                this.stator.states[p_initial_state].p_self_bits[i].unwrap();
                            let init_val = this.backrefs.get_val(p_initial).unwrap().val;

                            let p_tnode = this.make_tnode(p_looper, p_driver, delay);
                            if !delay.is_zero() {
                                // immediately setup an event
                                this.eval_tnode(p_tnode).unwrap();
                            } else {
                                // least priority event for the reason specified in the `LoopSource`
                                // case
                                this.evaluator.push_event(Event {
                                    partial_ord_num: NonZeroU64::MAX,
                                    change_kind: ChangeKind::TNode(p_tnode),
                                });
                            }

                            let init_val = match init_val {
                                Value::ConstUnknown => Value::Unknown,
                                Value::Const(b) => Value::Dynam(b),
                                Value::Unknown | Value::Dynam(_) => {
                                    return Err(Error::OtherStr(
                                        "A `Loop`'s initial value could not be calculated as a \
                                         constant known or constant unknown in lowering, the \
                                         argument to `Loop::from_*` needs to evaluate to a \
                                         constant",
                                    ));
                                }
                            };
                            let p_back = this.backrefs.get_val(p_looper).unwrap().p_self_equiv;
                            this.evaluator.push_event(Event {
                                partial_ord_num: NonZeroU64::new(1).unwrap(),
                                change_kind: ChangeKind::Manual(p_back, init_val),
                            });
                        }
                    }
                    _ => {
                        return Err(Error::OtherString(format!(
                            "cannot lower opaque with name {name:?}"
                        )))
                    }
                }
            }
        }
        ref op => return Err(Error::OtherString(format!("cannot lower {op:?}"))),
    }
    Ok(())
}

impl Default for Stator {
    fn default() -> Self {
        Self::new()
    }
}
