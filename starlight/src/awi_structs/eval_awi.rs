use std::{fmt, num::NonZeroUsize, thread::panicking};

use awint::{
    awint_dag::{dag, triple_arena::Ptr, Lineage, Location, PState},
    awint_internals::{forward_debug_fmt, BITS},
};

use crate::{
    awi,
    ensemble::{Ensemble, PExternal},
    epoch::get_current_epoch,
    utils::DisplayStr,
    Error,
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
    p_external: PExternal,
}

impl Drop for EvalAwi {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            self.drop_internal();
        }
    }
}

macro_rules! from_impl {
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
            pub fn $f(&self) -> Result<$x, Error> {
                let awi = self.eval()?;
                if awi.bw() == $w {
                    Ok(awi.$to_x())
                } else {
                    Err(Error::WrongBitwidth)
                }
            }
        )*
    };
}

impl EvalAwi {
    from_impl!(
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

    /// Sets up `PExternal`s and other things, requires that this be a new
    /// `EvalAwi` or that `drop_internal` has been called on the old value
    #[track_caller]
    fn set_internal(&mut self, p_state: PState) -> Result<(), Error> {
        let tmp = std::panic::Location::caller();
        let location = Location {
            file: tmp.file(),
            line: tmp.line(),
            col: tmp.column(),
        };
        if let Ok(epoch) = get_current_epoch() {
            let mut lock = epoch.epoch_data.borrow_mut();
            match lock
                .ensemble
                .make_rnode_for_pstate(p_state, Some(location), true, true)
            {
                Ok(p_external) => {
                    self.p_external = p_external;
                    Ok(())
                }
                Err(e) => Err(Error::OtherString(format!(
                    "could not create or `future_*` an `EvalAwi` from the given mimicking state: \
                     {e}"
                ))),
            }
        } else {
            Err(Error::OtherStr(
                "attempted to create or `future_*` an `EvalAwi` when no active `starlight::Epoch` \
                 exists",
            ))
        }
    }

    pub fn p_external(&self) -> PExternal {
        self.p_external
    }

    fn drop_internal(&self) {
        if let Ok(epoch) = get_current_epoch() {
            let mut lock = epoch.epoch_data.borrow_mut();
            let _ = lock.ensemble.rnode_dec_rc(self.p_external());
        }
    }

    pub fn try_get_nzbw(&self) -> Result<NonZeroUsize, Error> {
        Ensemble::get_thread_local_rnode_nzbw(self.p_external)
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.try_get_nzbw().unwrap()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub(crate) fn try_clone_from(p_external: PExternal) -> Result<Self, Error> {
        let epoch = get_current_epoch()?;
        let mut lock = epoch.epoch_data.borrow_mut();
        let _ = lock.ensemble.rnode_inc_rc(p_external)?;
        Ok(Self { p_external })
    }

    /// Clones `self`, returning a perfectly equivalent `Eval` that will have
    /// the same `eval` effects. Returns an error if the active `Epoch` is not
    /// correct.
    pub fn try_clone(&self) -> Result<Self, Error> {
        EvalAwi::try_clone_from(self.p_external())
    }

    /// Used internally to create `EvalAwi`s
    ///
    /// # Panics
    ///
    /// If an `Epoch` does not exist or the `PState` was pruned
    #[track_caller]
    pub fn from_state(p_state: PState) -> Self {
        let mut res = Self {
            p_external: PExternal::invalid(),
        };
        if let Err(e) = res.set_internal(p_state) {
            panic!("{e:?}")
        }
        res
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
    pub fn eval(&self) -> Result<awi::Awi, Error> {
        let nzbw = self.try_get_nzbw()?;
        let mut res = awi::Awi::zero(nzbw);
        for bit_i in 0..res.bw() {
            let val = Ensemble::request_thread_local_rnode_value(self.p_external, bit_i)?;
            if let Some(val) = val.known_value() {
                res.set(bit_i, val).unwrap();
            } else {
                return Err(Error::OtherString(format!(
                    "could not eval bit {bit_i} to known value, the node is {}",
                    self.p_external()
                )))
            }
        }
        Ok(res)
    }

    /// Like `EvalAwi::eval`, except it returns if the values are all unknowns
    pub fn eval_is_all_unknown(&self) -> Result<bool, Error> {
        let nzbw = self.try_get_nzbw()?;
        let mut all_unknown = true;
        for bit_i in 0..nzbw.get() {
            let val = Ensemble::request_thread_local_rnode_value(self.p_external, bit_i)?;
            if val.is_known() {
                all_unknown = false;
            }
        }
        Ok(all_unknown)
    }

    /// Sets a debug name for `self` that is used in debug reporting and
    /// rendering
    pub fn set_debug_name<S: AsRef<str>>(&self, debug_name: S) -> Result<(), Error> {
        Ensemble::thread_local_rnode_set_debug_name(self.p_external, Some(debug_name.as_ref()))
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

    // TODO not sure if we want this
    /*
    /// Assigns to `self` the state that will be evaluated in future calls to
    /// `eval_*`, overriding what `self` was initially constructed from or other
    /// calls to `future_*`.
    #[track_caller]
    pub fn future_(&mut self, rhs: &dag::Bits) -> Result<(), Error> {
        let nzbw = self.try_get_nzbw()?;
        if nzbw != rhs.nzbw() {
            return Err(Error::WrongBitwidth)
        }
        self.drop_internal();
        self.set_internal(rhs.state())?;
        Ok(())
    }
    */
}

impl fmt::Debug for EvalAwi {
    /// Can only display some fields if the `Epoch` `self` was created in is
    /// active
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tmp = f.debug_struct("EvalAwi");
        tmp.field("p_external", &self.p_external());
        if let Ok(epoch) = get_current_epoch() {
            if let Ok(lock) = epoch.epoch_data.try_borrow() {
                if let Ok((_, rnode)) = lock.ensemble.notary.get_rnode(self.p_external()) {
                    if let Some(ref name) = rnode.debug_name {
                        tmp.field("debug_name", &DisplayStr(name));
                    }
                    /*if let Some(s) = lock.ensemble.get_state_debug(self.state()) {
                        tmp.field("state", &DisplayStr(&s));
                    }*/
                    //tmp.field("bits", &rnode.bits());
                }
            }
        }
        tmp.finish()
    }
}

forward_debug_fmt!(EvalAwi);

impl<B: AsRef<dag::Bits>> From<B> for EvalAwi {
    #[track_caller]
    fn from(b: B) -> Self {
        Self::from_bits(b.as_ref())
    }
}
