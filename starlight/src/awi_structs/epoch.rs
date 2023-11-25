#![allow(clippy::new_without_default)]

use std::{cell::RefCell, mem, num::NonZeroUsize, rc::Rc, thread::panicking};

use awint::{
    awint_dag::{
        epoch::{EpochCallback, EpochKey},
        triple_arena::{ptr_struct, Arena},
        EvalError, Lineage, Location, Op, PState,
    },
    bw, dag,
};

use crate::ensemble::Ensemble;

#[derive(Debug, Clone)]
pub struct Assertions {
    pub bits: Vec<PState>,
}

impl Assertions {
    pub fn new() -> Self {
        Self { bits: vec![] }
    }
}

impl Default for Assertions {
    fn default() -> Self {
        Self::new()
    }
}

ptr_struct!(PEpochShared);

#[derive(Debug)]
pub struct PerEpochShared {
    pub states_inserted: Vec<PState>,
    pub assertions: Assertions,
}

impl PerEpochShared {
    pub fn new() -> Self {
        Self {
            states_inserted: vec![],
            assertions: Assertions::new(),
        }
    }
}

pub struct EpochData {
    pub epoch_key: EpochKey,
    pub ensemble: Ensemble,
    pub responsible_for: Arena<PEpochShared, PerEpochShared>,
    pub keep_flag: bool,
}

#[derive(Clone)]
pub struct EpochShared {
    pub epoch_data: Rc<RefCell<EpochData>>,
    pub p_self: PEpochShared,
}

impl EpochShared {
    /// Creates a new `Ensemble` and registers a new `EpochCallback`.
    pub fn new() -> Self {
        let mut epoch_data = EpochData {
            epoch_key: _callback().push_on_epoch_stack(),
            ensemble: Ensemble::new(),
            responsible_for: Arena::new(),
            keep_flag: true,
        };
        let p_self = epoch_data.responsible_for.insert(PerEpochShared::new());
        Self {
            epoch_data: Rc::new(RefCell::new(epoch_data)),
            p_self,
        }
    }

    /// Does _not_ register a new `EpochCallback`, instead
    pub fn shared_with(other: &Self) -> Self {
        let p_self = other
            .epoch_data
            .borrow_mut()
            .responsible_for
            .insert(PerEpochShared::new());
        Self {
            epoch_data: Rc::clone(&other.epoch_data),
            p_self,
        }
    }

    /// Returns a clone of the assertions currently associated with `self`
    pub fn assertions(&self) -> Assertions {
        let p_self = self.p_self;
        self.epoch_data
            .borrow()
            .responsible_for
            .get(p_self)
            .unwrap()
            .assertions
            .clone()
    }

    /// Returns a clone of the ensemble
    pub fn ensemble(&self) -> Ensemble {
        self.epoch_data.borrow().ensemble.clone()
    }

    pub fn assertions_empty(&self) -> bool {
        let epoch_data = self.epoch_data.borrow();
        let ours = epoch_data.responsible_for.get(self.p_self).unwrap();
        ours.assertions.bits.is_empty()
    }

    pub fn take_states_added(&mut self) -> Vec<PState> {
        let mut epoch_data = self.epoch_data.borrow_mut();
        let ours = epoch_data.responsible_for.get_mut(self.p_self).unwrap();
        mem::take(&mut ours.states_inserted)
    }

    /// Removes associated states and assertions
    pub fn remove_associated(&self) {
        let mut epoch_data = self.epoch_data.borrow_mut();
        let ours = epoch_data.responsible_for.remove(self.p_self).unwrap();
        for p_state in ours.states_inserted {
            let _ = epoch_data.ensemble.remove_state(p_state);
        }
        for p_state in ours.assertions.bits {
            let _ = epoch_data.ensemble.remove_state(p_state);
        }
    }

    pub fn set_as_current(&self) {
        CURRENT_EPOCH.with(|top| {
            let mut current = top.borrow_mut();
            if let Some(current) = current.take() {
                EPOCH_STACK.with(|top| {
                    let mut stack = top.borrow_mut();
                    stack.push(current);
                })
            }
            *current = Some(self.clone());
        });
    }

    pub fn remove_as_current(&self) {
        EPOCH_STACK.with(|top| {
            let mut stack = top.borrow_mut();
            if let Some(next_current) = stack.pop() {
                CURRENT_EPOCH.with(|top| {
                    let mut current = top.borrow_mut();
                    if let Some(to_drop) = current.take() {
                        if !Rc::ptr_eq(&to_drop.epoch_data, &self.epoch_data) {
                            panic!(
                                "tried to drop an `Epoch` out of stacklike order before dropping \
                                 the current one"
                            );
                        }
                        *current = Some(next_current);
                    } else {
                        // there should be something current if the `Epoch` still exists
                        unreachable!()
                    }
                });
            } else {
                CURRENT_EPOCH.with(|top| {
                    let mut current = top.borrow_mut();
                    if let Some(to_drop) = current.take() {
                        if !Rc::ptr_eq(&to_drop.epoch_data, &self.epoch_data) {
                            panic!(
                                "tried to drop an `Epoch` out of stacklike order before dropping \
                                 the current one"
                            );
                        }
                    } else {
                        // there should be something current if the `Epoch` still exists
                        unreachable!()
                    }
                });
            }
        });
    }
}

