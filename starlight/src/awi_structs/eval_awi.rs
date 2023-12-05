use std::{fmt, num::NonZeroUsize};

use awint::{
    awint_dag::{dag, epoch, EvalError, Lineage, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{
    awi,
    ensemble::{Evaluator, PNote, Value},
    epoch::get_current_epoch,
};

/// When created from a type implementing `AsRef<dag::Bits>`, it can later be
/// used to evaluate its dynamic value.
///
/// This will keep the source tree alive in case of pruning.
///
/// # Custom Drop
///
/// TODO
pub struct EvalAwi {
    pub(crate) p_state: PState,
    pub(crate) p_note: PNote,
}

// TODO impl drop to remove note

impl Lineage for EvalAwi {
    fn state(&self) -> PState {
        self.p_state
    }
}

impl Clone for EvalAwi {
    /// This makes another note to the same state that `self` pointed to.
    fn clone(&self) -> Self {
        let p_note = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .note_pstate(self.p_state)
            .unwrap();
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
        let p_note = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .note_pstate(p_state)?;
        Some(Self { p_state, p_note })
    }

    /// Can return `None` if the state has been pruned
    pub fn from_bits(bits: &dag::Bits) -> Option<Self> {
        Self::from_state(bits.state())
    }

    pub fn eval(&self) -> Result<awi::Awi, EvalError> {
        let nzbw = self.nzbw();
        let p_self = self.state();
        let mut res = awi::Awi::zero(nzbw);
        for bit_i in 0..res.bw() {
            let val = Evaluator::calculate_thread_local_state_value(p_self, bit_i)?;
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
                        .get_state_debug(p_self)
                        .unwrap()
                )))
            }
        }
        Ok(res)
    }

    /// Assumes `self` is a single bit
    pub(crate) fn eval_bit(&self) -> Result<Value, EvalError> {
        let p_self = self.state();
        assert_eq!(self.bw(), 1);
        Evaluator::calculate_thread_local_state_value(p_self, 0)
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
