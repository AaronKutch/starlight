use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awi::*,
    awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena},
};

use crate::{
    ensemble::{Ensemble, LNode, LNodeKind, PBack, PLNode, PTNode, Referent},
    epoch::EpochShared,
    Error,
};

#[derive(Debug, Clone, Copy)]
pub enum BasicValueKind {
    Opaque,
    Zero,
    Umax,
    Imax,
    Imin,
    Uone,
}

/// Used when we need to pass an argument that can multiplex over the basic
/// initial values
#[derive(Debug, Clone, Copy)]
pub struct BasicValue {
    pub kind: BasicValueKind,
    pub nzbw: NonZeroUsize,
}

impl BasicValue {
    pub fn nzbw(&self) -> NonZeroUsize {
        self.nzbw
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn get(&self, inx: usize) -> Option<Option<bool>> {
        if inx >= self.bw() {
            None
        } else {
            Some(match self.kind {
                BasicValueKind::Opaque => None,
                BasicValueKind::Zero => Some(false),
                BasicValueKind::Umax => Some(true),
                BasicValueKind::Imax => Some(inx != (self.bw() - 1)),
                BasicValueKind::Imin => Some(inx == (self.bw() - 1)),
                BasicValueKind::Uone => Some(inx == 0),
            })
        }
    }
}

/// Used when we need to pass an argument that can multiplex over common initial
/// values
#[derive(Debug, Clone)]
pub enum CommonValue<'a> {
    Bits(&'a Bits),
    Basic(BasicValue),
}

impl<'a> CommonValue<'a> {
    pub fn nzbw(&self) -> NonZeroUsize {
        match self {
            CommonValue::Bits(x) => x.nzbw(),
            CommonValue::Basic(basic) => basic.nzbw(),
        }
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn get(&self, inx: usize) -> Option<Option<bool>> {
        match self {
            CommonValue::Bits(bits) => bits.get(inx).map(Some),
            CommonValue::Basic(basic) => basic.get(inx),
        }
    }
}

/// The value of a multistate boolean
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    /// The value is permanently unknown
    ConstUnknown,
    /// The value is simply unknown, or a circuit is undriven
    Unknown,
    /// The value is a known constant that is guaranteed to not change under any
    /// condition
    Const(bool),
    /// The value is known, but may be dynamically changed
    Dynam(bool),
}

impl Value {
    pub fn known_value(self) -> Option<bool> {
        match self {
            Value::ConstUnknown => None,
            Value::Unknown => None,
            Value::Const(b) => Some(b),
            Value::Dynam(b) => Some(b),
        }
    }

    pub fn is_known(self) -> bool {
        match self {
            Value::ConstUnknown | Value::Unknown => false,
            Value::Const(_) | Value::Dynam(_) => true,
        }
    }

    pub fn is_const(self) -> bool {
        match self {
            Value::Unknown | Value::Dynam(_) => false,
            Value::ConstUnknown | Value::Const(_) => true,
        }
    }

