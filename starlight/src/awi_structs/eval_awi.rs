use std::{fmt, num::NonZeroUsize};

use awint::{
    awint_dag::{dag, EvalError, Lineage, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{awi, epoch::get_ensemble_mut};

pub struct EvalAwi {
    state: dag::Awi,
}

impl Lineage for EvalAwi {
    fn state(&self) -> PState {
        self.state.state()
    }
}

impl EvalAwi {
    pub fn nzbw(&self) -> NonZeroUsize {
        self.state.nzbw()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn from_bits(bits: &dag::Bits) -> Self {
        Self {
            state: dag::Awi::from_bits(bits),
        }
    }

    pub fn eval(&mut self) -> Result<awi::Awi, EvalError> {
        let nzbw = self.nzbw();
        // DFS from leaf to roots
        get_ensemble_mut(|ensemble| {
            let p_self = self.state();
            ensemble.initialize_state_bits_if_needed(p_self).unwrap();
            let mut res = awi::Awi::zero(nzbw);
            for i in 0..res.bw() {
                let bit = ensemble.states.get(p_self).unwrap().p_self_bits[i];
                let val = ensemble.request_value(bit)?;
                if let Some(val) = val.known_value() {
                    res.set(i, val).unwrap();
                } else {
                    return Err(EvalError::OtherStr("could not eval bit to known value"))
                }
            }
            Ok(res)
        })
    }

    pub fn _internal_init(&mut self) {
        let p_lhs = self.state();
        get_ensemble_mut(|ensemble| {
            ensemble.initialize_state_bits_if_needed(p_lhs).unwrap();
        })
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

impl From<&dag::Bits> for EvalAwi {
    fn from(bits: &dag::Bits) -> EvalAwi {
        Self::from_bits(&bits)
    }
}

impl From<&dag::Awi> for EvalAwi {
    fn from(bits: &dag::Awi) -> EvalAwi {
        Self::from_bits(&bits)
    }
}

impl From<dag::Awi> for EvalAwi {
    fn from(bits: dag::Awi) -> EvalAwi {
        Self::from_bits(&bits)
    }
}
