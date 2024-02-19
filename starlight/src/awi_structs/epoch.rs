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
        epoch::{EpochCallback, EpochKey, _get_epoch_stack},
        triple_arena::{ptr_struct, Arena},
        Lineage, Location, Op, PState,
    },
    bw, dag,
};

use crate::{
    ensemble::{Delay, Ensemble, Value},
    Error, EvalAwi,
};

/// A list of single bit `EvalAwi`s for assertions
#[derive(Debug)]
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
    // this is used primarily in shared epoch situations like the meta lowerer where there is a
    // subroutine where states are created that can be removed when the subroutine is done
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
        // do nothing with the `EpochKey`
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
    /// Creates a new `EpochData` that is not registered anywhere yet.
    pub fn new() -> Self {
        let mut epoch_data = EpochData {
            epoch_key: None,
            ensemble: Ensemble::new(),
            responsible_for: Arena::new(),
        };
        let p_self = epoch_data.responsible_for.insert(PerEpochShared::new());
        Self {
            epoch_data: Rc::new(RefCell::new(epoch_data)),
            p_self,
        }
    }

    /// Does _not_ register anything, instead adds a new
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
    /// stack and also registers a new `EpochCallback` for the `awint_dag` stack
    /// if not already registered
    pub fn set_as_current(&self) {
        let mut lock = self.epoch_data.borrow_mut();
        if lock.epoch_key.is_none() {
            lock.epoch_key = Some(_callback().push_on_epoch_stack());
        }
        drop(lock);
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
    /// starlight stack. Calls `EpochKey::pop_off_epoch_stack` when
    /// `responsible_for.is_empty()`, meaning that `drop_associated` should be
    /// called before this function if needed. Returns an error if there is no
    /// current `EpochShared` or `self.epoch_data` did not match the
    /// current.
    pub fn remove_as_current(&self) -> Result<(), Error> {
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
                        Err(Error::OtherStr(
                            "tried to drop or suspend an `Epoch` out of stacklike order before \
                             dropping or suspending the current `Epoch`",
                        ))
                    }
                } else {
                    Err(Error::OtherStr(
                        "`remove_as_current` encountered no current `EpochShared`, which should \
                         not be possible if an `Epoch` still exists",
                    ))
                }
            })
        })?;
        let mut lock = self.epoch_data.borrow_mut();
        if lock.responsible_for.is_empty() {
            // we are the last `EpochShared`
            match lock.epoch_key.take().unwrap().pop_off_epoch_stack() {
                Ok(()) => Ok(()),
                Err((self_gen, top_gen)) => Err(Error::OtherString(format!(
                    "The last `starlight::Epoch` or `starlight::SuspendedEpoch` of a group of one \
                     or more shared `Epoch`s was dropped out of stacklike order, such that an \
                     `awint_dag::epoch::EpochKey` with generation {} was attempted to be dropped \
                     before the current key with generation {}. This may be because explicit \
                     `drop`s of `Epoch`s should be used in a different order.",
                    self_gen, top_gen
                ))),
            }
        } else {
            Ok(())
        }
    }

    /// Removes states and drops assertions from the `Ensemble` that were
    /// associated with this particular `EpochShared`.
    ///
    /// This function should not be called more than once per `self.p_self`.
    pub fn drop_associated(&self) -> Result<(), Error> {
        let mut lock = self.epoch_data.borrow_mut();
        if let Some(mut ours) = lock.responsible_for.remove(self.p_self) {
            let assertion_bits = mem::take(&mut ours.assertions.bits);
            drop(lock);
            // drop the `EvalAwi`s
            drop(assertion_bits);
            // the virtual cleanup with `states_inserted` happens here
            let mut lock = self.epoch_data.borrow_mut();
            for p_state in ours.states_inserted.iter().copied() {
                let _ = lock.ensemble.remove_state_if_pruning_allowed(p_state);
            }
            drop(lock);
            // drop the rest
            drop(ours);
            Ok(())
        } else {
            Err(Error::OtherStr(
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
        // need to indirectly clone to avoid double borrow
        let epoch_data = self.epoch_data.borrow();
        let bits = &epoch_data
            .responsible_for
            .get(p_self)
            .unwrap()
            .assertions
            .bits;
        let mut p_externals = vec![];
        for bit in bits {
            p_externals.push(bit.p_external());
        }
        drop(epoch_data);
        let mut cloned = vec![];
        for bit in p_externals {
            cloned.push(EvalAwi::try_clone_from(bit).unwrap());
        }
        Assertions { bits: cloned }
    }

    /// This evaluates all associated assertions of this `EpochShared`
    /// (returning an error if any are false, and returning an error on
    /// unevaluatable assertions if `strict`), and eliminates assertions
    /// that evaluate to a constant true.
    pub fn assert_assertions(&self, strict: bool) -> Result<(), Error> {
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
            let p_external = eval_awi.p_external();
            drop(epoch_data);
            let val = Ensemble::request_thread_local_rnode_value(p_external, 0)?;
            if let Some(val) = val.known_value() {
                if !val {
                    return Err(Error::OtherString(format!(
                        "an assertion bit evaluated to false, failed on {p_external:#?}"
                    )))
                }
            } else if unknown.is_none() {
                // get the earliest failure to evaluate, should be closest to the root cause.
                // Wait for all bits to be checked for falsity
                unknown = Some(p_external);
            }
            if (val == Value::ConstUnknown) && strict && unknown.is_none() {
                unknown = Some(p_external);
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
            if let Some(p_external) = unknown {
                return Err(Error::OtherString(format!(
                    "an assertion bit could not be evaluated to a known value, failed on \
                     {p_external:#?}"
                )))
            }
        }
        Ok(())
    }

    fn internal_run_with_lower_capability(&self, time: Delay) -> Result<(), Error> {
        // `Loop`s register states to lower so that the old handle process is not needed
        Ensemble::handle_states_to_lower(self)?;
        // first evaluate all loop drivers
        let mut lock = self.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        ensemble.run(time)
    }

    fn internal_run(&self, time: Delay) -> Result<(), Error> {
        // first evaluate all loop drivers
        let mut lock = self.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        ensemble.run(time)
    }
}

thread_local!(
    /// We have this separate from `EPOCH_STACK` to minimize the assembly needed
    /// to access the data.
    static CURRENT_EPOCH: RefCell<Option<EpochShared>> = RefCell::new(None);

    /// Epochs lower than the current one
    static EPOCH_STACK: RefCell<Vec<EpochShared>> = RefCell::new(vec![]);
);

/// Returns a clone of the current `EpochShared`, or return
/// `Error::NoCurrentlyActiveEpoch` if there is none
pub fn get_current_epoch() -> Result<EpochShared, Error> {
    CURRENT_EPOCH
        .with(|top| {
            let top = top.borrow();
            top.clone()
        })
        .ok_or(Error::NoCurrentlyActiveEpoch)
}

pub fn debug_epoch_stack() {
    println!("awint epoch stack: {:?}", _get_epoch_stack());
    CURRENT_EPOCH.with(|top| {
        let top = top.borrow();
        if let Some(x) = top.as_ref() {
            println!("starlight current: {x:?}");
        } else {
            println!("no current epoch on starlight stack");
        }
    });
    EPOCH_STACK.with(|top| {
        let top = top.borrow();
        for (i, x) in top.iter().rev().enumerate() {
            println!("starlight stack depth {i}+1: {x:?}");
        }
    });
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
        let need_register = if let Some(awi) = bit.state().try_get_as_awi() {
            assert_eq!(awi.bw(), 1);
            // only need to register false bits so the location can get propogated
            awi.is_zero()
        } else {
            true
        };
        if need_register {
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
                    panic!(
                        "there needs to be an `Epoch` in scope for assertion registration to work"
                    );
                }
            })
        }
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
                .expect(
                    "probably, an `awint_dag`/`starlight` mimicking type was operated on in the \
                     wrong `Epoch`",
                )
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
                .expect(
                    "probably, an `awint_dag`/`starlight` mimicking type was operated on in the \
                     wrong `Epoch`",
                )
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
    is_suspended: bool,
}

