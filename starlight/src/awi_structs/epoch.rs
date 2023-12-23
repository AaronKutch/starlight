//! Internals used for epoch management (most users should just be interacting
//! with `Epoch` and `Assertions`)

#![allow(clippy::new_without_default)]

use std::{
    cell::RefCell,
    fmt::Debug,
    mem::{self},
    num::NonZeroUsize,
    rc::Rc,
    thread::panicking,
};

use awint::{
    awint_dag::{
        epoch::{EpochCallback, EpochKey},
        triple_arena::{ptr_struct, Advancer, Arena},
        EvalError, Lineage, Location, Op, PState,
    },
    bw, dag,
};

use crate::{ensemble::Ensemble, EvalAwi};

/// A list of single bit `EvalAwi`s for assertions
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

/// Data stored  in `EpochData` per each live `EpochShared`
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

/// The unit of data that gets a registered `awint_dag` `EpochKey`, and which
/// several `EpochShared`s can share
///
/// # Custom Drop
///
/// This struct should have its `epoch_key` popped off the stack and
/// `responsible_for` emptied before being dropped normally. During a panic, the
/// order of TLS operations is unspecified, and in practice
/// `std::thread::panicking` can return false during the drop code of structs in
/// TLS even if the thread is panicking. So, the drop code for `EpochData` does
/// nothing with the `EpochKey` and `mem::forget`s the `EvalAwi` assertions.
pub struct EpochData {
    pub epoch_key: Option<EpochKey>,
    pub ensemble: Ensemble,
    pub responsible_for: Arena<PEpochShared, PerEpochShared>,
}

impl Drop for EpochData {
    fn drop(&mut self) {
        for (_, mut shared) in self.responsible_for.drain() {
            for eval_awi in shared.assertions.bits.drain(..) {
                // avoid the `EvalAwi` drop code
                mem::forget(eval_awi);
            }
        }
    }
}

impl Debug for EpochData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EpochData")
            .field("epoch_key", &self.epoch_key)
            .field("responsible_for.len()", &self.responsible_for.len())
            .finish()
    }
}

/// The raw internal management struct for `Epoch`s. Most users should be using
/// `Epoch`.
///
/// `awint_dag::epoch` has a stack system which this uses, but this can have its
/// own stack on top of that.
///
/// This raw version of `Epoch` has no drop code and all things need to be
/// carefully handled to avoid virtual leakage or trying to call
/// `remove_as_current` twice.
#[derive(Clone)]
pub struct EpochShared {
    pub epoch_data: Rc<RefCell<EpochData>>,
    pub p_self: PEpochShared,
}

impl Debug for EpochShared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Ok(epoch_data) = self.epoch_data.try_borrow() {
            f.debug_struct("EpochShared")
                .field("epoch_data", &epoch_data)
                .field("p_self", &self.p_self)
                .finish()
        } else {
            f.debug_struct("EpochShared")
                .field(
                    "epoch_data (already borrowed, cannot display in `Debug` impl)",
                    &(),
                )
                .field("p_self", &self.p_self)
                .finish()
        }
    }
}

impl EpochShared {
    /// Creates a new `EpochData` and registers a new `EpochCallback`.
    pub fn new() -> Self {
        let mut epoch_data = EpochData {
            epoch_key: Some(_callback().push_on_epoch_stack()),
            ensemble: Ensemble::new(),
            responsible_for: Arena::new(),
        };
        let p_self = epoch_data.responsible_for.insert(PerEpochShared::new());
        Self {
            epoch_data: Rc::new(RefCell::new(epoch_data)),
            p_self,
        }
    }

    /// Does _not_ register a new `EpochCallback`, instead adds a new
    /// `PerEpochShared` to the current `EpochData` of `other`
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

    /// Sets `self` as the current `EpochShared` with respect to the starlight
    /// stack (does not affect whatever the `awint_dag` stack is doing)
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

