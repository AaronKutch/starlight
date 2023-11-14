use std::num::NonZeroU64;

use awint::awint_dag::{
    triple_arena::{ptr_struct, Arena, Ptr},
    EvalError, PState,
};

use crate::ensemble::{Ensemble, PBack, Referent, Value};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    Unknown,
    Const(bool),
    Dynam(bool, NonZeroU64),
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
            Value::Dynam(b, _) => Some(b),
        }
    }

    pub fn is_const(self) -> bool {
        matches!(self, Value::Const(_))
    }

    pub fn is_known_with_visit_ge(self, visit: NonZeroU64) -> bool {
        match self {
            Value::Unknown => false,
            Value::Const(_) => true,
            Value::Dynam(_, this_visit) => this_visit >= visit,
        }
    }

    /// Converts constants to dynamics, and sets any generations to `visit_gen`
    pub fn const_to_dynam(self, visit_gen: NonZeroU64) -> Self {
        match self {
            Value::Unknown => Value::Unknown,
            Value::Const(b) => Value::Dynam(b, visit_gen),
            Value::Dynam(b, _) => Value::Dynam(b, visit_gen),
        }
    }
}

ptr_struct!(PValueChange);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct ValueChange {
    pub p_back: PBack,
    pub new_value: Value,
}

#[derive(Debug, Default, Clone)]
pub struct Evaluator {
    pub changes: Arena<PValueChange, ValueChange>,
    pub list: Vec<PValueChange>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Ensemble {
    /// Does nothing besides check for containment if the value does not
    /// actually change, or if the value was constant
    pub fn update_value(&mut self, p_back: PBack, value: Value) -> Option<()> {
        if let Some(equiv) = tdag.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() {
                // not sure about my semantics
                todo!();
            }
            if equiv.val == value {
                if let Some(prev_val_change) = equiv.val_change {
                    // this needs to be kept because of the list
                    self.changes.get_mut(prev_val_change).unwrap().new_value = value;
                }
                return Some(())
            }
            if let Some(prev_val_change) = equiv.val_change {
                // there was another change to this bit in this evaluation phase we need to
                // overwrite so we don't have bugs where the previous runs later
                self.changes.get_mut(prev_val_change).unwrap().new_value = value;
            } else {
                let p_val_change = self.changes.insert(ValueChange {
                    p_back: equiv.p_self_equiv,
                    new_value: value,
                });
                equiv.val_change = Some(p_val_change);
                self.list.push(p_val_change);
            }
            Some(())
        } else {
            None
        }
    }
}

impl Ensemble {
    pub fn internal_eval_bit(&mut self, p_back: PBack) -> Result<Value, EvalError> {
        if !self.backrefs.contains(p_back) {
            return Err(EvalError::InvalidPtr)
        }
        Ok(self.backrefs.get_val(p_back).unwrap().val)
    }
}