impl Drop for EpochInnerDrop {
    // track_caller does not work for `Drop`
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Err(e) = self.epoch_shared.drop_associated() {
                panic!("{e}");
            }
            if !self.is_suspended {
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
/// // suspended epochs work the same with
/// // respect tosuspend and resume points
/// let epoch0 = Epoch::new();
/// // `epoch0` is current
/// let suspended_epoch0 = epoch0.suspend();
/// // no epoch is current
/// let epoch1 = Epoch::new();
/// // `epoch1` is current
/// let epoch0 = suspended_epoch0.resume();
/// // `epoch0` is current
/// //drop(epoch1); // not here
/// let suspended_epoch0 = epoch0.suspend();
/// // `epoch1` is current
/// drop(epoch1);
/// // no epoch is current
/// // use `SuspendedEpoch`s to restart at any point
/// let epoch0 = suspended_epoch0.resume();
/// // `epoch0` is current
/// let suspended_epoch0 = epoch0.suspend();
/// // no epoch is current
/// let epoch1 = Epoch::new();
/// // `epoch1` is current
/// // could be done at any later point except
/// // in some shared cases
/// drop(suspended_epoch0);
/// drop(epoch1);
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
        self.inner.is_suspended = false;
        Epoch { inner: self.inner }
    }

    /// Returns the `EpochShared` of `self`
    fn shared(&self) -> &EpochShared {
        &self.inner.epoch_shared
    }