    pub fn constified(self) -> Self {
        match self {
            Value::ConstUnknown => self,
            Value::Unknown => Value::ConstUnknown,
            Value::Const(_) => self,
            Value::Dynam(b) => Value::Const(b),
        }
    }
}

/// Used for dealing with mixed values and dynamics
#[derive(Debug, Clone, Copy)]
pub enum DynamicValue {
    /// Corresponds with `Value::Unknown`
    ConstUnknown,
    /// Corresponds with `Value::Const`
    Const(bool),
    Dynam(PBack),
}

/*
Consider a request front where we want to know if the output of a LUT is unable to change and thus
that part of the front can be eliminated

a b
0 0
_____
0 0 | 0
0 1 | 0
1 0 | 1
1 1 | 0
    ___
      0

If `b` changes but `a` stays, the output will not change, so what we can do is explore just `a`
first. If `a` doesn't change the front stops as it should. If `a` does change then when the front
reaches back `b` must then be explored.

We will call the number of inputs that could lead to an early termination number_a
TODO find better name

*/

ptr_struct!(PRequestFront; PEval);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvalPhase {
    Change,
    Request,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestLNode {
    pub depth: i64,
    pub number_a: u8,
    pub p_back_lnode: PBack,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RequestTNode {
    pub depth: i64,
    pub number_a: u8,
    pub p_back_tnode: PBack,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Change {
    pub depth: i64,
    pub p_equiv: PBack,
    pub value: Value,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Eval {
    Investigate0(i64, PBack),
    ChangeLNode(PLNode),
    ChangeTNode(PTNode),
    Change(Change),
    RequestLNode(RequestLNode),
    RequestTNode(RequestTNode),
    /// When we have run out of normal things this will activate lowering
    Investigate1(PBack),
}

#[derive(Debug, Clone)]
pub struct Evaluator {
    phase: EvalPhase,
    change_visit_gen: NonZeroU64,
    request_visit_gen: NonZeroU64,
    evaluations: OrdArena<PEval, Eval, ()>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            phase: EvalPhase::Change,
            change_visit_gen: NonZeroU64::new(2).unwrap(),
            request_visit_gen: NonZeroU64::new(2).unwrap(),
            evaluations: OrdArena::new(),
        }
    }

    /// Checks that there are no remaining evaluations, then shrinks allocations
    pub fn check_clear(&mut self) -> Result<(), Error> {
        if !self.evaluations.is_empty() {
            return Err(Error::OtherStr("evaluations need to be empty"));
        }
        self.evaluations.clear_and_shrink();
        Ok(())
    }

    pub fn change_visit_gen(&self) -> NonZeroU64 {
        self.change_visit_gen
    }

    pub fn next_change_visit_gen(&mut self) -> NonZeroU64 {
        self.change_visit_gen =
            NonZeroU64::new(self.change_visit_gen.get().checked_add(1).unwrap()).unwrap();
        self.change_visit_gen
    }

    pub fn request_visit_gen(&self) -> NonZeroU64 {
        self.request_visit_gen
    }

    pub fn next_request_visit_gen(&mut self) -> NonZeroU64 {
        self.request_visit_gen =
            NonZeroU64::new(self.request_visit_gen.get().checked_add(1).unwrap()).unwrap();
        self.request_visit_gen
    }

    pub fn insert(&mut self, eval_step: Eval) {
        let _ = self.evaluations.insert(eval_step, ());
    }
}

impl Ensemble {
    /// If the returned vector is empty, evaluation was successful, otherwise
    /// what is needed for evaluation is returned
    pub fn try_eval_lnode(&mut self, p_lnode: PLNode, depth: i64) -> Vec<RequestLNode> {
        let mut res = vec![];
        // read current inputs
        let lnode = self.lnodes.get(p_lnode).unwrap();
        let p_equiv = self.backrefs.get_val(lnode.p_self).unwrap().p_self_equiv;
        match &lnode.kind {
            LNodeKind::Copy(p_inp) => {
                let equiv = self.backrefs.get_val(*p_inp).unwrap();
                if equiv.val.is_const() || (equiv.change_visit == self.evaluator.change_visit_gen())
                {
                    self.evaluator.insert(Eval::Change(Change {
                        depth,
                        p_equiv,
                        value: equiv.val,
                    }));
                } else {
                    res.push(RequestLNode {
                        depth: depth - 1,
                        number_a: 0,
                        p_back_lnode: *p_inp,
                    });
                }
            }
            LNodeKind::Lut(inp, original_lut) => {
                let len = u8::try_from(inp.len()).unwrap();
                let len = usize::from(len);
                // the nominal value of the inputs
                let mut inp_val = Awi::zero(NonZeroUsize::new(len).unwrap());
                // corresponding bits are set if the input is either a const value or is
                // already evaluated
                let mut fixed = inp_val.clone();
                // corresponding bits are set if the input is `Value::Unknown`
                let mut unknown = inp_val.clone();
                for i in 0..len {
                    let p_inp = inp[i];
                    let equiv = self.backrefs.get_val(p_inp).unwrap();
                    match equiv.val {
                        Value::ConstUnknown => {
                            fixed.set(i, true).unwrap();
                            unknown.set(i, true).unwrap();
                        }
                        Value::Const(val) => {
                            fixed.set(i, true).unwrap();
                            inp_val.set(i, val).unwrap();
                        }
                        Value::Unknown => {
                            if equiv.change_visit == self.evaluator.change_visit_gen() {
                                fixed.set(i, true).unwrap();
                                unknown.set(i, true).unwrap();
                            }
                        }
                        Value::Dynam(val) => {
                            if equiv.change_visit == self.evaluator.change_visit_gen() {
                                fixed.set(i, true).unwrap();
                                inp_val.set(i, val).unwrap()
                            }
                        }
                    }
                }
                let mut lut = original_lut.clone();

                // In an earlier version there was a test where if fixed and unknown bits
                // influence the value, then the equiv can be fixed to unknown. However,
                // consider a lookup table 0100 and an inputs (fixed unknown, 0), where the 0
                // may be a later fixed value. It will see that the output value is dependent on
                // the fixed unknown, but misses that the 0 reduces the LUT to 00 which is
                // independent of the fixed value. We have to wait for all bits to be fixed,
                // reduce the LUT on known bits, then only change to unknown if the
                // `lut.is_zero()` and `lut.is_umax` checks fail. The only tables where you
                // could safely set to unknown earlier are unoptimized.

                // reduce the LUT based on fixed and known bits
                for i in (0..len).rev() {
                    if fixed.get(i).unwrap() && (!unknown.get(i).unwrap()) {
                        LNode::reduce_lut(&mut lut, i, inp_val.get(i).unwrap());
                    }
                }

                // if the LUT is all ones or all zeros, we can know that any unfixed or
                // unknown changes will be unable to affect the
                // output
                if lut.is_zero() {
                    self.evaluator.insert(Eval::Change(Change {
                        depth,
                        p_equiv,
                        value: Value::Dynam(false),
                    }));
                    return vec![];
                } else if lut.is_umax() {
                    self.evaluator.insert(Eval::Change(Change {
                        depth,
                        p_equiv,
                        value: Value::Dynam(true),
                    }));
                    return vec![];
                }
                if fixed.is_umax() {
                    // there are fixed unknown bits influencing the value
                    self.evaluator.insert(Eval::Change(Change {
                        depth,
                        p_equiv,
                        value: Value::Unknown,
                    }));
                    return vec![];
                }
                // TODO prioritize bits that could lead to number_a optimization

                // note: we can do this because `fixed` and `unknown` were not reduced along
                // with the test `lut`
                for i in (0..inp.len()).rev() {
                    if (!fixed.get(i).unwrap()) || unknown.get(i).unwrap() {
                        res.push(RequestLNode {
                            depth: depth - 1,
                            number_a: 0,
                            p_back_lnode: inp[i],
                        });
                    }
                }
            }
            LNodeKind::DynamicLut(inp, original_lut) => {
                let len = u8::try_from(inp.len()).unwrap();
                let mut len = usize::from(len);
                // the nominal value of the inputs
                let mut inp_val = Awi::zero(NonZeroUsize::new(len).unwrap());
                // corresponding bits are set if the input is either a const value or is
                // already evaluated
                let mut fixed = inp_val.clone();
                // corresponding bits are set if the input is `Value::Unknown`
                let mut unknown = inp_val.clone();
                for i in 0..len {
                    let p_inp = inp[i];
                    let equiv = self.backrefs.get_val(p_inp).unwrap();
                    match equiv.val {
                        Value::ConstUnknown => {
                            fixed.set(i, true).unwrap();
                            unknown.set(i, true).unwrap();
                        }
                        Value::Const(val) => {
                            fixed.set(i, true).unwrap();
                            inp_val.set(i, val).unwrap();
                        }
                        Value::Unknown => {
                            if equiv.change_visit == self.evaluator.change_visit_gen() {
                                fixed.set(i, true).unwrap();
                                unknown.set(i, true).unwrap();
                            }
                        }
                        Value::Dynam(val) => {
                            if equiv.change_visit == self.evaluator.change_visit_gen() {
                                fixed.set(i, true).unwrap();
                                inp_val.set(i, val).unwrap()
                            }
                        }
                    }
                }
                let lut_w = NonZeroUsize::new(original_lut.len()).unwrap();
                let mut lut = Awi::zero(lut_w);
                let mut reduced_lut = original_lut.clone();
                let mut lut_fixed = Awi::zero(lut_w);
                let mut lut_unknown = Awi::zero(lut_w);
                for (i, value) in original_lut.iter().enumerate() {
                    match value {
                        DynamicValue::ConstUnknown => {
                            lut_fixed.set(i, true).unwrap();
                            lut_unknown.set(i, true).unwrap();
                        }
                        DynamicValue::Const(b) => {
                            lut_fixed.set(i, true).unwrap();
                            lut.set(i, *b).unwrap()
                        }
                        DynamicValue::Dynam(p) => {
                            let equiv = self.backrefs.get_val(*p).unwrap();
                            match equiv.val {
                                Value::ConstUnknown => {
                                    lut_fixed.set(i, true).unwrap();
                                    lut_unknown.set(i, true).unwrap();
                                }
                                Value::Unknown => {
                                    lut_unknown.set(i, true).unwrap();
                                    if equiv.change_visit == self.evaluator.change_visit_gen() {
                                        lut_fixed.set(i, true).unwrap();
                                    }
                                }
                                Value::Const(b) => {
                                    lut_fixed.set(i, true).unwrap();
                                    lut.set(i, b).unwrap()
                                }
                                Value::Dynam(b) => {
                                    if equiv.change_visit == self.evaluator.change_visit_gen() {
                                        lut_fixed.set(i, true).unwrap();
                                        lut.set(i, b).unwrap()
                                    } else {
                                        lut_unknown.set(i, true).unwrap();
                                    }
                                }
                            }
                        }
                    }
                }
                // reduce the LUT based on fixed and known bits
                for i in (0..len).rev() {
                    if fixed.get(i).unwrap() && (!unknown.get(i).unwrap()) {
                        let bit = inp_val.get(i).unwrap();
                        LNode::reduce_lut(&mut lut, i, bit);
                        LNode::reduce_lut(&mut lut_fixed, i, bit);
                        LNode::reduce_lut(&mut lut_unknown, i, bit);
                        reduced_lut = LNode::reduce_dynamic_lut(&reduced_lut, i, bit).0;
                        // remove the input bits
                        len = len.checked_sub(1).unwrap();
                    }
                }
                if len == 0 {
                    // only one LUT bit left, no inputs
                    if lut_fixed.get(0).unwrap() {
                        if lut_unknown.get(0).unwrap() {
                            self.evaluator.insert(Eval::Change(Change {
                                depth,
                                p_equiv,
                                value: Value::Unknown,
                            }));
                            return vec![];
                        } else {
                            self.evaluator.insert(Eval::Change(Change {
                                depth,
                                p_equiv,
                                value: Value::Dynam(lut.get(0).unwrap()),
                            }));
                            return vec![];
                        }
                    } else {
                        let lut_bit = reduced_lut[0];
                        match lut_bit {
                            DynamicValue::ConstUnknown | DynamicValue::Const(_) => (),
                            DynamicValue::Dynam(p) => {
                                res.push(RequestLNode {
                                    depth: depth - 1,
                                    number_a: 0,
                                    p_back_lnode: p,
                                });
                                return res;
                            }
                        }
                    }
                }

                // if the LUT is all fixed and known ones or zeros, we can know that any unfixed
                // or unknown changes will be unable to affect the
                // output
                if lut_fixed.is_umax() && lut_unknown.is_zero() {
                    if lut.is_zero() {
                        self.evaluator.insert(Eval::Change(Change {
                            depth,
                            p_equiv,
                            value: Value::Dynam(false),
                        }));
                        return vec![];
                    } else if lut.is_umax() {
                        self.evaluator.insert(Eval::Change(Change {
                            depth,
                            p_equiv,
                            value: Value::Dynam(true),
                        }));
                        return vec![];
                    }
                }

                if fixed.is_umax() && lut_fixed.is_umax() {
                    // if we did not evaluate to a value earlier, then we know our value is unknown
                    self.evaluator.insert(Eval::Change(Change {
                        depth,
                        p_equiv,
                        value: Value::Unknown,
                    }));
                    return vec![];
                }

                for i in (0..inp.len()).rev() {
                    if (!fixed.get(i).unwrap()) || unknown.get(i).unwrap() {
                        res.push(RequestLNode {
                            depth: depth - 1,
                            number_a: 0,
                            p_back_lnode: inp[i],
                        });
                    }
                }
                // make sure we only request the LUT bits we need
                for lut_bit in reduced_lut {
                    if let DynamicValue::Dynam(p) = lut_bit {
                        // TODO make the priority make the index bits always requested fully first
                        res.push(RequestLNode {
                            depth: depth - 1,
                            number_a: 0,
                            p_back_lnode: p,
                        });
                    }
                }
            }
        }
        res
    }

    /// If the returned vector is empty, evaluation was successful, otherwise
    /// what is needed for evaluation is returned
    pub fn try_eval_tnode(&mut self, p_tnode: PTNode, depth: i64) -> Option<RequestTNode> {
        // read current inputs
        let tnode = self.tnodes.get(p_tnode).unwrap();
        let p_equiv = self.backrefs.get_val(tnode.p_self).unwrap().p_self_equiv;
        let p_driver = tnode.p_driver;
        let equiv = self.backrefs.get_val(p_driver).unwrap();
        if equiv.val.is_const() || (equiv.change_visit == self.evaluator.change_visit_gen()) {
            self.evaluator.insert(Eval::Change(Change {
                depth,
                p_equiv,
                value: equiv.val,
            }));
            None
        } else {
            Some(RequestTNode {
                depth: depth - 1,
                number_a: 0,
                p_back_tnode: p_driver,
            })
        }
    }

    pub fn change_value(&mut self, p_back: PBack, value: Value) -> Result<(), Error> {
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() && (equiv.val != value) {
                return Err(Error::OtherStr(
                    "tried to change a constant (probably, `retro_const_` was used followed by a \
                     contradicting `retro_*`",
                ))
            }
            // switch to change phase if not already
            if self.evaluator.phase != EvalPhase::Change {
                self.evaluator.phase = EvalPhase::Change;
                self.evaluator.next_change_visit_gen();
            }
            equiv.val = value;
            equiv.change_visit = self.evaluator.change_visit_gen();
            Ok(())
        } else {
            Err(Error::InvalidPtr)
        }
    }

    pub fn calculate_value_with_lower_capability(
        epoch_shared: &EpochShared,
        p_back: PBack,
    ) -> Result<Value, Error> {
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some(equiv) = ensemble.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() {
                return Ok(equiv.val)
            }
            // switch to request phase if not already
            if ensemble.evaluator.phase != EvalPhase::Request {
                ensemble.evaluator.phase = EvalPhase::Request;
                ensemble.evaluator.next_request_visit_gen();
            }
            let visit = ensemble.evaluator.request_visit_gen();
            if equiv.request_visit != visit {
                equiv.request_visit = visit;
                ensemble
                    .evaluator
                    .insert(Eval::Investigate0(0, equiv.p_self_equiv));
                drop(lock);
                Ensemble::handle_requests_with_lower_capability(epoch_shared)?;
            } else {
                drop(lock);
            }
            Ok(epoch_shared
                .epoch_data
                .borrow()
                .ensemble
                .backrefs
                .get_val(p_back)
                .unwrap()
                .val)
        } else {
            Err(Error::InvalidPtr)
        }
    }

    pub fn calculate_value(&mut self, p_back: PBack) -> Result<Value, Error> {
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() {
                return Ok(equiv.val)
            }
            // switch to request phase if not already
            if self.evaluator.phase != EvalPhase::Request {
                self.evaluator.phase = EvalPhase::Request;
                self.evaluator.next_request_visit_gen();
            }
            let visit = self.evaluator.request_visit_gen();
            if equiv.request_visit != visit {
                equiv.request_visit = visit;
                self.evaluator
                    .insert(Eval::Investigate0(0, equiv.p_self_equiv));
                self.handle_requests()?;
            }
            Ok(self.backrefs.get_val(p_back).unwrap().val)
        } else {
            Err(Error::InvalidPtr)
        }
    }

