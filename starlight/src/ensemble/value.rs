use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{
        triple_arena::{ptr_struct, Advancer, OrdArena},
        EvalError,
    },
    Awi,
};

use crate::{
    ensemble::{Ensemble, PBack, PTNode, Referent, TNode},
    epoch::EpochShared,
};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Unknown,
    Const(bool),
    Dynam(bool),
}

impl Value {
    pub fn from_dag_lit(lit: Option<bool>) -> Self {
        if let Some(lit) = lit {
            Value::Const(lit)
        } else {
            Value::Unknown
        }
    }

    pub fn known_value(self) -> Option<bool> {
        match self {
            Value::Unknown => None,
            Value::Const(b) => Some(b),
            Value::Dynam(b) => Some(b),
        }
    }

    pub fn is_const(self) -> bool {
        matches!(self, Value::Const(_))
    }

    pub fn is_known(self) -> bool {
        match self {
            Value::Unknown => false,
            Value::Const(_) | Value::Dynam(_) => true,
        }
    }

    pub fn is_unknown(self) -> bool {
        !self.is_known()
    }
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
    ChangeTNode(PTNode),
    Change(Change),
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
    pub fn try_eval_tnode(&mut self, p_tnode: PTNode, depth: i64) -> Vec<RequestTNode> {
        let mut res = vec![];
        // read current inputs
        let tnode = self.tnodes.get(p_tnode).unwrap();
        let p_equiv = self.backrefs.get_val(tnode.p_self).unwrap().p_self_equiv;
        if let Some(original_lut) = &tnode.lut {
            let len = u8::try_from(tnode.inp.len()).unwrap();
            let len = usize::from(len);
            // the nominal value of the inputs
            let mut inp = Awi::zero(NonZeroUsize::new(len).unwrap());
            // corresponding bits are set if the input is either a const value or is
            // already evaluated
            let mut fixed = inp.clone();
            // corresponding bits ar set if the input is `Value::Unknown`
            let mut unknown = inp.clone();
            for i in 0..len {
                let p_inp = tnode.inp[i];
                let equiv = self.backrefs.get_val(p_inp).unwrap();
                if let Value::Const(val) = equiv.val {
                    fixed.set(i, true).unwrap();
                    inp.set(i, val).unwrap();
                } else if equiv.change_visit == self.evaluator.change_visit_gen() {
                    fixed.set(i, true).unwrap();
                    if let Some(val) = equiv.val.known_value() {
                        inp.set(i, val).unwrap()
                    } else {
                        unknown.set(i, true).unwrap();
                    }
                }
            }
            let mut lut = original_lut.clone();
            // if fixed and unknown bits can influence the value,
            // then the value of this equivalence can also be fixed
            // to unknown
            for i in 0..len {
                if fixed.get(i).unwrap()
                    && unknown.get(i).unwrap()
                    && TNode::reduce_independent_lut(&lut, i).is_none()
                {
                    self.evaluator.insert(Eval::Change(Change {
                        depth,
                        p_equiv,
                        value: Value::Unknown,
                    }));
                    return vec![];
                }
            }
            // reduce the LUT based on fixed and known bits
            for i in (0..len).rev() {
                if fixed.get(i).unwrap() && (!unknown.get(i).unwrap()) {
                    lut = TNode::reduce_lut(&lut, i, inp.get(i).unwrap());
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
            // TODO prioritize bits that could lead to number_a optimization
            /*let mut skip = 0;
            for i in 0..len {
                if fixed.get(i).unwrap() && !unknown.get(i).unwrap() {
                    skip += 1;
                } else if unknown.get(i).unwrap() {
                    // assume unchanging
                    lut = TNode::reduce_lut(&lut, i, inp.get(i).unwrap());
                    //
                } else {}
            }*/
            for i in (0..len).rev() {
                if (!fixed.get(i).unwrap()) || unknown.get(i).unwrap() {
                    res.push(RequestTNode {
                        depth: depth - 1,
                        number_a: 0,
                        p_back_tnode: tnode.inp[i],
                    });
                }
            }
        } else {
            // TNode without LUT
            let p_inp = tnode.inp[0];
            let equiv = self.backrefs.get_val(p_inp).unwrap();
            if let Value::Const(val) = equiv.val {
                self.evaluator.insert(Eval::Change(Change {
                    depth,
                    p_equiv,
                    value: Value::Const(val),
                }));
            } else if equiv.change_visit == self.evaluator.change_visit_gen() {
                // fixed
                self.evaluator.insert(Eval::Change(Change {
                    depth,
                    p_equiv,
                    value: equiv.val,
                }));
            } else {
                res.push(RequestTNode {
                    depth: depth - 1,
                    number_a: 0,
                    p_back_tnode: tnode.inp[0],
                });
            }
        }
        res
    }

    pub fn change_value(&mut self, p_back: PBack, value: Value) -> Option<()> {
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() {
                // not allowed
                panic!();
            }
            // switch to change phase if not already
            if self.evaluator.phase != EvalPhase::Change {
                self.evaluator.phase = EvalPhase::Change;
                self.evaluator.next_change_visit_gen();
            }
            equiv.val = value;
            Some(())
        } else {
            None
        }
    }

    pub fn calculate_value(epoch_shared: &EpochShared, p_back: PBack) -> Result<Value, EvalError> {
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
                Ensemble::handle_requests(epoch_shared)?;
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
            Err(EvalError::InvalidPtr)
        }
    }

