use std::{fmt, num::NonZeroUsize};

use awint::{
    awint_dag::{dag, epoch, EvalError, Lineage, PNote, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{
    awi,
    ensemble::{Evaluator, Value},
    epoch::get_current_epoch,
};

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

    pub(crate) fn from_state(p_state: PState) -> Self {
        let p_note = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .note_pstate(p_state)
            .unwrap();
        Self { p_state, p_note }
    }

    pub fn from_bits(bits: &dag::Bits) -> Self {
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
                return Err(EvalError::OtherStr("could not eval bit to known value"))
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
        Self::from_bits(&dag::Awi::zero(w))
    }

    pub fn umax(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::umax(w))
    }

    pub fn imax(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::imax(w))
    }

    pub fn imin(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::imin(w))
    }

    pub fn uone(w: NonZeroUsize) -> Self {
        Self::from_bits(&dag::Awi::uone(w))
    }
}

impl fmt::Debug for EvalAwi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EvalAwi({:?})", self.state())
    }
}

forward_debug_fmt!(EvalAwi);

impl<B: AsRef<dag::Bits>> From<B> for EvalAwi {
    fn from(b: B) -> Self {
        Self::from_bits(b.as_ref())
    }
}