    /// Removes `self` as the current `EpochShared` with respect to the
    /// starlight stack. Returns an error if there is no current `EpochShared`
    /// or `self.epoch_data` did not match the current.
    pub fn remove_as_current(&self) -> Result<(), &'static str> {
        EPOCH_STACK.with(|top| {
            let mut stack = top.borrow_mut();
            let next_current = stack.pop();
            CURRENT_EPOCH.with(|top| {
                let mut current = top.borrow_mut();
                if let Some(ref to_drop) = current.take() {
                    if Rc::ptr_eq(&to_drop.epoch_data, &self.epoch_data) {
                        *current = next_current;
                        Ok(())
                    } else {
                        // return the error how most users will trigger it
                        Err(
                            "tried to drop or suspend an `Epoch` out of stacklike order before \
                             dropping or suspending the current `Epoch`",
                        )
                    }
                } else {
                    Err(
                        "`remove_as_current` encountered no current `EpochShared`, which should \
                         not be possible if an `Epoch` still exists",
                    )
                }
            })
        })
    }

    /// Removes states and drops assertions from the `Ensemble` that were
    /// associated with this particular `EpochShared`. This also deregisters the
    /// `EpochCallback` if this was the last `EpochShared` with a
    /// `PerEpochShared` in the `EpochData`.
    ///
    /// This function should not be called more than once per `self.p_self`.
    pub fn drop_associated(&self) -> Result<(), EvalError> {
        let mut lock = self.epoch_data.borrow_mut();
        if let Some(ours) = lock.responsible_for.remove(self.p_self) {
            for p_state in &ours.states_inserted {
                let _ = lock.ensemble.remove_state(*p_state);
            }
            drop(lock);
            // drop the `EvalAwi`s of the assertions after unlocking
            drop(ours);

            let mut lock = self.epoch_data.borrow_mut();
            if lock.responsible_for.is_empty() {
                // we are the last `EpochShared`
                match lock.epoch_key.take().unwrap().pop_off_epoch_stack() {
                    Ok(()) => (),
                    Err((self_gen, top_gen)) => {
                        return Err(EvalError::OtherString(format!(
                            "The last `starlight::Epoch` or `starlight::SuspendedEpoch` of a \
                             group of one or more shared `Epoch`s was dropped out of stacklike \
                             order, such that an `awint_dag::epoch::EpochKey` with generation {} \
                             was attempted to be dropped before the current key with generation \
                             {}. This may be because explicit `drop`s of `Epoch`s should be used \
                             in a different order.",
                            self_gen, top_gen
                        )));
                    }
                }
            }
            Ok(())
        } else {
            Err(EvalError::OtherStr(
                "should be unreachable: called `EpochShared::drop_associated` on the same \
                 `EpochShared`",
            ))
        }
    }

    /// Access to the `Ensemble`
    pub fn ensemble<O, F: FnMut(&Ensemble) -> O>(&self, mut f: F) -> O {
        f(&self.epoch_data.borrow().ensemble)
    }

    /// Takes the `Vec<PState>` corresponding to just states added when the
    /// current `EpochShared` was active. This also means that
    /// `remove_associated` done immediately after this will only remove
    /// assertions, responsibility should be taken over for the `PState`s
    /// returned by this function
    pub fn take_states_added(&mut self) -> Vec<PState> {
        let mut epoch_data = self.epoch_data.borrow_mut();
        let ours = epoch_data.responsible_for.get_mut(self.p_self).unwrap();
        mem::take(&mut ours.states_inserted)
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

    /// This evaluates all assertions (returning an error if any are false, and
    /// returning an error on unevaluatable assertions if `strict`), and
    /// eliminates assertions that evaluate to a constant true.
    pub fn assert_assertions(&self, strict: bool) -> Result<(), EvalError> {
        let p_self = self.p_self;
        let epoch_data = self.epoch_data.borrow();
        let mut len = epoch_data
            .responsible_for
            .get(p_self)
            .unwrap()
            .assertions
            .bits
            .len();
        drop(epoch_data);
        let mut unknown = None;
        let mut i = 0;
        loop {
            if i >= len {
                break
            }
            let epoch_data = self.epoch_data.borrow();
            let eval_awi = &epoch_data
                .responsible_for
                .get(p_self)
                .unwrap()
                .assertions
                .bits[i];
            let p_state = eval_awi.state();
            let p_rnode = eval_awi.p_rnode();
            drop(epoch_data);
            let val = Ensemble::calculate_thread_local_rnode_value(p_rnode, 0)?;
            if let Some(val) = val.known_value() {
                if !val {
                    let epoch_data = self.epoch_data.borrow();
                    let s = epoch_data.ensemble.get_state_debug(p_state);
                    if let Some(s) = s {
                        return Err(EvalError::OtherString(format!(
                            "an assertion bit evaluated to false, failed on {p_rnode} {:?}",
                            s
                        )))
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "an assertion bit evaluated to false, failed on {p_rnode} {p_state}"
                        )))
                    }
                }
            } else if unknown.is_none() {
                // get the earliest failure to evaluate, should be closest to the root cause.
                // Wait for all bits to be checked for falsity
                unknown = Some((p_rnode, p_state));
            }
            if val.is_const() {
                // remove the assertion
                let mut epoch_data = self.epoch_data.borrow_mut();
                let eval_awi = epoch_data
                    .responsible_for
                    .get_mut(p_self)
                    .unwrap()
                    .assertions
                    .bits
                    .swap_remove(i);
                drop(epoch_data);
                drop(eval_awi);
                len -= 1;
            } else {
                i += 1;
            }
        }
        if strict {
            if let Some((p_rnode, p_state)) = unknown {
                let epoch_data = self.epoch_data.borrow();
                let s = epoch_data.ensemble.get_state_debug(p_state);
                if let Some(s) = s {
                    return Err(EvalError::OtherString(format!(
                        "an assertion bit could not be evaluated to a known value, failed on \
                         {p_rnode} {}",
                        s
                    )))
                } else {
                    return Err(EvalError::OtherString(format!(
                        "an assertion bit could not be evaluated to a known value, failed on \
                         {p_rnode} {p_state}"
                    )))
                }
            }
        }
        Ok(())
    }

    fn internal_drive_loops_with_lower_capability(&self) -> Result<(), EvalError> {
        // `Loop`s register states to lower so that the below loops can find them
        Ensemble::handle_requests_with_lower_capability(self)?;
        // first evaluate all loop drivers
        let lock = self.epoch_data.borrow();
        let mut adv = lock.ensemble.tnodes.advancer();
        drop(lock);
        loop {
            let lock = self.epoch_data.borrow();
            if let Some(p_tnode) = adv.advance(&lock.ensemble.tnodes) {
                let tnode = lock.ensemble.tnodes.get(p_tnode).unwrap();
                let p_driver = tnode.p_driver;
                drop(lock);
                Ensemble::calculate_value_with_lower_capability(self, p_driver)?;
            } else {
                break
            }
        }
        // second do all loopback changes
        let mut lock = self.epoch_data.borrow_mut();
        let mut adv = lock.ensemble.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&lock.ensemble.tnodes) {
            let tnode = lock.ensemble.tnodes.get(p_tnode).unwrap();
            let val = lock.ensemble.backrefs.get_val(tnode.p_driver).unwrap().val;
            let p_self = tnode.p_self;
            lock.ensemble.change_value(p_self, val).unwrap();
        }
        Ok(())
    }

    fn internal_drive_loops(&self) -> Result<(), EvalError> {
        // first evaluate all loop drivers
        let mut lock = self.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;

        let mut adv = ensemble.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&ensemble.tnodes) {
            let tnode = ensemble.tnodes.get(p_tnode).unwrap();
            let p_driver = tnode.p_driver;
            ensemble.calculate_value(p_driver)?;
        }
        // second do all loopback changes
        let mut adv = ensemble.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&ensemble.tnodes) {
            let tnode = ensemble.tnodes.get(p_tnode).unwrap();
            let val = ensemble.backrefs.get_val(tnode.p_driver).unwrap().val;
            let p_self = tnode.p_self;
            ensemble.change_value(p_self, val).unwrap();
        }
        Ok(())
    }
}