    fn handle_requests(epoch_shared: &EpochShared) -> Result<(), EvalError> {
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
                    let state = &lock.ensemble.stator.states[p_state];
                    // first check that it has not already been lowered
                    if !state.lowered_to_tnodes {
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

    fn evaluate(&mut self, p_eval: PEval) {
        let evaluation = self.evaluator.evaluations.remove(p_eval).unwrap().0;
        match evaluation {
            Eval::Investigate0(depth, p_equiv) => self.eval_investigate0(p_equiv, depth),
            Eval::ChangeTNode(p_tnode) => {
                // the initial investigate handles all input requests
                // TODO get priorities right
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
                        Referent::ThisEquiv => (),
                        Referent::ThisTNode(_) => (),
                        Referent::ThisStateBit(..) => (),
                        Referent::Input(p_tnode) => {
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
                        Referent::LoopDriver(_) => (),
                        Referent::Note(_) => (),
                    }
                }
            }
            Eval::RequestTNode(request) => {
                if let Referent::Input(_) = self.backrefs.get_key(request.p_back_tnode).unwrap() {
                    let equiv = self.backrefs.get_val(request.p_back_tnode).unwrap();
                    if (equiv.change_visit != self.evaluator.change_visit_gen())
                        || (equiv.request_visit != self.evaluator.request_visit_gen())
                    {
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
        if matches!(equiv.val, Value::Const(_))
            || (equiv.change_visit == self.evaluator.change_visit_gen())
        {
            // no need to do anything
            return
        }
        // eval but is only inserted if nothing like the TNode evaluation is able to
        // prove early value setting
        let mut insert_if_no_early_exit = vec![];
        let mut saw_tnode = false;
        let mut saw_state = None;
        let mut adv = self.backrefs.advancer_surject(p_equiv);
        while let Some(p_back) = adv.advance(&self.backrefs) {
            let referent = *self.backrefs.get_key(p_back).unwrap();
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisTNode(p_tnode) => {
                    let v = self.try_eval_tnode(p_tnode, depth);
                    if v.is_empty() {
                        // early exit because evaluation was successful
                        return
                    }
                    for eval in v {
                        insert_if_no_early_exit.push(Eval::RequestTNode(eval));
                    }
                    saw_tnode = true;
                }
                Referent::ThisStateBit(p_state, _) => {
                    saw_state = Some(p_state);
                }
                Referent::Input(_) => (),
                Referent::LoopDriver(_) => {}
                Referent::Note(_) => (),
            }
        }
        if !saw_tnode {
            let mut will_lower = false;
            if let Some(p_state) = saw_state {
                if !self.stator.states[p_state].lowered_to_tnodes {
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
