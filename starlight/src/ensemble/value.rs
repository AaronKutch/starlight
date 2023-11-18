use std::num::NonZeroU64;

use awint::awint_dag::{
    triple_arena::{ptr_struct, SurjectArena},
    EvalError,
};

use crate::ensemble::{Ensemble, PBack};

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

*/

ptr_struct!(PChangeFront; PRequestFront);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvalPhase {
    Change,
    Request,
}

#[derive(Debug, Clone)]
pub struct Evaluator {
    // the lists are used to avoid the O(N) penalty of advancing through an arena
    change_list: Vec<PBack>,
    request_list: Vec<PBack>,
    phase: EvalPhase,
    change_visit_gen: NonZeroU64,
    request_visit_gen: NonZeroU64,
    change_front: SurjectArena<PChangeFront, PBack, ()>,
    request_front: SurjectArena<PRequestFront, PBack, ()>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            change_list: vec![],
            request_list: vec![],
            phase: EvalPhase::Change,
            change_visit_gen: NonZeroU64::new(2).unwrap(),
            request_visit_gen: NonZeroU64::new(2).unwrap(),
            change_front: SurjectArena::new(),
            request_front: SurjectArena::new(),
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
}

impl Ensemble {
    pub fn change_value(&mut self, p_back: PBack, value: Value) -> Option<()> {
        if self.evaluator.phase != EvalPhase::Change {
            self.evaluator.phase = EvalPhase::Change;
            self.evaluator.next_change_visit_gen();
        }
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
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

    // stepping loops should request their drivers, evaluating everything requests
    // everything
    pub fn request_value(&mut self, p_back: PBack) -> Result<Value, EvalError> {
        if !self.backrefs.contains(p_back) {
            return Err(EvalError::InvalidPtr)
        }
        // switch to request phase
        if self.evaluator.phase != EvalPhase::Request {
            self.evaluator.phase = EvalPhase::Request;
            self.evaluator.request_front.clear();
            self.evaluator.next_request_visit_gen();
        }
        self.evaluator.request_list.push(p_back);
        self.handle_requests();
        Ok(self.backrefs.get_val(p_back).unwrap().val)
    }

    // TODO have a harder request that initiates optimizations if the fronts run out

    fn handle_requests(&mut self) {
        // TODO currently, the only way of avoiding N^2 worst case scenarios where
        // different change cascades lead to large groups of nodes being evaluated
        // repeatedly, is to use the front strategy. Only a powers of two reduction tree
        // hierarchy system could fix this it appears, which will require a lot more
        // code.

        // The current system improves on previous impls creating a front on all nodes,
        // by having tracking changes. Independent fronts expand out from root changes,
        // merging cyclic chains together when they contact, and only growing if there
        // are nodes with changes. If part wany through, the set of changes becomes
        // empty, the entire evaluation can stop early.

        // TODO in an intermediate step we could identify choke points and step the
        // changes to them to identify early if a cascade stops

        let request_visit = self.evaluator.request_visit_gen();
        while let Some(p_back) = self.evaluator.request_list.pop() {
            let equiv = self.backrefs.get_val_mut(p_back).unwrap();
            if equiv.request_visit != request_visit {
                equiv.request_visit = request_visit;
            }
            self.evaluator.request_front.insert(equiv.p_self_equiv, ());
        }
    }
}