thread_local!(
    /// We have this separate from `EPOCH_STACK` to minimize the assembly needed
    /// to access the data.
    static CURRENT_EPOCH: RefCell<Option<EpochShared>> = RefCell::new(None);

    /// Epochs lower than the current one
    static EPOCH_STACK: RefCell<Vec<EpochShared>> = RefCell::new(vec![]);
);

/// Returns a clone of the current `EpochShared`, or return `None` if there is
/// none
#[must_use]
pub fn get_current_epoch() -> Option<EpochShared> {
    CURRENT_EPOCH.with(|top| {
        let top = top.borrow();
        top.clone()
    })
}

/// Allows access to the current epoch. Do no call recursively.
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

/// Allows mutable access to the current epoch. Do no call recursively.
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
            let p_state = epoch_data.ensemble.make_state(nzbw, op.clone(), location);
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
        let eval_awi = EvalAwi::from_state(new_bit);
        // manual to get around closure issue
        CURRENT_EPOCH.with(|top| {
            let mut top = top.borrow_mut();
            if let Some(current) = top.as_mut() {
                let mut epoch_data = current.epoch_data.borrow_mut();
                epoch_data
                    .responsible_for
                    .get_mut(current.p_self)
                    .unwrap()
                    .assertions
                    .bits
                    .push(eval_awi);
            } else {
                panic!("There needs to be an `Epoch` in scope for this to work");
            }
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

/// Has the actual drop code attached, preventing the need for unsafe or a
/// nonzero cost abstraction somewhere
#[derive(Debug)]
struct EpochInnerDrop {
    epoch_shared: EpochShared,
    is_current: bool,
}

impl Drop for EpochInnerDrop {
    // track_caller does not work for `Drop`
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Err(e) = self.epoch_shared.drop_associated() {
                panic!("{e}");
            }
            if self.is_current {
                if let Err(e) = self.epoch_shared.remove_as_current() {
                    panic!("panicked upon dropping an `Epoch`: {e}");
                }
            }
        }
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
/// # Custom Drop
///
/// Upon being dropped, this will remove states that were associated with this
/// epoch, completely removing the `Ensemble` if there are no other `Epoch`s
/// shared with this one, and deregistering this as the current `Epoch`.
///
/// The lifetimes of `Epoch` structs should be stacklike, such that a
/// `Epoch` created during the lifetime of another `Epoch` should be
/// dropped before the older `Epoch` is dropped, otherwise a panic occurs.
///
/// ```
/// use starlight::Epoch;
///
/// // let epoch0 = Epoch::new();
/// // // `epoch0` is the current epoch
/// // let epoch1 = Epoch::new();
/// // // `epoch1` is the current epoch
/// // drop(epoch0); // panics here because `epoch1` was created during `epoch0`
/// // drop(epoch1);
///
/// // this succeeds
/// let epoch0 = Epoch::new();
/// // `epoch0` is current
/// let epoch1 = Epoch::new();
/// // `epoch1` is current
/// drop(epoch1);
/// // `epoch0` is current
/// let epoch2 = Epoch::new();
/// // `epoch2` is current
/// let epoch3 = Epoch::new();
/// // `epoch3` is current
/// drop(epoch3);
/// // `epoch2` is current
/// drop(epoch2);
/// // `epoch0` is current
/// drop(epoch0);
///
/// // these could be dropped in any order relative to one
/// // another because they share the same `Ensemble` and
/// // `awint_dag` mimicking types callback registration,
/// let epoch0 = Epoch::new();
/// let subepoch0 = Epoch::shared_with(&epoch0);
/// drop(epoch0);
/// // but the last one to be dropped has the restriction
/// // with respect to an independent `Epoch`
/// let epoch1 = Epoch::new();
/// //drop(subepoch0); // would panic
/// drop(epoch1);
/// drop(subepoch0);
/// ```
///
/// Using `mem::forget` or similar on a `Epoch` will leak `State`s and
/// cause them to not be cleaned up, and will also likely cause panics because
/// of the stack requirement.
#[derive(Debug)]
pub struct Epoch {
    inner: EpochInnerDrop,
}

/// Represents a suspended epoch
///
/// # Custom Drop
///
/// Upon being dropped, this will have the effect of dropping the `Epoch` this
/// was created from (except the fact of which epoch is current is not changed).
#[derive(Debug)]
pub struct SuspendedEpoch {
    inner: EpochInnerDrop,
}

impl SuspendedEpoch {
    /// Resumes the `Epoch` as current
    pub fn resume(mut self) -> Epoch {
        self.inner.epoch_shared.set_as_current();
        self.inner.is_current = true;
        Epoch { inner: self.inner }
    }
}

impl Epoch {
    /// Creates a new `Epoch` with an independent `Ensemble`
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let new = EpochShared::new();
        new.set_as_current();
        Self {
            inner: EpochInnerDrop {
                epoch_shared: new,
                is_current: true,
            },
        }
    }

    /// Creates an `Epoch` that shares the `Ensemble` of `other`
    ///
    /// The epoch from this can be dropped out of order from `other`,
    /// but the shared group of `Epoch`s as a whole must follow the stacklike
    /// drop order described in the documentation of `Epoch`.
    pub fn shared_with(other: &Epoch) -> Self {
        let shared = EpochShared::shared_with(&other.shared());
        shared.set_as_current();
        Self {
            inner: EpochInnerDrop {
                epoch_shared: shared,
                is_current: true,
            },
        }
    }

    /// Returns the `EpochShared` of `self`
    fn shared(&self) -> &EpochShared {
        &self.inner.epoch_shared
    }

    /// Checks if `self.shared()` is the same as the current epoch, and returns
    /// the `EpochShared` if so
    fn check_current(&self) -> Result<EpochShared, EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        if Rc::ptr_eq(&epoch_shared.epoch_data, &self.shared().epoch_data) {
            Ok(epoch_shared)
        } else {
            Err(EvalError::OtherStr("epoch is not the current epoch"))
        }
    }

    /// Suspends the `Epoch` from being the current epoch temporarily. Returns
    /// an error if `self` is not the current `Epoch`.
    pub fn suspend(mut self) -> Result<SuspendedEpoch, EvalError> {
        // TODO in the `EvalError` redo (probably needs a `starlight` side `EvalError`),
        // there should be a variant that returns the `Epoch` to prevent it from being
        // dropped and causing another error
        self.inner.epoch_shared.remove_as_current().unwrap();
        self.inner.is_current = false;
        Ok(SuspendedEpoch { inner: self.inner })
    }

    pub fn ensemble<O, F: FnMut(&Ensemble) -> O>(&self, f: F) -> O {
        self.shared().ensemble(f)
    }

    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        self.ensemble(|ensemble| ensemble.verify_integrity())
    }

    /// Gets the assertions associated with this Epoch (not including assertions
    /// from when sub-epochs are alive or from before the this Epoch was
    /// created)
    pub fn assertions(&self) -> Assertions {
        self.shared().assertions()
    }

    /// If any assertion bit evaluates to false, this returns an error. If
    /// `strict` and an assertion could not be evaluated to a known value, this
    /// also returns an error. Prunes assertions evaluated to a constant true.
    pub fn assert_assertions(&self, strict: bool) -> Result<(), EvalError> {
        self.shared().assert_assertions(strict)
    }

    /// Removes all states that do not lead to a live `EvalAwi`, and loosely
    /// evaluates assertions.
    pub fn prune(&self) -> Result<(), EvalError> {
        let epoch_shared = self.check_current()?;
        // get rid of constant assertions
        let _ = epoch_shared.assert_assertions(false);
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.prune_states()
    }

    /// Lowers all states internally into `LNode`s and `TNode`s. This is not
    /// needed in most circumstances, `EvalAwi` and optimization functions
    /// do this on demand.
    pub fn lower(&self) -> Result<(), EvalError> {
        let epoch_shared = self.check_current()?;
        Ensemble::lower_all(&epoch_shared)?;
        let _ = epoch_shared.assert_assertions(false);
        Ok(())
    }

    /// Runs optimization including lowering then pruning all states.
    pub fn optimize(&self) -> Result<(), EvalError> {
        let epoch_shared = self.check_current()?;
        Ensemble::lower_all(&epoch_shared)?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.optimize_all();
        drop(lock);
        let _ = epoch_shared.assert_assertions(false);
        Ok(())
    }

    /// This evaluates all loop drivers, and then registers loopback changes
    pub fn drive_loops(&self) -> Result<(), EvalError> {
        let epoch_shared = self.check_current()?;
        if epoch_shared
            .epoch_data
            .borrow()
            .ensemble
            .stator
            .states
            .is_empty()
        {
            epoch_shared.internal_drive_loops()
        } else {
            epoch_shared.internal_drive_loops_with_lower_capability()
        }
    }
}