    pub(crate) fn handle_requests_with_lower_capability(
        epoch_shared: &EpochShared,
    ) -> Result<(), Error> {
        // TODO currently, the only way of avoiding N^2 worst case scenarios where
        // different change cascades lead to large groups of nodes being evaluated
        // repeatedly, is to use the front strategy. Only a powers of two reduction tree
        // hierarchy system could fix this it appears, which will require a lot more
        // code.

        loop {
            // empty `states_to_lower`
            loop {
                let mut lock = epoch_shared.epoch_data.borrow_mut();
                if let Some(p_state) = lock.ensemble.stator.states_to_lower.pop() {
                    if let Some(state) = lock.ensemble.stator.states.get(p_state) {
                        // first check that it has not already been lowered
                        if !state.lowered_to_lnodes {
                            drop(lock);
                            Ensemble::dfs_lower(epoch_shared, p_state)?;
                            let mut lock = epoch_shared.epoch_data.borrow_mut();
                            // reinvestigate
                            let len = lock.ensemble.stator.states[p_state].p_self_bits.len();
                            for i in 0..len {
                                let p_bit = lock.ensemble.stator.states[p_state].p_self_bits[i];
                                if let Some(p_bit) = p_bit {
                                    lock.ensemble.evaluator.insert(Eval::Investigate0(0, p_bit));
                                }
                            }
                            drop(lock);
                        }
                    }
                } else {
                    break
                }
            }
            // break if both are empty
            let mut lock = epoch_shared.epoch_data.borrow_mut();
            if lock.ensemble.evaluator.evaluations.is_empty()
                && lock.ensemble.stator.states_to_lower.is_empty()
            {
                break
            }
            // evaluate
            if let Some(p_eval) = lock.ensemble.evaluator.evaluations.min() {
                lock.ensemble.evaluate(p_eval);
            }
            drop(lock);
        }
        Ok(())
    }

