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

use crate::{ensemble::Ensemble, EvalAwi};

#[derive(Debug, Clone)]
pub struct Assertions {
    pub bits: Vec<EvalAwi>,
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

/// # Custom Drop
///
/// This deregisters the `awint_dag::epoch::EpochKey` upon being dropped
#[derive(Debug)]
pub struct EpochData {
    pub epoch_key: EpochKey,
    pub ensemble: Ensemble,
    pub responsible_for: Arena<PEpochShared, PerEpochShared>,
    pub keep_flag: bool,
}

impl Drop for EpochData {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            self.epoch_key.pop_off_epoch_stack();
        }
    }
}

// `awint_dag::epoch` has a stack system which this uses, but this can have its
// own stack on top of that.
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
        // need to indirectly clone
        let epoch_data = self.epoch_data.borrow();
        let bits = &epoch_data
            .responsible_for
            .get(p_self)
            .unwrap()
            .assertions
            .bits;
        let mut states = vec![];
        for bit in bits {
            states.push(bit.state())
        }
        drop(epoch_data);
        let mut cloned = vec![];
        for p_state in states {
            cloned.push(EvalAwi::from_state(p_state))
        }
        Assertions { bits: cloned }
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
        let mut ours = epoch_data.responsible_for.remove(self.p_self).unwrap();
        for p_state in ours.states_inserted {
            let _ = epoch_data.ensemble.remove_state(p_state);
        }
        ours.assertions.bits.clear();
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
pub fn no_recursive_current_epoch<T, F: FnMut(&EpochShared) -> T>(mut f: F) -> T {
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
pub fn no_recursive_current_epoch_mut<T, F: FnMut(&mut EpochShared) -> T>(mut f: F) -> T {
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
        // need a new bit to attach new location data to
        let new_bit = new_pstate(bw(1), Op::Assert([bit.state()]), Some(location));
        no_recursive_current_epoch_mut(|current| {
            let mut epoch_data = current.epoch_data.borrow_mut();
            // need to manually construct to get around closure issues
            let p_note = epoch_data.ensemble.note_pstate(new_bit).unwrap();
            let eval_awi = EvalAwi {
                p_state: new_bit,
                p_note,
            };
            epoch_data
                .responsible_for
                .get_mut(current.p_self)
                .unwrap()
                .assertions
                .bits
                .push(eval_awi);
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

/// Manages the lifetimes and assertions of `State`s created by mimicking types.
///
/// During the lifetime of a `Epoch` struct, all thread local `State`s
/// created will be kept until the struct is dropped, in which case the capacity
/// for those states are reclaimed and their `PState`s are invalidated.
///
/// Additionally, assertion bits from [crate::dag::assert],
/// [crate::dag::assert_eq], [crate::dag::Option::unwrap], etc are
/// associated with the top level `Epoch` alive at the time they are
/// created. Use [Epoch::assertions] to acquire these.
///
/// The internal `Ensemble` can be freed from any non-`Send`, non-`Sync`, and
/// other thread local restrictions once all states have been lowered.
/// [Epoch::ensemble] can be called to get it.
///
/// # Panics
///
/// The lifetimes of `Epoch` structs should be stacklike, such that a
/// `Epoch` created during the lifetime of another `Epoch` should be
/// dropped before the older `Epoch` is dropped, otherwise a panic occurs.
///
/// Using `mem::forget` or similar on a `Epoch` will leak `State`s and
/// cause them to not be cleaned up, and will also likely cause panics because
/// of the stack requirement.
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

    /// Intended primarily for developer use
    pub fn internal_epoch_shared(&self) -> &EpochShared {
        &self.shared
    }

    /// Gets the assertions associated with this Epoch (not including assertions
    /// from when sub-epochs are alive or from before the this Epoch was
    /// created)
    pub fn assertions(&self) -> Assertions {
        self.shared.assertions()
    }

    /// If any assertion bit evaluates to false, this returns an error.
    // TODO fix the enum situation
    pub fn assert_assertions(&self) -> Result<(), EvalError> {
        let bits = self.shared.assertions().bits;
        for eval_awi in bits {
            let val = eval_awi.eval_bit()?;
            if let Some(val) = val.known_value() {
                if !val {
                    return Err(EvalError::OtherString(format!(
                        "an assertion bit evaluated to false, failed on {}",
                        self.shared
                            .epoch_data
                            .borrow()
                            .ensemble
                            .get_state_debug(eval_awi.state())
                            .unwrap()
                    )))
                }
            }
        }
        Ok(())
    }

    /// If any assertion bit evaluates to false, this returns an error. If there
    /// were no known false assertions but some are `Value::Unknown`, this
    /// returns a specific error for it.
    // TODO fix the enum situation
    pub fn assert_assertions_strict(&self) -> Result<(), EvalError> {
        let bits = self.shared.assertions().bits;
        let mut unknown = None;
        for eval_awi in bits {
            let val = eval_awi.eval_bit()?;
            if let Some(val) = val.known_value() {
                if !val {
                    return Err(EvalError::OtherString(format!(
                        "assertion bits are not all true, failed on {}",
                        self.shared
                            .epoch_data
                            .borrow()
                            .ensemble
                            .get_state_debug(eval_awi.state())
                            .unwrap()
                    )))
                }
            } else if unknown.is_none() {
                // get the earliest failure to evaluate wait for all bits to be checked for
                // falsity
                unknown = Some(eval_awi.p_state);
            }
        }
        if let Some(p_state) = unknown {
            Err(EvalError::OtherString(format!(
                "an assertion bit could not be evaluated to a known value, failed on {}",
                self.shared
                    .epoch_data
                    .borrow()
                    .ensemble
                    .get_state_debug(p_state)
                    .unwrap()
            )))
        } else {
            Ok(())
        }
    }

    pub fn ensemble(&self) -> Ensemble {
        self.shared.ensemble()
    }

    /// Removes all non-noted states
    pub fn prune(&self) -> Result<(), EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        if !Rc::ptr_eq(&epoch_shared.epoch_data, &self.shared.epoch_data) {
            return Err(EvalError::OtherStr("epoch is not the current epoch"))
        }
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.prune_unnoted_states()
    }

    /// Lowers all states. This is not needed in most circumstances, `EvalAwi`
    /// and optimization functions do this on demand.
    pub fn lower(&self) -> Result<(), EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        if !Rc::ptr_eq(&epoch_shared.epoch_data, &self.shared.epoch_data) {
            return Err(EvalError::OtherStr("epoch is not the current epoch"))
        }
        Ensemble::lower_all(&epoch_shared)
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

    // TODO
    //pub fn prune_nonnoted
}
