use std::num::NonZeroU64;

use awint::awint_dag::{
    triple_arena::{Recast, Recaster},
    PState,
};

use super::Delayer;
use crate::{
    ensemble::{
        value::Evaluator, LNode, LNodeKind, Notary, Optimizer, PLNode, PRNode, PTNode, Stator,
        TNode, Value,
    },
    triple_arena::{ptr_struct, Arena, SurjectArena},
    Error,
};

ptr_struct!(PBack);

#[derive(Debug, Clone)]
pub struct Equiv {
    /// `Ptr` back to this equivalence through a `Referent::ThisEquiv` in the
    /// backref surject associated with this `Equiv`
    pub p_self_equiv: PBack,
    /// Output of the equivalence surject
    pub val: Value,
    /// Used by the evaluator
    pub evaluator_partial_order: NonZeroU64,
}

impl Recast<PBack> for Equiv {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_self_equiv.recast(recaster)
    }
}

impl Equiv {
    pub fn new(p_self_equiv: PBack, val: Value) -> Self {
        Self {
            p_self_equiv,
            val,
            evaluator_partial_order: NonZeroU64::new(1).unwrap(),
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
    /// Referent is using this for a driver of a `TNode`
    Driver(PTNode),
    /// Referent is an `RNode`
    ThisRNode(PRNode),
}

impl Recast<PBack> for Referent {
    fn recast<R: Recaster<Item = PBack>>(&mut self, _: &R) -> Result<(), <R as Recaster>::Item> {
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Ensemble {
    pub backrefs: SurjectArena<PBack, Referent, Equiv>,
    pub notary: Notary,
    pub stator: Stator,
    pub lnodes: Arena<PLNode, LNode>,
    pub tnodes: Arena<PTNode, TNode>,
    pub evaluator: Evaluator,
    pub delayer: Delayer,
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
            delayer: Delayer::new(),
            optimizer: Optimizer::new(),
            debug_counter: 0,
        }
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
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
                    return Err(Error::OtherString(format!(
                        "{equiv:?}.p_self_equiv roundtrip fail"
                    )))
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{equiv:?}.p_self_equiv is invalid"
                )))
            }
            // need to roundtrip in both directions to ensure existence and uniqueness of a
            // `ThisEquiv` for each equivalence surject
            if let Some(Referent::ThisEquiv) = self.backrefs.get_key(p_back) {
                if p_back != equiv.p_self_equiv {
                    return Err(Error::OtherString(format!(
                        "{equiv:?}.p_self_equiv roundtrip fail"
                    )))
                }
            }
        }
        // check other kinds of self refs
        for (p_state, state) in &self.stator.states {
            if (!state.p_self_bits.is_empty()) && (state.nzbw.get() != state.p_self_bits.len()) {
                return Err(Error::OtherString(format!(
                    "{state:?}.nzbw mismatch with p_self_bits.len"
                )))
            }
            for operand in state.op.operands() {
                if !self.stator.states.contains(*operand) {
                    return Err(Error::OtherString(format!("{state:?} operand is missing")))
                }
            }
            for (inx, p_self_bit) in state.p_self_bits.iter().copied().enumerate() {
                if let Some(p_self_bit) = p_self_bit {
                    if let Some(Referent::ThisStateBit(p_self, inx_self)) =
                        self.backrefs.get_key(p_self_bit).copied()
                    {
                        if (p_state != p_self) || (inx != inx_self) {
                            return Err(Error::OtherString(format!(
                                "{state:?}.p_self_bits roundtrip fail"
                            )))
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "{state:?}.p_self_bits is invalid"
                        )))
                    }
                }
            }
        }
        for (p_lnode, lnode) in &self.lnodes {
            if let Some(Referent::ThisLNode(p_self)) = self.backrefs.get_key(lnode.p_self).copied()
            {
                if p_lnode != p_self {
                    return Err(Error::OtherString(format!(
                        "{lnode:?}.p_self roundtrip fail"
                    )))
                }
            } else {
                return Err(Error::OtherString(format!("{lnode:?}.p_self is invalid")))
            }
        }
        for (p_tnode, tnode) in &self.tnodes {
            if let Some(Referent::ThisTNode(p_self)) = self.backrefs.get_key(tnode.p_self).copied()
            {
                if p_tnode != p_self {
                    return Err(Error::OtherString(format!(
                        "{tnode:?}.p_self roundtrip fail"
                    )))
                }
            } else {
                return Err(Error::OtherString(format!("{tnode:?}.p_self is invalid")))
            }
        }
        // check other referent validities
        for referent in self.backrefs.keys().copied() {
            let invalid = match referent {
                // already checked
                Referent::ThisEquiv => false,
                Referent::ThisLNode(_) => false,
                Referent::ThisTNode(_) => false,
                Referent::ThisStateBit(..) => false,
                Referent::Input(p_input) => !self.lnodes.contains(p_input),
                Referent::Driver(p_driver) => !self.tnodes.contains(p_driver),
                Referent::ThisRNode(p_rnode) => !self.notary.rnodes().contains(p_rnode),
            };
            if invalid {
                return Err(Error::OtherString(format!("{referent:?} is invalid")))
            }
        }
        // other kinds of validity
        for p_lnode in self.lnodes.ptrs() {
            let lnode = self.lnodes.get(p_lnode).unwrap();
            let mut res = Ok(());
            lnode.inputs(|p_input| {
                if let Some(referent) = self.backrefs.get_key(p_input) {
                    if let Referent::Input(referent) = referent {
                        if !self.lnodes.contains(*referent) {
                            res = Err(Error::OtherString(format!(
                                "{p_lnode}: {lnode:?} input {p_input} referrent {referent} is \
                                 invalid"
                            )));
                        }
                    } else {
                        res = Err(Error::OtherString(format!(
                            "{p_lnode}: {lnode:?} input {p_input} has incorrect referrent"
                        )));
                    }
                } else {
                    res = Err(Error::OtherString(format!(
                        "{p_lnode}: {lnode:?} input {p_input} is invalid"
                    )));
                }
            });
            res?;
        }
        for p_tnode in self.tnodes.ptrs() {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            if let Some(referent) = self.backrefs.get_key(tnode.p_driver).copied() {
                if let Referent::Driver(p_driver) = referent {
                    if !self.tnodes.contains(p_driver) {
                        return Err(Error::OtherString(format!(
                            "{p_tnode}: {tnode:?} driver referrent {p_driver} is invalid"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_tnode}: {tnode:?} driver has incorrect referrent"
                    )))
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{p_tnode}: {tnode:?} driver {} is invalid",
                    tnode.p_driver
                )))
            }
        }
        for rnode in self.notary.rnodes().vals() {
            if let Some(bits) = rnode.bits() {
                for p_back in bits.iter().copied() {
                    if let Some(p_back) = p_back {
                        if let Some(referent) = self.backrefs.get_key(p_back).copied() {
                            if let Referent::ThisRNode(p_rnode) = referent {
                                if !self.notary.rnodes().contains(p_rnode) {
                                    return Err(Error::OtherString(format!(
                                        "{rnode:?} backref {p_rnode} is invalid"
                                    )))
                                }
                            } else {
                                return Err(Error::OtherString(format!(
                                    "{rnode:?} backref {p_back} has incorrect referrent"
                                )))
                            }
                        } else {
                            return Err(Error::OtherString(format!("rnode {p_back} is invalid")))
                        }
                    }
                }
            }
        }
        // Other roundtrips from `backrefs` direction to ensure bijection
        for p_back in self.backrefs.ptrs() {
            let referent = self.backrefs.get_key(p_back).unwrap();
            let fail = match *referent {
                // already checked
                Referent::ThisEquiv => false,
                Referent::ThisLNode(p_lnode) => {
                    let lnode = self.lnodes.get(p_lnode).unwrap();
                    p_back != lnode.p_self
                }
                Referent::ThisTNode(p_tnode) => {
                    let tnode = self.tnodes.get(p_tnode).unwrap();
                    p_back != tnode.p_self
                }
                Referent::ThisStateBit(p_state, inx) => {
                    let state = self.stator.states.get(p_state).unwrap();
                    let p_bit = state.p_self_bits.get(inx).copied().unwrap();
                    if let Some(p_bit) = p_bit {
                        p_bit != p_back
                    } else {
                        true
                    }
                }
                Referent::Input(p_input) => {
                    let lnode = self.lnodes.get(p_input).unwrap();
                    let mut found = false;
                    lnode.inputs(|p_back1| {
                        if p_back1 == p_back {
                            found = true;
                        }
                    });
                    !found
                }
                Referent::Driver(p_tnode) => {
                    let tnode = self.tnodes.get(p_tnode).unwrap();
                    tnode.p_driver != p_back
                }
                Referent::ThisRNode(p_rnode) => {
                    let rnode = self.notary.rnodes().get_val(p_rnode).unwrap();
                    let mut found = false;
                    if let Some(bits) = rnode.bits() {
                        for bit in bits {
                            if *bit == Some(p_back) {
                                found = true;
                                break
                            }
                        }
                    }
                    !found
                }
            };
            if fail {
                return Err(Error::OtherString(format!("{referent:?} roundtrip fail")))
            }
        }
        // non-pointer invariants
        for lnode in self.lnodes.vals() {
            match &lnode.kind {
                LNodeKind::Copy(_) => (),
                LNodeKind::Lut(inp, lut) => {
                    if inp.is_empty() {
                        return Err(Error::OtherStr("no inputs for lookup table"))
                    }
                    if !lut.bw().is_power_of_two() {
                        return Err(Error::OtherStr(
                            "lookup table is not a power of two in bitwidth",
                        ))
                    }
                    if (lut.bw().trailing_zeros() as usize) != inp.len() {
                        return Err(Error::OtherStr(
                            "number of inputs does not correspond to lookup table size",
                        ))
                    }
                }
                LNodeKind::DynamicLut(inp, lut) => {
                    if inp.is_empty() {
                        return Err(Error::OtherStr("no inputs for lookup table"))
                    }
                    if !lut.len().is_power_of_two() {
                        return Err(Error::OtherStr(
                            "lookup table is not a power of two in bitwidth",
                        ))
                    }
                    if (lut.len().trailing_zeros() as usize) != inp.len() {
                        return Err(Error::OtherStr(
                            "number of inputs does not correspond to lookup table size",
                        ))
                    }
                }
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
                return Err(Error::OtherStr(
                    "{p_state} {state:?} reference count mismatch",
                ))
            }
        }

        Ok(())
    }

    /// Compresses and shrinks all internal `Ptr`s. Returns an error if the
    /// optimizer, evaluator, or stator are not empty.
    pub fn recast_all_internal_ptrs(&mut self) -> Result<(), Error> {
        self.optimizer.check_clear()?;
        self.evaluator.check_clear()?;
        self.stator.check_clear()?;

        self.delayer.compress();
        let p_tnode_recaster = self.tnodes.compress_and_shrink_recaster();
        if let Err(e) = self.delayer.recast(&p_tnode_recaster) {
            return Err(Error::OtherString(format!(
                "recast error with {e} in the `Delayer`"
            )));
        }

        let p_lnode_recaster = self.lnodes.compress_and_shrink_recaster();
        let p_rnode_recaster = self.notary.recast_p_rnode();

        for referent in self.backrefs.keys_mut() {
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisLNode(p_lnode) => {
                    if let Err(e) = p_lnode.recast(&p_lnode_recaster) {
                        return Err(Error::OtherString(format!(
                            "recast error with {e} in a `Referent::ThisLNode`"
                        )));
                    }
                }
                Referent::ThisTNode(p_tnode) => {
                    if let Err(e) = p_tnode.recast(&p_tnode_recaster) {
                        return Err(Error::OtherString(format!(
                            "recast error with {e} in a `Referent::ThisTNode`"
                        )));
                    }
                }
                Referent::ThisStateBit(..) => unreachable!(),
                Referent::Input(p_lnode) => {
                    if let Err(e) = p_lnode.recast(&p_lnode_recaster) {
                        return Err(Error::OtherString(format!(
                            "recast error with {e} in a `Referent::Input`"
                        )));
                    }
                }
                Referent::Driver(p_tnode) => {
                    if let Err(e) = p_tnode.recast(&p_tnode_recaster) {
                        return Err(Error::OtherString(format!(
                            "recast error with {e} in a `Referent::Driver`"
                        )));
                    }
                }
                Referent::ThisRNode(p_rnode) => {
                    if let Err(e) = p_rnode.recast(&p_rnode_recaster) {
                        return Err(Error::OtherString(format!(
                            "recast error with {e} in a `Referent::ThisRNode`"
                        )));
                    }
                }
            }
        }

        let p_back_recaster = self.backrefs.compress_and_shrink_recaster();
        if let Err(e) = self.backrefs.recast(&p_back_recaster) {
            return Err(Error::OtherString(format!(
                "recast error with {e} in the backrefs"
            )));
        }
        if let Err(e) = self.notary.recast(&p_back_recaster) {
            return Err(Error::OtherString(format!(
                "recast error with {e} in the notary"
            )));
        }
        if let Err(e) = self.lnodes.recast(&p_back_recaster) {
            return Err(Error::OtherString(format!(
                "recast error with {e} in the lnodes"
            )));
        }
        if let Err(e) = self.tnodes.recast(&p_back_recaster) {
            return Err(Error::OtherString(format!(
                "recast error with {e} in the tnodes"
            )));
        }
        Ok(())
    }

    /// Inserts a `LNode` with `lit` value and returns a `PBack` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PBack {
        self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, {
                    if let Some(b) = lit {
                        Value::Const(b)
                    } else {
                        Value::Unknown
                    }
                }),
            )
        })
    }

    pub fn union_equiv(&mut self, p_equiv0: PBack, p_equiv1: PBack) -> Result<(), Error> {
        let (equiv0, equiv1) = self.backrefs.get2_val_mut(p_equiv0, p_equiv1).unwrap();
        if (equiv0.val.is_const() && equiv1.val.is_const()) && (equiv0.val != equiv1.val) {
            panic!("tried to merge two const equivalences with differing values");
        }
        if equiv0.val != equiv1.val {
            if !equiv0.val.is_known() {
                equiv0.val = equiv1.val;
            } else if !equiv1.val.is_known() {
                equiv1.val = equiv0.val;
            } else {
                return Err(Error::OtherString(format!(
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

    pub fn inc_debug_counter(&mut self) {
        self.debug_counter = self.debug_counter.checked_add(1).unwrap()
    }
}

impl Default for Ensemble {
    fn default() -> Self {
        Self::new()
    }
}