    pub fn ensemble<O, F: FnMut(&Ensemble) -> O>(&self, f: F) -> O {
        self.shared().ensemble(f)
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
                is_suspended: false,
            },
        }
    }

    /// Creates an `Epoch` that shares the `Ensemble` of `other`
    ///
    /// The epoch from this can be dropped out of order from `other`,
    /// but the shared group of `Epoch`s as a whole must follow the stacklike
    /// drop order described in the documentation of `Epoch`.
    pub fn shared_with(other: &Epoch) -> Self {
        let shared = EpochShared::shared_with(other.shared());
        shared.set_as_current();
        Self {
            inner: EpochInnerDrop {
                epoch_shared: shared,
                is_suspended: false,
            },
        }
    }

    /// Returns the `EpochShared` of `self`
    fn shared(&self) -> &EpochShared {
        &self.inner.epoch_shared
    }

    /// Checks if `self.shared()` is the same as the current epoch, and returns
    /// the `EpochShared` if so. Returns `NoCurrentlyActiveEpoch` or
    /// `WrongCurrentlyActiveEpoch` depending on error conditions.
    fn check_current(&self) -> Result<EpochShared, Error> {
        let epoch_shared = get_current_epoch()?;
        if Rc::ptr_eq(&epoch_shared.epoch_data, &self.shared().epoch_data) {
            Ok(self.shared().clone())
        } else {
            Err(Error::WrongCurrentlyActiveEpoch)
        }
    }

    /// Suspends the `Epoch` from being the current epoch temporarily.
    ///
    /// # Panics
    ///
    /// Panics if `self` is not the current `Epoch`
    #[track_caller]
    pub fn suspend(mut self) -> SuspendedEpoch {
        // In case of an error, the `Epoch` would need to drop which would cause a
        // different panic. I would rather not inflate the `Error` enum just to contain
        // an `Epoch` for this case, instead we will panic here.
        self.inner.epoch_shared.remove_as_current().unwrap();
        self.inner.is_suspended = true;
        SuspendedEpoch { inner: self.inner }
    }

    pub fn ensemble<O, F: FnMut(&Ensemble) -> O>(&self, f: F) -> O {
        self.shared().ensemble(f)
    }

    pub fn clone_ensemble(&self) -> Ensemble {
        self.ensemble(|ensemble| ensemble.clone())
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
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
    /// Requires that `self` be the current `Epoch`.
    pub fn assert_assertions(&self, strict: bool) -> Result<(), Error> {
        let epoch_shared = self.check_current()?;
        epoch_shared.assert_assertions(strict)
    }

    /// Removes all states that do not lead to a live `EvalAwi`, and loosely
    /// evaluates assertions. Requires
    /// that `self` be the current `Epoch`.
    pub fn prune_unused_states(&self) -> Result<(), Error> {
        let epoch_shared = self.check_current()?;
        // get rid of constant assertions
        let _ = epoch_shared.assert_assertions(false);
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.prune_unused_states()
    }

    /// Lowers states internally into `LNode`s and `TNode`s, for trees of
    /// `RNode`s that need it. This is not needed in most circumstances,
    /// `EvalAwi` and optimization functions do this on demand. Requires
    /// that `self` be the current `Epoch`.
    pub fn lower(&self) -> Result<(), Error> {
        let epoch_shared = self.check_current()?;
        Ensemble::handle_states_to_lower(&epoch_shared)?;
        Ensemble::lower_for_rnodes(&epoch_shared)?;
        let _ = epoch_shared.assert_assertions(false);
        Ok(())
    }

    /// Aggressively prunes all states, lowering `RNode`s for `EvalAwi`s and
    /// `LazyAwi`s if necessary and evaluating assertions. Requires that `self`
    /// be the current `Epoch`.
    pub fn lower_and_prune(&self) -> Result<(), Error> {
        let epoch_shared = self.check_current()?;
        Ensemble::handle_states_to_lower(&epoch_shared)?;
        Ensemble::lower_for_rnodes(&epoch_shared)?;
        // get rid of constant assertions
        let _ = epoch_shared.assert_assertions(false);
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.force_remove_all_states()
    }

    /// Runs optimization including lowering then pruning all states. Requires
    /// that `self` be the current `Epoch`.
    pub fn optimize(&self) -> Result<(), Error> {
        let epoch_shared = self.check_current()?;
        Ensemble::handle_states_to_lower(&epoch_shared)?;
        Ensemble::lower_for_rnodes(&epoch_shared).unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble.optimize_all().unwrap();
        drop(lock);
        let _ = epoch_shared.assert_assertions(false);
        Ok(())
    }

    /// Evaluates temporal nodes according to their delays until `time` has
    /// passed. Requires that `self` be the current `Epoch`.
    pub fn run<D: Into<Delay>>(&self, time: D) -> Result<(), Error> {
        let epoch_shared = self.check_current()?;
        if epoch_shared
            .epoch_data
            .borrow()
            .ensemble
            .stator
            .states
            .is_empty()
        {
            epoch_shared.internal_run(time.into())
        } else {
            epoch_shared.internal_run_with_lower_capability(time.into())
        }
    }

    /// Returns if the `Epoch` is in a quiescent state, i.e. the internal
    /// temporal event queue is empty and there will be no value changes if
    /// `Epoch::run` is used. Requires that `self` be the current `Epoch`.
    pub fn quiesced(&self) -> Result<bool, Error> {
        // the reason for this signature is that we don't want the user to have the
        // responsibility of emptying the zero delay queue to know for sure that there
        // is actual quiescent, there are too many gotchas about what happens if the
        // `Epoch` is in a logically quiescent state but eval has not happened or
        // happened to the point that it stopped at a certain delay and did not
        // empty the zero delay queue because normal `EvalAwi` evals would empty
        // it, we need to empty it ourselves here and return an error in case of
        // zero duration infinite loop. This and initial startup cases also
        // implies the need for `check_current`.

        // just call `run` with zero delay, otherwise we have to repeat various lowering
        // cases
        self.run(Delay::zero())?;
        self.ensemble(|ensemble| {
            Ok(ensemble.delayer.delayed_events.is_empty() && ensemble.evaluator.are_events_empty())
        })
    }
}
