use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        EvalError, Location, Op, PNote, PState,
    },
    awint_macro_internals::triple_arena::Advancer,
    Awi, Bits,
};

use super::{value::Evaluator, Optimizer, Stator};
use crate::{
    ensemble::{Note, PTNode, State, TNode, Value},
    triple_arena::{ptr_struct, Arena, SurjectArena},
};

ptr_struct!(PBack);

#[derive(Debug, Clone)]
pub struct Equiv {
    /// `Ptr` back to this equivalence through a `Referent::ThisEquiv` in the
    /// backref surject associated with this `Equiv`
    pub p_self_equiv: PBack,
    /// Output of the equivalence surject
    pub val: Value,
    pub change_visit: NonZeroU64,
    pub request_visit: NonZeroU64,
}

impl Equiv {
    pub fn new(p_self_equiv: PBack, val: Value) -> Self {
        Self {
            p_self_equiv,
            val,
            change_visit: NonZeroU64::new(1).unwrap(),
            request_visit: NonZeroU64::new(1).unwrap(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Referent {
    /// Self equivalence class referent
    ThisEquiv,
    /// Self referent, used by all the `Tnode`s of an equivalence class
    ThisTNode(PTNode),
    /// Self referent to a particular bit of a `State`
    ThisStateBit(PState, usize),
    /// Referent is using this for registering an input dependency
    Input(PTNode),
    LoopDriver(PTNode),
    /// Referent is a note
    Note(PNote),
}

#[derive(Debug, Clone)]
pub struct Ensemble {
    pub backrefs: SurjectArena<PBack, Referent, Equiv>,
    pub notes: Arena<PNote, Note>,
    pub stator: Stator,
    pub tnodes: Arena<PTNode, TNode>,
    pub evaluator: Evaluator,
    pub optimizer: Optimizer,
}

impl Ensemble {
    pub fn new() -> Self {
        Self {
            backrefs: SurjectArena::new(),
            notes: Arena::new(),
            stator: Stator::new(),
            tnodes: Arena::new(),
            evaluator: Evaluator::new(),
            optimizer: Optimizer::new(),
        }
    }

    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        // return errors in order of most likely to be root cause

        // first check that equivalences aren't broken by themselves
        for p_back in self.backrefs.ptrs() {
            let equiv = self.backrefs.get_val(p_back).unwrap();
            if let Some(Referent::ThisEquiv) = self.backrefs.get_key(equiv.p_self_equiv) {
                if !self
                    .backrefs
                    .in_same_set(p_back, equiv.p_self_equiv)
                    .unwrap()
                {
                    return Err(EvalError::OtherString(format!(
                        "{equiv:?}.p_self_equiv roundtrip fail"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{equiv:?}.p_self_equiv is invalid"
                )))
            }
            // need to roundtrip in both directions to ensure existence and uniqueness of a
            // `ThisEquiv` for each equivalence surject
            if let Some(Referent::ThisEquiv) = self.backrefs.get_key(p_back) {
                if p_back != equiv.p_self_equiv {
                    return Err(EvalError::OtherString(format!(
                        "{equiv:?}.p_self_equiv roundtrip fail"
                    )))
                }
            }
        }
        // check other kinds of self refs
        for (p_state, state) in &self.stator.states {
            if (!state.p_self_bits.is_empty()) && (state.nzbw.get() != state.p_self_bits.len()) {
                return Err(EvalError::OtherString(format!(
                    "{state:?}.nzbw mismatch with p_self_bits.len"
                )))
            }
            for operand in state.op.operands() {
                if !self.stator.states.contains(*operand) {
                    return Err(EvalError::OtherString(format!(
                        "{state:?} operand is missing"
                    )))
                }
            }
            for (inx, p_self_bit) in state.p_self_bits.iter().enumerate() {
                if let Some(p_self_bit) = p_self_bit {
                    if let Some(Referent::ThisStateBit(p_self, inx_self)) =
                        self.backrefs.get_key(*p_self_bit)
                    {
                        if (p_state != *p_self) || (inx != *inx_self) {
                            return Err(EvalError::OtherString(format!(
                                "{state:?}.p_self_bits roundtrip fail"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{state:?}.p_self_bits is invalid"
                        )))
                    }
                }
            }
        }
        for (p_tnode, tnode) in &self.tnodes {
            if let Some(Referent::ThisTNode(p_self)) = self.backrefs.get_key(tnode.p_self) {
                if p_tnode != *p_self {
                    return Err(EvalError::OtherString(format!(
                        "{tnode:?}.p_self roundtrip fail"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{tnode:?}.p_self is invalid"
                )))
            }
        }
        // check other referent validities
        for referent in self.backrefs.keys() {
            let invalid = match referent {
                // already checked
                Referent::ThisEquiv => false,
                Referent::ThisTNode(_) => false,
                Referent::ThisStateBit(..) => false,
                Referent::Input(p_input) => !self.tnodes.contains(*p_input),
                Referent::LoopDriver(p_driver) => !self.tnodes.contains(*p_driver),
                Referent::Note(p_note) => !self.notes.contains(*p_note),
            };
            if invalid {
                return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
            }
        }
        // other kinds of validity
        for p_tnode in self.tnodes.ptrs() {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            for p_input in &tnode.inp {
                if let Some(referent) = self.backrefs.get_key(*p_input) {
                    if let Referent::Input(referent) = referent {
                        if !self.tnodes.contains(*referent) {
                            return Err(EvalError::OtherString(format!(
                                "{p_tnode}: {tnode:?} input {p_input} referrent {referent} is \
                                 invalid"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{p_tnode}: {tnode:?} input {p_input} has incorrect referrent"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} input {p_input} is invalid"
                    )))
                }
            }
            if let Some(loop_driver) = tnode.loop_driver {
                if let Some(referent) = self.backrefs.get_key(loop_driver) {
                    if let Referent::LoopDriver(p_driver) = referent {
                        if !self.tnodes.contains(*p_driver) {
                            return Err(EvalError::OtherString(format!(
                                "{p_tnode}: {tnode:?} loop driver referrent {p_driver} is invalid"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{p_tnode}: {tnode:?} loop driver has incorrect referrent"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} loop driver {loop_driver} is invalid"
                    )))
                }
            }
        }
        for note in self.notes.vals() {
            for p_back in &note.bits {
                if let Some(p_back) = p_back {
                    if let Some(referent) = self.backrefs.get_key(*p_back) {
                        if let Referent::Note(p_note) = referent {
                            if !self.notes.contains(*p_note) {
                                return Err(EvalError::OtherString(format!(
                                    "{note:?} backref {p_note} is invalid"
                                )))
                            }
                        } else {
                            return Err(EvalError::OtherString(format!(
                                "{note:?} backref {p_back} has incorrect referrent"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!("note {p_back} is invalid")))
                    }
                }
            }
        }
        // Other roundtrips from `backrefs` direction to ensure bijection
        for p_back in self.backrefs.ptrs() {
            let referent = self.backrefs.get_key(p_back).unwrap();
            let fail = match referent {
                // already checked
                Referent::ThisEquiv => false,
                Referent::ThisTNode(p_tnode) => {
                    let tnode = self.tnodes.get(*p_tnode).unwrap();
                    p_back != tnode.p_self
                }
                Referent::ThisStateBit(p_state, inx) => {
                    let state = self.stator.states.get(*p_state).unwrap();
                    let p_bit = state.p_self_bits.get(*inx).unwrap();
                    if let Some(p_bit) = p_bit {
                        *p_bit != p_back
                    } else {
                        true
                    }
                }
                Referent::Input(p_input) => {
                    let tnode1 = self.tnodes.get(*p_input).unwrap();
                    let mut found = false;
                    for p_back1 in &tnode1.inp {
                        if *p_back1 == p_back {
                            found = true;
                            break
                        }
                    }
                    !found
                }
                Referent::LoopDriver(p_loop) => {
                    let tnode1 = self.tnodes.get(*p_loop).unwrap();
                    tnode1.loop_driver != Some(p_back)
                }
                Referent::Note(p_note) => {
                    let note = self.notes.get(*p_note).unwrap();
                    let mut found = false;
                    for bit in &note.bits {
                        if *bit == Some(p_back) {
                            found = true;
                            break
                        }
                    }
                    !found
                }
            };
            if fail {
                return Err(EvalError::OtherString(format!(
                    "{referent:?} roundtrip fail"
                )))
            }
        }
        // non-pointer invariants
        for tnode in self.tnodes.vals() {
            if let Some(ref lut) = tnode.lut {
                if tnode.inp.is_empty() {
                    return Err(EvalError::OtherStr("no inputs for lookup table"))
                }
                if !lut.bw().is_power_of_two() {
                    return Err(EvalError::OtherStr(
                        "lookup table is not a power of two in bitwidth",
                    ))
                }
                if (lut.bw().trailing_zeros() as usize) != tnode.inp.len() {
                    return Err(EvalError::OtherStr(
                        "number of inputs does not correspond to lookup table size",
                    ))
                }
            } else if tnode.inp.len() != 1 {
                return Err(EvalError::OtherStr(
                    "`TNode` with no lookup table has more or less than one input",
                ))
            }
        }
        // state reference counts
        let mut counts = Arena::<PState, usize>::new();
        counts.clone_from_with(&self.stator.states, |_, _| 0);
        for state in self.stator.states.vals() {
            for operand in state.op.operands() {
                counts[*operand] = counts[operand].checked_add(1).unwrap();
            }
        }
        for (p_state, state) in &self.stator.states {
            if state.rc != counts[p_state] {
                return Err(EvalError::OtherStr(
                    "{p_state} {state:?} reference count mismatch",
                ))
            }
        }

        // TODO verify DAGness
        Ok(())
    }

    pub fn make_state(
        &mut self,
        nzbw: NonZeroUsize,
        op: Op<PState>,
        location: Option<Location>,
        keep: bool,
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
            keep,
            lowered_to_elementary: false,
            lowered_to_tnodes: false,
        })
    }

    /// If `p_state_bits.is_empty`, this will create new equivalences and
    /// `Referent::ThisStateBits`s needed for every self bit. Sets the values to
    /// a constant if the `Op` is a `Literal`, otherwise sets to unknown.
    #[must_use]
    pub fn initialize_state_bits_if_needed(&mut self, p_state: PState) -> Option<()> {
        let state = self.stator.states.get(p_state)?;
        if !state.p_self_bits.is_empty() {
            return Some(())
        }
        let mut bits = smallvec![];
        for i in 0..state.nzbw.get() {
            let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
                (
                    Referent::ThisEquiv,
                    Equiv::new(p_self_equiv, match state.op {
                        Op::Literal(ref awi) => Value::Const(awi.get(i).unwrap()),
                        _ => Value::Unknown,
                    }),
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
        Some(())
    }

    /// Inserts a `TNode` with `lit` value and returns a `PBack` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PBack {
        self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::from_dag_lit(lit)),
            )
        })
    }

    /// Makes a single output bit lookup table `TNode` and returns a `PBack` to
    /// it. Returns `None` if the table length is incorrect or any of the
    /// `p_inxs` are invalid.
    #[must_use]
    pub fn make_lut(
        &mut self,
        p_inxs: &[Option<PBack>],
        table: &Bits,
        lowered_from: Option<PState>,
    ) -> Option<PBack> {
        let num_entries = 1 << p_inxs.len();
        if table.bw() != num_entries {
            return None
        }
        for p_inx in p_inxs {
            if let Some(p_inx) = p_inx {
                if !self.backrefs.contains(*p_inx) {
                    return None
                }
            }
        }
        let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::Unknown),
            )
        });
        self.tnodes.insert_with(|p_tnode| {
            let p_self = self
                .backrefs
                .insert_key(p_equiv, Referent::ThisTNode(p_tnode))
                .unwrap();
            let mut tnode = TNode::new(p_self, lowered_from);
            tnode.lut = Some(Awi::from(table));
            for p_inx in p_inxs {
                let p_back = self
                    .backrefs
                    .insert_key(p_inx.unwrap(), Referent::Input(p_tnode))
                    .unwrap();
                tnode.inp.push(p_back);
            }
            tnode
        });
        Some(p_equiv)
    }

    /// Sets up a loop from the loop source `p_looper` and driver `p_driver`
    #[must_use]
    pub fn make_loop(&mut self, p_looper: PBack, p_driver: PBack, init_val: Value) -> Option<()> {
        let looper_equiv = self.backrefs.get_val_mut(p_looper)?;
        match looper_equiv.val {
            Value::Unknown => (),
            // shouldn't fail unless the special Opaque loopback structure is broken
            _ => panic!("looper is already set to a known value"),
        }
        looper_equiv.val = init_val;

        let referent = self.backrefs.get_key(p_looper)?;
        let p_looper_tnode = match referent {
            Referent::ThisEquiv => {
                // need to create the TNode
                self.tnodes.insert_with(|p_tnode| {
                    let p_back_self = self
                        .backrefs
                        .insert_key(p_looper, Referent::ThisTNode(p_tnode))
                        .unwrap();
                    TNode::new(p_back_self, None)
                })
            }
            // we might want to support more cases in the future
            _ => panic!("bad referent {referent:?}"),
        };
        let p_back_driver = self
            .backrefs
            .insert_key(p_driver, Referent::LoopDriver(p_looper_tnode))
            .unwrap();
        let tnode = self.tnodes.get_mut(p_looper_tnode).unwrap();
        tnode.loop_driver = Some(p_back_driver);
        Some(())
    }

    pub fn union_equiv(&mut self, p_equiv0: PBack, p_equiv1: PBack) -> Result<(), EvalError> {
        let (equiv0, equiv1) = self.backrefs.get2_val_mut(p_equiv0, p_equiv1).unwrap();
        if (equiv0.val.is_const() && equiv1.val.is_const()) && (equiv0.val != equiv1.val) {
            panic!("tried to merge two const equivalences with differing values");
        }
        // TODO, not sure about these cases
        if equiv0.change_visit == self.evaluator.change_visit_gen() {
            if equiv1.change_visit == self.evaluator.change_visit_gen() {
                if equiv0.val != equiv1.val {
                    // prevent what is probably some bug
                    panic!();
                }
            } else {
                equiv1.val = equiv0.val;
                equiv1.change_visit = equiv0.change_visit;
                equiv1.val = equiv0.val;
            }
        } else if equiv1.change_visit == self.evaluator.change_visit_gen() {
            equiv0.val = equiv1.val;
            equiv0.change_visit = equiv1.change_visit;
            equiv0.val = equiv1.val;
        } else if equiv0.val != equiv1.val {
            if equiv0.val.is_unknown() {
                equiv0.val = equiv1.val;
            } else if equiv1.val.is_unknown() {
                equiv1.val = equiv0.val;
            } else {
                return Err(EvalError::OtherString(format!(
                    "inconsistent value merging:\n{equiv0:?}\n{equiv1:?}"
                )));
            }
        }
        let (removed_equiv, _) = self.backrefs.union(p_equiv0, p_equiv1).unwrap();
        // remove the extra `ThisEquiv`
        self.backrefs
            .remove_key(removed_equiv.p_self_equiv)
            .unwrap();
        Ok(())
    }

    /// Removes the state (it does not necessarily need to still be contained)
    /// and removes its source tree of states with resulting zero reference
    /// count and `!state.keep`
    pub fn remove_state(&mut self, p_state: PState) -> Result<(), EvalError> {
        let mut pstate_stack = vec![p_state];
        while let Some(p) = pstate_stack.pop() {
            let mut delete = false;
            if let Some(state) = self.stator.states.get(p) {
                if (state.rc == 0) && !state.keep {
                    delete = true;
                }
            }
            if delete {
                for i in 0..self.stator.states[p].op.operands_len() {
                    let op = self.stator.states[p].op.operands()[i];
                    self.stator.states[op].rc =
                        if let Some(x) = self.stator.states[op].rc.checked_sub(1) {
                            x
                        } else {
                            return Err(EvalError::OtherStr("tried to subtract a 0 reference count"))
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

    pub fn drive_loops(&mut self) {
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            if let Some(p_driver) = tnode.loop_driver {
                let driver_equiv = self.backrefs.get_val(p_driver).unwrap();
                let val = driver_equiv.val;
                let looper_equiv = self.backrefs.get_val_mut(tnode.p_self).unwrap();
                looper_equiv.val = val;
            }
        }
    }
}

impl Default for Ensemble {
    fn default() -> Self {
        Self::new()
    }
}
