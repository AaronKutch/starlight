use std::{collections::BinaryHeap, num::NonZeroU64};

use awint::awint_dag::{
    triple_arena::{ptr_struct, Arena, SurjectArena},
    EvalError,
};

use super::PTNode;
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

ptr_struct!(PValueChange; PValueRequest; PChangeFront; PRequestFront);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValueChange {
    pub p_back: PBack,
    pub new_value: Value,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValueRequest {
    pub p_back: PBack,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvalPhase {
    Change,
    Request,
}

/*#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct EvalFront {
    front_id: NonZeroU64,
}

impl EvalFront {
    /// `front_id` needs to be unique per front
    pub fn new(front_id: NonZeroU64) -> Self {
        Self { front_id }
    }

    pub fn front_id(&self) -> NonZeroU64 {
        self.front_id
    }
}*/

/// In order from highest to lowest priority
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvalAction {
    FindRoots(PBack),
    EvalChange(PTNode),
}

#[derive(Debug, Clone)]
pub struct Evaluator {
    changes: Arena<PValueChange, ValueChange>,
    // the lists are used to avoid the O(N) penalty of advancing through an arena
    change_list: Vec<PValueChange>,
    requests: Arena<PValueRequest, ValueRequest>,
    request_list: Vec<PValueRequest>,
    phase: EvalPhase,
    visit_gen: NonZeroU64,
    change_front: SurjectArena<PChangeFront, PBack, ()>,
    request_front: SurjectArena<PRequestFront, PBack, ()>,
    eval_priority: BinaryHeap<EvalAction>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            changes: Arena::new(),
            change_list: vec![],
            requests: Arena::new(),
            request_list: vec![],
            phase: EvalPhase::Change,
            visit_gen: NonZeroU64::new(2).unwrap(),
            change_front: SurjectArena::new(),
            request_front: SurjectArena::new(),
            eval_priority: BinaryHeap::new(),
        }
    }

    pub fn visit_gen(&self) -> NonZeroU64 {
        self.visit_gen
    }

    pub fn next_visit_gen(&mut self) -> NonZeroU64 {
        self.visit_gen = NonZeroU64::new(self.visit_gen.get().checked_add(1).unwrap()).unwrap();
        self.visit_gen
    }
}

impl Ensemble {
    /// Does nothing besides check for containment if the value does not
    /// actually change, or if the value was constant
    pub fn change_value(&mut self, p_back: PBack, value: Value) -> Option<()> {
        if self.evaluator.phase != EvalPhase::Change {
            self.evaluator.phase = EvalPhase::Change;
        }
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() {
                // not allowed
                panic!();
            }
            if equiv.val == value {
                if let Some(prev_val_change) = equiv.val_change {
                    // this needs to be kept because of the list, this prevents the list from being
                    // able to grow indefinitely with duplicates
                    self.evaluator
                        .changes
                        .get_mut(prev_val_change)
                        .unwrap()
                        .new_value = value;
                }
                return Some(())
            }
            if let Some(prev_val_change) = equiv.val_change {
                // there was another change to this bit in this evaluation phase we need to
                // overwrite so we don't have bugs where the previous runs later
                self.evaluator
                    .changes
                    .get_mut(prev_val_change)
                    .unwrap()
                    .new_value = value;
            } else {
                let p_val_change = self.evaluator.changes.insert(ValueChange {
                    p_back: equiv.p_self_equiv,
                    new_value: value,
                });
                equiv.val_change = Some(p_val_change);
                self.evaluator.change_list.push(p_val_change);
            }
            Some(())
        } else {
            None
        }
    }
}

impl Ensemble {
    // stepping loops should request their drivers, evaluating everything requests
    // everything
    pub fn request_value(&mut self, p_back: PBack) -> Result<Value, EvalError> {
        if !self.backrefs.contains(p_back) {
            return Err(EvalError::InvalidPtr)
        }
        // switch to request phase
        if self.evaluator.phase != EvalPhase::Request {
            self.evaluator.phase = EvalPhase::Request;
            self.evaluator.change_front.clear();
            self.evaluator.request_front.clear();
            self.evaluator.eval_priority.clear();
            self.evaluator.next_visit_gen();
        }
        let p_val_request = self.evaluator.requests.insert(ValueRequest { p_back });
        self.handle_requests();
        self.evaluator.request_list.push(p_val_request);
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

        let visit = self.evaluator.visit_gen();
        while let Some(p_val_change) = self.evaluator.change_list.pop() {
            if let Some(change) = self.evaluator.changes.remove(p_val_change) {
                let equiv = self.backrefs.get_val_mut(change.p_back).unwrap();
                if equiv.eval_visit == visit {
                    // indicates that some kind of exploring didn't handle the change
                    unreachable!();
                }
                equiv.eval_visit = visit;
                self.evaluator.change_front.insert(equiv.p_self_equiv, ());
            }
            // else a backtrack to root probably already handled the change
        }
    }
}