    pub(crate) fn handle_requests(&mut self) -> Result<(), Error> {
        while let Some(p_eval) = self.evaluator.evaluations.min() {
            self.evaluate(p_eval);
        }
        Ok(())
    }

    fn evaluate(&mut self, p_eval: PEval) {
        let evaluation = self.evaluator.evaluations.remove(p_eval).unwrap().0;
        match evaluation {
            Eval::Investigate0(depth, p_equiv) => self.eval_investigate0(p_equiv, depth),
            Eval::ChangeLNode(p_lnode) => {
                // the initial investigate handles all input requests
                // TODO get priorities right
                let _ = self.try_eval_lnode(p_lnode, 0);
            }
            Eval::ChangeTNode(p_tnode) => {
                let _ = self.try_eval_tnode(p_tnode, 0);
            }
            Eval::Change(change) => {
                let equiv = self.backrefs.get_val_mut(change.p_equiv).unwrap();
                equiv.change_visit = self.evaluator.change_visit_gen();
                // Handles a rare case where the evaluator decides to change to a const, and
                // something later tries to set it to an unknown. TODO not sure if this is a bug
                // that should be resolved some other way, the relevant part is where `Change`s
                // are pushed in `eval_state`.
                if !equiv.val.is_const() {
                    equiv.val = change.value;
                }
                let mut adv = self.backrefs.advancer_surject(change.p_equiv);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    let referent = *self.backrefs.get_key(p_back).unwrap();
                    match referent {
                        Referent::ThisEquiv
                        | Referent::ThisLNode(_)
                        | Referent::ThisTNode(_)
                        | Referent::ThisStateBit(..) => (),
                        Referent::Input(p_lnode) => {
                            let lnode = self.lnodes.get(p_lnode).unwrap();
                            let p_self = lnode.p_self;
                            let equiv = self.backrefs.get_val(p_self).unwrap();
                            if (equiv.request_visit == self.evaluator.request_visit_gen())
                                && (equiv.change_visit != self.evaluator.change_visit_gen())
                            {
                                // only go leafward to the given input if it was in the request
                                // front and it hasn't been updated by some other route
                                self.evaluator.insert(Eval::ChangeLNode(p_lnode));
                            }
                        }
                        Referent::Driver(p_tnode) => {
                            let tnode = self.tnodes.get(p_tnode).unwrap();
                            let p_self = tnode.p_self;
                            let equiv = self.backrefs.get_val(p_self).unwrap();
                            if (equiv.request_visit == self.evaluator.request_visit_gen())
                                && (equiv.change_visit != self.evaluator.change_visit_gen())
                            {
                                // only go leafward to the given input if it was in the request
                                // front and it hasn't been updated by some other route
                                self.evaluator.insert(Eval::ChangeTNode(p_tnode));
                            }
                        }
                        Referent::ThisRNode(_) => (),
                    }
                }
            }
            Eval::RequestLNode(request) => {
                if let Referent::Input(_) = self.backrefs.get_key(request.p_back_lnode).unwrap() {
                    let equiv = self.backrefs.get_val(request.p_back_lnode).unwrap();
                    if equiv.request_visit != self.evaluator.request_visit_gen() {
                        self.evaluator
                            .insert(Eval::Investigate0(request.depth, equiv.p_self_equiv));
                    }
                } else {
                    unreachable!()
                }
            }
            Eval::RequestTNode(request) => {
                if let Referent::Driver(_) = self.backrefs.get_key(request.p_back_tnode).unwrap() {
                    let equiv = self.backrefs.get_val(request.p_back_tnode).unwrap();
                    if equiv.request_visit != self.evaluator.request_visit_gen() {
                        self.evaluator
                            .insert(Eval::Investigate0(request.depth, equiv.p_self_equiv));
                    }
                } else {
                    unreachable!()
                }
            }
            Eval::Investigate1(_) => todo!(),
        }
    }

    fn eval_investigate0(&mut self, p_equiv: PBack, depth: i64) {
        let equiv = self.backrefs.get_val_mut(p_equiv).unwrap();
        equiv.request_visit = self.evaluator.request_visit_gen();
        if equiv.val.is_const() || (equiv.change_visit == self.evaluator.change_visit_gen()) {
            // no need to do anything
            return
        }
        // eval but is only inserted if nothing like the LNode evaluation is able to
        // prove early value setting
        let mut insert_if_no_early_exit = vec![];
        let mut saw_node = false;
        let mut saw_state = None;
        let mut adv = self.backrefs.advancer_surject(p_equiv);
        while let Some(p_back) = adv.advance(&self.backrefs) {
            let referent = *self.backrefs.get_key(p_back).unwrap();
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisLNode(p_lnode) => {
                    let v = self.try_eval_lnode(p_lnode, depth);
                    if v.is_empty() {
                        // early exit because evaluation was successful
                        return
                    }
                    for request in v {
                        insert_if_no_early_exit.push(Eval::RequestLNode(request));
                    }
                    saw_node = true;
                }
                Referent::ThisTNode(p_tnode) => {
                    if let Some(request) = self.try_eval_tnode(p_tnode, depth) {
                        insert_if_no_early_exit.push(Eval::RequestTNode(request));
                    } else {
                        // early exit because evaluation was successful
                        return
                    }
                    saw_node = true;
                }
                Referent::ThisStateBit(p_state, _) => {
                    saw_state = Some(p_state);
                }
                Referent::Input(_) => (),
                Referent::Driver(_) => (),
                Referent::ThisRNode(_) => (),
            }
        }
        if !saw_node {
            let mut will_lower = false;
            if let Some(p_state) = saw_state {
                if !self.stator.states[p_state].lowered_to_lnodes {
                    will_lower = true;
                    self.stator.states_to_lower.push(p_state);
                }
            }
            if !will_lower {
                // must be a root
                let equiv = self.backrefs.get_val_mut(p_equiv).unwrap();
                let value = equiv.val;
                equiv.change_visit = self.evaluator.change_visit_gen();
                self.evaluator.insert(Eval::Change(Change {
                    depth,
                    p_equiv,
                    value,
                }));
            }
        }
        for eval in insert_if_no_early_exit {
            self.evaluator.insert(eval);
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