thread_local!(
    /// We have this separate from `EPOCH_STACK` to minimize the assembly needed
    /// to access the data.
    static CURRENT_EPOCH: RefCell<Option<EpochShared>> = RefCell::new(None);

    /// Epochs lower than the current one
    static EPOCH_STACK: RefCell<Vec<EpochShared>> = RefCell::new(vec![]);
);

#[must_use]
pub fn get_current_epoch() -> Option<EpochShared> {
    CURRENT_EPOCH.with(|top| {
        let top = top.borrow();
        top.clone()
    })
}

/// Do no call recursively.
fn no_recursive_current_epoch<T, F: FnMut(&EpochShared) -> T>(mut f: F) -> T {
    CURRENT_EPOCH.with(|top| {
        let top = top.borrow();
        if let Some(current) = top.as_ref() {
            f(current)
        } else {
            panic!("There needs to be an `Epoch` in scope for this to work");
        }
    })
}

/// Do no call recursively.
fn no_recursive_current_epoch_mut<T, F: FnMut(&mut EpochShared) -> T>(mut f: F) -> T {
    CURRENT_EPOCH.with(|top| {
        let mut top = top.borrow_mut();
        if let Some(current) = top.as_mut() {
            f(current)
        } else {
            panic!("There needs to be an `Epoch` in scope for this to work");
        }
    })
}

#[doc(hidden)]
pub fn _callback() -> EpochCallback {
    fn new_pstate(nzbw: NonZeroUsize, op: Op<PState>, location: Option<Location>) -> PState {
        no_recursive_current_epoch_mut(|current| {
            let mut epoch_data = current.epoch_data.borrow_mut();
            let keep = epoch_data.keep_flag;
            let p_state = epoch_data
                .ensemble
                .make_state(nzbw, op.clone(), location, keep);
            epoch_data
                .responsible_for
                .get_mut(current.p_self)
                .unwrap()
                .states_inserted
                .push(p_state);
            p_state
        })
    }
    fn register_assertion_bit(bit: dag::bool, location: Location) {
        // need a new bit to attach location data to
        let new_bit = new_pstate(bw(1), Op::Copy([bit.state()]), Some(location));
        no_recursive_current_epoch_mut(|current| {
            let mut epoch_data = current.epoch_data.borrow_mut();
            epoch_data
                .responsible_for
                .get_mut(current.p_self)
                .unwrap()
                .assertions
                .bits
                .push(new_bit);
        })
    }
    fn get_nzbw(p_state: PState) -> NonZeroUsize {
        no_recursive_current_epoch(|current| {
            current
                .epoch_data
                .borrow()
                .ensemble
                .stator
                .states
                .get(p_state)
                .unwrap()
                .nzbw
        })
    }
    fn get_op(p_state: PState) -> Op<PState> {
        no_recursive_current_epoch(|current| {
            current
                .epoch_data
                .borrow()
                .ensemble
                .stator
                .states
                .get(p_state)
                .unwrap()
                .op
                .clone()
        })
    }
    EpochCallback {
        new_pstate,
        register_assertion_bit,
        get_nzbw,
        get_op,
    }
}

pub struct Epoch {
    shared: EpochShared,
}

impl Drop for Epoch {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            self.shared.remove_associated();
            self.shared.remove_as_current();
        }
    }
}

impl Epoch {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let new = EpochShared::new();
        new.set_as_current();
        Self { shared: new }
    }

    /// The epoch from this can be dropped out of order from `other`,
    /// but must be dropped before others that aren't also shared
    pub fn shared_with(other: &Epoch) -> Self {
        let shared = EpochShared::shared_with(&other.shared);
        shared.set_as_current();
        Self { shared }
    }

    /// Gets the assertions associated with this Epoch (not including assertions
    /// from when sub-epochs are alive or from before the this Epoch was
    /// created)
    pub fn assertions(&self) -> Assertions {
        self.shared.assertions()
    }

    pub fn ensemble(&self) -> Ensemble {
        self.shared.ensemble()
    }

    pub fn optimize(&self) -> Result<(), EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        if !Rc::ptr_eq(&epoch_shared.epoch_data, &self.shared.epoch_data) {
            return Err(EvalError::OtherStr("epoch is not the current epoch"))
        }
        Ensemble::lower_all(&epoch_shared)?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.optimize_all();
        Ok(())
    }
}
