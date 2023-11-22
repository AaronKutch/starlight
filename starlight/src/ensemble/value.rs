use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{
        triple_arena::{ptr_struct, Advancer, OrdArena},
        EvalError, PState,
    },
    Awi,
};

use super::{PTNode, Referent, TNode};
use crate::{
    ensemble::{Ensemble, PBack},
    epoch::{get_current_epoch, EpochShared},
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
            // TODO how to handle `Opaque`s?
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
    depth: i64,
    number_a: u8,
    p_back_tnode: PBack,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct Change {
    depth: i64,
    p_equiv: PBack,
    value: Value,
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
    // the lists are used to avoid the O(N) penalty of advancing through an arena
    change_list: Vec<PBack>,
    phase: EvalPhase,
    change_visit_gen: NonZeroU64,
    request_visit_gen: NonZeroU64,
    evaluations: OrdArena<PEval, Eval, ()>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            change_list: vec![],
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

    // stepping loops should request their drivers, evaluating everything requests
    // everything
    pub fn thread_local_state_value(p_state: PState, bit_i: usize) -> Result<Value, EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.lock().unwrap();
        let ensemble = &mut lock.ensemble;
        ensemble.initialize_state_bits_if_needed(p_state).unwrap();
        let state = ensemble.states.get(p_state).unwrap();
        let p_back = *state.p_self_bits.get(bit_i).unwrap();
        if let Some(equiv) = ensemble.backrefs.get_val_mut(p_back) {
            // switch to request phase
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
                Ensemble::handle_requests(&epoch_shared);
            } else {
                drop(lock);
            }
            Ok(epoch_shared
                .epoch_data
                .lock()
                .unwrap()
                .ensemble
                .backrefs
                .get_val(p_back)
                .unwrap()
                .val)
        } else {
            Err(EvalError::InvalidPtr)
        }
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
                if fixed.get(i).unwrap() && unknown.get(i).unwrap() {
                    if TNode::reduce_independent_lut(&lut, i).is_none() {
                        self.evaluator.insert(Eval::Change(Change {
                            depth,
                            p_equiv,
                            value: Value::Unknown,
                        }));
                        return vec![];
                    }
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
        }
        res
    }

    pub fn change_value(&mut self, p_back: PBack, value: Value) -> Option<()> {
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if self.evaluator.phase != EvalPhase::Change {
                self.evaluator.phase = EvalPhase::Change;
                self.evaluator.next_change_visit_gen();
            }
            if equiv.val.is_const() {
                // not allowed
                panic!();
            }
            if let Some(ref mut prev_val_change) = equiv.val_change {
                // there was another change to this bit in this evaluation phase we need to
                // overwrite so we don't have bugs where the previous runs later
                *prev_val_change = value;
            }
            if equiv.val == value {
                // this needs to be kept because of the list, this prevents the list from being
                // able to grow indefinitely with duplicates
                return Some(())
            }
            if equiv.val_change.is_none() {
                equiv.val_change = Some(value);
                self.evaluator.change_list.push(equiv.p_self_equiv);
            }
            Some(())
        } else {
            None
        }
    }

    fn handle_requests(epoch_shared: &EpochShared) {
        // TODO currently, the only way of avoiding N^2 worst case scenarios where
        // different change cascades lead to large groups of nodes being evaluated
        // repeatedly, is to use the front strategy. Only a powers of two reduction tree
        // hierarchy system could fix this it appears, which will require a lot more
        // code.

        loop {
            while let Some(p_state) = epoch_shared
                .epoch_data
                .lock()
                .unwrap()
                .ensemble
                .states_to_lower
                .pop()
            {
                Ensemble::dfs_lower(&epoch_shared, p_state).unwrap();
            }
            let mut lock = epoch_shared.epoch_data.lock().unwrap();
            if lock.ensemble.evaluator.evaluations.is_empty()
                && lock.ensemble.states_to_lower.is_empty()
            {
                break
            }
            if let Some(p_eval) = lock.ensemble.evaluator.evaluations.min() {
                lock.ensemble.evaluate(p_eval);
            }
            drop(lock);
        }
    }

    fn evaluate(&mut self, p_eval: PEval) {
        let evaluation = self.evaluator.evaluations.remove(p_eval).unwrap().0;
        dbg!(evaluation);
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
                                && (equiv.change_visit == self.evaluator.change_visit_gen())
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
        let equiv = self.backrefs.get_val(p_equiv).unwrap();
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
            if let Some(p_state) = saw_state {
                self.states_to_lower.push(p_state);
            }
        }
        for eval in insert_if_no_early_exit {
            self.evaluator.insert(eval);
        }
    }
}
