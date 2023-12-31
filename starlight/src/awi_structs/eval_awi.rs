use std::{fmt, num::NonZeroUsize, thread::panicking};

use awint::{
    awint_dag::{dag, EvalError, Lineage, PState},
    awint_internals::{forward_debug_fmt, BITS},
};

use crate::{
    awi,
    ensemble::{Ensemble, PExternal},
    epoch::get_current_epoch,
};

// Note: `mem::forget` can be used on `EvalAwi`s, but in this crate it should
// only be done in special cases like if a `EpochShared` is being force dropped
// by a panic or something that would necessitate giving up on `Epoch`
// invariants anyway

/// When created from a type implementing `AsRef<dag::Bits>`, it can later be
/// used to evaluate its dynamic value.
///
/// This will keep the source tree alive in case of pruning.
///
/// # Custom Drop
///
/// Upon being dropped, this will remove special references being kept by the
/// current `Epoch`.
pub struct EvalAwi {
    p_state: PState,
    p_external: PExternal,
}

impl Drop for EvalAwi {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Some(epoch) = get_current_epoch() {
                let mut lock = epoch.epoch_data.borrow_mut();
                let res = lock.ensemble.remove_rnode(self.p_external);
                if res.is_err() {
                    panic!(
                        "most likely, an `EvalAwi` created in one `Epoch` was dropped in another"
                    )
                }
                if let Some(state) = lock.ensemble.stator.states.get_mut(self.p_state) {
                    state.dec_extern_rc();
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
    /// This makes another rnode to the same state that `self` pointed to.
    #[track_caller]
    fn clone(&self) -> Self {
        Self::from_state(self.p_state)
    }
}

macro_rules! evalawi_from_impl {
    ($($fn:ident $t:ident);*;) => {
        $(
            #[track_caller]
            pub fn $fn(x: dag::$t) -> Self {
                Self::from_state(x.state())
            }
        )*
    }
}

macro_rules! eval_primitives {
    ($($f:ident $x:ident $to_x:ident $w:expr);*;) => {
        $(
            /// The same as [EvalAwi::eval], except that it returns a primitive
            /// and returns an error if the bitwidth of the evaluation does not
            /// match the bitwidth of the primitive
            pub fn $f(&self) -> Result<$x, EvalError> {
                let awi = self.eval()?;
                if awi.bw() == $w {
                    Ok(awi.$to_x())
                } else {
                    Err(EvalError::WrongBitwidth)
                }
            }
        )*
    };
}

impl EvalAwi {
    evalawi_from_impl!(
        from_bool bool;
        from_u8 u8;
        from_i8 i8;
        from_u16 u16;
        from_i16 i16;
        from_u32 u32;
        from_i32 i32;
        from_u64 u64;
        from_i64 i64;
        from_u128 u128;
        from_i128 i128;
        from_usize usize;
        from_isize isize;
    );

    eval_primitives!(
        eval_bool bool to_bool 1;
        eval_u8 u8 to_u8 8;
        eval_i8 i8 to_i8 8;
        eval_u16 u16 to_u16 16;
        eval_i16 i16 to_i16 16;
        eval_u32 u32 to_u32 32;
        eval_i32 i32 to_i32 32;
        eval_u64 u64 to_u64 64;
        eval_i64 i64 to_i64 64;
        eval_u128 u128 to_u128 128;
        eval_i128 i128 to_i128 128;
        eval_usize usize to_usize BITS;
        eval_isize isize to_isize BITS;
    );

    pub fn p_external(&self) -> PExternal {
        self.p_external
    }

    fn try_get_nzbw(&self) -> Result<NonZeroUsize, EvalError> {
        Ensemble::get_thread_local_rnode_nzbw(self.p_external)
    }

    #[track_caller]
    pub fn nzbw(&self) -> NonZeroUsize {
        self.try_get_nzbw().unwrap()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    /// Used internally to create `EvalAwi`s
    ///
    /// # Panics
    ///
    /// If an `Epoch` does not exist or the `PState` was pruned
    #[track_caller]
    pub fn from_state(p_state: PState) -> Self {
        if let Some(epoch) = get_current_epoch() {
            let mut lock = epoch.epoch_data.borrow_mut();
            match lock.ensemble.make_rnode_for_pstate(p_state, true) {
                Some(p_external) => {
                    lock.ensemble
                        .stator
                        .states
                        .get_mut(p_state)
                        .unwrap()
                        .inc_extern_rc();
                    Self {
                        p_state,
                        p_external,
                    }
                }
                None => {
                    panic!(
                        "could not create an `EvalAwi` from the given mimicking state, probably \
                         because the state was pruned or came from a different `Epoch`"
                    )
                }
            }
        } else {
            panic!("attempted to create an `EvalAwi` when no live `Epoch` exists")
        }
    }

    /// Can panic if the state has been pruned
    #[track_caller]
    pub fn from_bits(bits: &dag::Bits) -> Self {
        Self::from_state(bits.state())
    }

    /// Evaluates the value that `self` would evaluate to given the current
    /// state of any `LazyAwi`s. Depending on the conditions of internal LUTs,
    /// it may be possible to evaluate to a known value even if some inputs are
    /// `opaque`, but in general this will return an error that a bit could not
    /// be evaluated to a known value, if any upstream inputs are `opaque`.
    pub fn eval(&self) -> Result<awi::Awi, EvalError> {
        let nzbw = self.try_get_nzbw()?;
        let mut res = awi::Awi::zero(nzbw);
        for bit_i in 0..res.bw() {
            let val = Ensemble::calculate_thread_local_rnode_value(self.p_external, bit_i)?;
            if let Some(val) = val.known_value() {
                res.set(bit_i, val).unwrap();
            } else {
                return Err(EvalError::OtherString(format!(
                    "could not eval bit {bit_i} to known value, the node is {}",
                    self.p_external()
                )))
            }
        }
        Ok(res)
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
        Self::from_bits(b.as_ref())
    }
}
