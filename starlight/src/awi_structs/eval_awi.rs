use std::{fmt, num::NonZeroUsize, thread::panicking};

use awint::{
    awint_dag::{dag, epoch, EvalError, Lineage, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{
    awi,
    ensemble::{Ensemble, PNote},
    epoch::get_current_epoch,
};

/// When created from a type implementing `AsRef<dag::Bits>`, it can later be
/// used to evaluate its dynamic value.
///
/// This will keep the source tree alive in case of pruning.
///
/// # Custom Drop
///
/// Upon being dropped, this will remove special references being kept by the
/// current `Epoch`
pub struct EvalAwi {
    p_state: PState,
    p_note: PNote,
}

impl Drop for EvalAwi {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Some(epoch) = get_current_epoch() {
                let mut lock = epoch.epoch_data.borrow_mut();
                let res = lock.ensemble.remove_note(self.p_note);
                if res.is_err() {
                    panic!(
                        "most likely, an `EvalAwi` created in one `Epoch` was dropped in another"
                    )
                }
            }
            // else the epoch has been dropped
        }
    }
}

impl Lineage for EvalAwi {
    fn state(&self) -> PState {
        self.p_state
    }
}

impl Clone for EvalAwi {
    /// This makes another note to the same state that `self` pointed to.
    fn clone(&self) -> Self {
        let epoch_data = get_current_epoch().unwrap().epoch_data;
        let mut lock = epoch_data.borrow_mut();
        let p_note = lock.ensemble.note_pstate(self.p_state).unwrap();
        Self {
            p_state: self.p_state,
            p_note,
        }
    }
}

impl EvalAwi {
    pub fn nzbw(&self) -> NonZeroUsize {
        epoch::get_nzbw_from_current_epoch(self.p_state)
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn p_note(&self) -> PNote {
        self.p_note
    }

    pub(crate) fn from_state(p_state: PState) -> Option<Self> {
        let epoch_data = get_current_epoch().unwrap().epoch_data;
        let mut lock = epoch_data.borrow_mut();
        let p_note = lock.ensemble.note_pstate(p_state)?;
        lock.ensemble
            .stator
            .states
            .get_mut(p_state)
            .unwrap()
            .allow_pruning = false;
        Some(Self { p_state, p_note })
    }

    /// Can return `None` if the state has been pruned
    pub fn from_bits(bits: &dag::Bits) -> Option<Self> {
        Self::from_state(bits.state())
    }

    pub fn eval(&self) -> Result<awi::Awi, EvalError> {
        let nzbw = self.nzbw();
        let mut res = awi::Awi::zero(nzbw);
        for bit_i in 0..res.bw() {
            let val = Ensemble::calculate_thread_local_note_value(self.p_note, bit_i)?;
            if let Some(val) = val.known_value() {
                res.set(bit_i, val).unwrap();
            } else {
                return Err(EvalError::OtherString(format!(
                    "could not eval bit {bit_i} to known value, the state is {}",
                    get_current_epoch()
                        .unwrap()
                        .epoch_data
                        .borrow()
                        .ensemble
                        .get_state_debug(self.p_state)
                        .unwrap()
                )))
            }
        }
        Ok(res)
    }

    pub fn zero(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::zero(w)).unwrap()
    }

    pub fn umax(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::umax(w)).unwrap()
    }

    pub fn imax(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::imax(w)).unwrap()
    }

    pub fn imin(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::imin(w)).unwrap()
    }

    pub fn uone(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::uone(w)).unwrap()
    }
}

impl fmt::Debug for EvalAwi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(epoch) = get_current_epoch() {
            if let Some(s) = epoch
                .epoch_data
                .borrow()
                .ensemble
                .get_state_debug(self.state())
            {
                return write!(f, "EvalAwi({s})");
            }
        }
        write!(f, "EvalAwi({:?})", self.state())
    }
}

forward_debug_fmt!(EvalAwi);

impl<B: AsRef<dag::Bits>> From<B> for EvalAwi {
    #[track_caller]
    fn from(b: B) -> Self {
        Self::from_bits(b.as_ref()).unwrap()
    }
}
