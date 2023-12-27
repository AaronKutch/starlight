use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        EvalError, Location, Op, PState,
    },
    Awi, Bits,
};

use crate::{
    ensemble::{
        value::Evaluator, LNode, Notary, Optimizer, PLNode, PRNode, PTNode, State, Stator, TNode,
        Value,
    },
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
    /// Self referent, used by all the `LNode`s of an equivalence class
    ThisLNode(PLNode),
    /// Self referent for an `TNode`
    ThisTNode(PTNode),
    /// Self referent to a particular bit of a `State`
    ThisStateBit(PState, usize),
    /// Referent is using this for registering an input dependency
    Input(PLNode),
    /// Referent is using this for a loop driver
    LoopDriver(PTNode),
    /// Referent is an `RNode`
    ThisRNode(PRNode),
}

#[derive(Debug, Clone)]
pub struct Ensemble {
    pub backrefs: SurjectArena<PBack, Referent, Equiv>,
    pub notary: Notary,
    pub stator: Stator,
    pub lnodes: Arena<PLNode, LNode>,
    pub tnodes: Arena<PTNode, TNode>,
    pub evaluator: Evaluator,
    pub optimizer: Optimizer,
    pub debug_counter: u64,
}

impl Ensemble {
    pub fn new() -> Self {
        Self {
            backrefs: SurjectArena::new(),
            notary: Notary::new(),
            stator: Stator::new(),
            lnodes: Arena::new(),
            tnodes: Arena::new(),
            evaluator: Evaluator::new(),
            optimizer: Optimizer::new(),
            debug_counter: 0,
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
        for (p_lnode, lnode) in &self.lnodes {
            if let Some(Referent::ThisLNode(p_self)) = self.backrefs.get_key(lnode.p_self) {
                if p_lnode != *p_self {
                    return Err(EvalError::OtherString(format!(
                        "{lnode:?}.p_self roundtrip fail"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{lnode:?}.p_self is invalid"
                )))
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
                Referent::ThisLNode(_) => false,
                Referent::ThisTNode(_) => false,
                Referent::ThisStateBit(..) => false,
                Referent::Input(p_input) => !self.lnodes.contains(*p_input),
                Referent::LoopDriver(p_driver) => !self.tnodes.contains(*p_driver),
                Referent::ThisRNode(p_rnode) => !self.notary.rnodes().contains(*p_rnode),
            };
            if invalid {
                return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
            }
        }
        // other kinds of validity
        for p_lnode in self.lnodes.ptrs() {
            let lnode = self.lnodes.get(p_lnode).unwrap();
            for p_input in &lnode.inp {
                if let Some(referent) = self.backrefs.get_key(*p_input) {
                    if let Referent::Input(referent) = referent {
                        if !self.lnodes.contains(*referent) {
                            return Err(EvalError::OtherString(format!(
                                "{p_lnode}: {lnode:?} input {p_input} referrent {referent} is \
                                 invalid"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{p_lnode}: {lnode:?} input {p_input} has incorrect referrent"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_lnode}: {lnode:?} input {p_input} is invalid"
                    )))
                }
            }
        }
        for p_tnode in self.tnodes.ptrs() {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            if let Some(referent) = self.backrefs.get_key(tnode.p_driver) {
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
                    "{p_tnode}: {tnode:?} loop driver {} is invalid",
                    tnode.p_driver
                )))
            }
        }
        for rnode in self.notary.rnodes().vals() {
            for p_back in &rnode.bits {
                if let Some(p_back) = p_back {
                    if let Some(referent) = self.backrefs.get_key(*p_back) {
                        if let Referent::ThisRNode(p_rnode) = referent {
                            if !self.notary.rnodes().contains(*p_rnode) {
                                return Err(EvalError::OtherString(format!(
                                    "{rnode:?} backref {p_rnode} is invalid"
                                )))
                            }
                        } else {
                            return Err(EvalError::OtherString(format!(
                                "{rnode:?} backref {p_back} has incorrect referrent"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!("rnode {p_back} is invalid")))
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
                Referent::ThisLNode(p_lnode) => {
                    let lnode = self.lnodes.get(*p_lnode).unwrap();
                    p_back != lnode.p_self
                }
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
                    let lnode = self.lnodes.get(*p_input).unwrap();
                    let mut found = false;
                    for p_back1 in &lnode.inp {
                        if *p_back1 == p_back {
                            found = true;
                            break
                        }
                    }
                    !found
                }
                Referent::LoopDriver(p_tnode) => {
                    let tnode = self.tnodes.get(*p_tnode).unwrap();
                    tnode.p_driver != p_back
                }
                Referent::ThisRNode(p_rnode) => {
                    let rnode = self.notary.rnodes().get_val(*p_rnode).unwrap();
                    let mut found = false;
                    for bit in &rnode.bits {
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
        for lnode in self.lnodes.vals() {
            if let Some(ref lut) = lnode.lut {
                if lnode.inp.is_empty() {
                    return Err(EvalError::OtherStr("no inputs for lookup table"))
                }
                if !lut.bw().is_power_of_two() {
                    return Err(EvalError::OtherStr(
                        "lookup table is not a power of two in bitwidth",
                    ))
                }
                if (lut.bw().trailing_zeros() as usize) != lnode.inp.len() {
                    return Err(EvalError::OtherStr(
                        "number of inputs does not correspond to lookup table size",
                    ))
                }
            } else if lnode.inp.len() != 1 {
                return Err(EvalError::OtherStr(
                    "`LNode` with no lookup table has more or less than one input",
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

        Ok(())
    }

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

    /// Inserts a `LNode` with `lit` value and returns a `PBack` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PBack {
        self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::from_dag_lit(lit)),
            )
        })
    }

    /// Makes a single output bit lookup table `LNode` and returns a `PBack` to
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
        self.lnodes.insert_with(|p_lnode| {
            let p_self = self
                .backrefs
                .insert_key(p_equiv, Referent::ThisLNode(p_lnode))
                .unwrap();
            let mut lnode = LNode::new(p_self, lowered_from);
            lnode.lut = Some(Awi::from(table));
            for p_inx in p_inxs {
                let p_back = self
                    .backrefs
                    .insert_key(p_inx.unwrap(), Referent::Input(p_lnode))
                    .unwrap();
                lnode.inp.push(p_back);
            }
            lnode
        });
        Some(p_equiv)
    }

    /// Sets up a loop from the loop source `p_looper` and driver `p_driver`
    #[must_use]
    pub fn make_loop(
        &mut self,
        p_looper: PBack,
        p_driver: PBack,
        init_val: Value,
    ) -> Option<PTNode> {
        let p_tnode = self.tnodes.insert_with(|p_tnode| {
            let p_driver = self
                .backrefs
                .insert_key(p_driver, Referent::LoopDriver(p_tnode))
                .unwrap();
            let p_self = self
                .backrefs
                .insert_key(p_looper, Referent::ThisTNode(p_tnode))
                .unwrap();
            TNode::new(p_self, p_driver)
        });
        // in order for the value to register correctly
        self.change_value(p_looper, init_val).unwrap();
        Some(p_tnode)
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

    /// Triggers a cascade of state removals if `pruning_allowed()` and
    /// their reference counts are zero
    pub fn remove_state(&mut self, p_state: PState) -> Result<(), EvalError> {
        if !self.stator.states.contains(p_state) {
            return Err(EvalError::InvalidPtr);
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

    pub fn force_remove_all_states(&mut self) -> Result<(), EvalError> {
        for (_, mut state) in self.stator.states.drain() {
            for p_self_state in state.p_self_bits.drain(..) {
                if let Some(p_self_state) = p_self_state {
                    self.backrefs.remove_key(p_self_state).unwrap();
                }
            }
        }
        Ok(())
    }

    pub fn inc_debug_counter(&mut self) {
        self.debug_counter = self.debug_counter.checked_add(1).unwrap()
    }
}

impl Default for Ensemble {
    fn default() -> Self {
        Self::new()
    }
}
