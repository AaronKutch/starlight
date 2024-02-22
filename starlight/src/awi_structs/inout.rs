use std::{
    fmt,
    num::NonZeroUsize,
    ops::{Deref, Index, RangeFull},
};

use awint::{
    awint_dag::{Lineage, PState},
    awint_internals::USIZE_BITS,
};

use super::lazy_awi::format_auto_awi;
use crate::{
    awi::{self, *},
    dag,
    ensemble::PExternal,
    Delay, Error, EvalAwi, LazyAwi,
};

/// A wrapper around [crate::LazyAwi] that has a constant width
pub struct In<const W: usize>(LazyAwi);

impl<const W: usize> std::borrow::Borrow<LazyAwi> for In<W> {
    fn borrow(&self) -> &LazyAwi {
        &self.0
    }
}

#[allow(clippy::from_over_into)]
impl<const W: usize> Into<LazyAwi> for In<W> {
    fn into(self) -> LazyAwi {
        self.0
    }
}

macro_rules! retro_primitives {
    ($($f:ident $x:ident $w:expr);*;) => {
        $(
            impl In<$w> {
                /// Retroactively-assigns by `rhs`
                pub fn $f(&self, rhs: $x) -> Result<(), Error> {
                    self.0.$f(rhs)
                }
            }
        )*
    };
}

macro_rules! init {
    ($($f:ident);*;) => {
        $(
            /// Initializes an `In<N>` with the corresponding dynamic value
            #[track_caller]
            pub fn $f() -> Self {
                Self(LazyAwi::$f(bw(W)))
            }
        )*
    };
}

macro_rules! retro {
    ($($f:ident);*;) => {
        $(
            /// Retroactively-assigns by `rhs`. Returns an error if this
            /// is being called after the corresponding Epoch is dropped.
            pub fn $f(&self) -> Result<(), Error> {
                self.0.$f()
            }
        )*
    };
}

retro_primitives!(
    retro_bool_ bool 1;
    retro_u8_ u8 8;
    retro_i8_ i8 8;
    retro_u16_ u16 16;
    retro_i16_ i16 16;
    retro_u32_ u32 32;
    retro_i32_ i32 32;
    retro_u64_ u64 64;
    retro_i64_ i64 64;
    retro_u128_ u128 128;
    retro_i128_ i128 128;
);

impl In<{ USIZE_BITS }> {
    /// Retroactively-assigns by `rhs`
    pub fn retro_usize_(&self, rhs: usize) -> Result<(), Error> {
        self.0.retro_usize_(rhs)
    }

    /// Retroactively-assigns by `rhs`
    pub fn retro_isize_(&self, rhs: isize) -> Result<(), Error> {
        self.0.retro_isize_(rhs)
    }
}

impl<const W: usize> In<W> {
    init!(
        zero;
        umax;
        imax;
        imin;
        uone;
    );

    retro!(
        retro_zero_;
        retro_umax_;
        retro_imax_;
        retro_imin_;
        retro_uone_;
    );

    pub fn p_external(&self) -> PExternal {
        self.0.p_external()
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.0.nzbw()
    }

    pub fn bw(&self) -> usize {
        self.0.bw()
    }

    /// Initializes an `In<W>` with an unknown dynamic value
    #[track_caller]
    pub fn opaque() -> Self {
        Self(LazyAwi::opaque(bw(W)))
    }

    /// Retroactively-assigns by `rhs`. Returns an error if bitwidths mismatch
    /// or if this is being called after the corresponding Epoch is dropped.
    pub fn retro_(&self, rhs: &awi::Bits) -> Result<(), Error> {
        self.0.retro_(rhs)
    }

    /// Retroactively-unknown-assigns, the same as `retro_` except it sets the
    /// bits to a dynamically unknown value
    pub fn retro_unknown_(&self) -> Result<(), Error> {
        self.0.retro_unknown_()
    }

    /// Retroactively-constant-assigns by `rhs`, the same as `retro_` except it
    /// adds the guarantee that the value will never be changed again (or else
    /// it will result in errors if you try another `retro_*` function on
    /// `self`)
    pub fn retro_const_(&self, rhs: &awi::Bits) -> Result<(), Error> {
        self.0.retro_const_(rhs)
    }

    /// Retroactively-constant-unknown-assigns by `rhs`, the same as
    /// `retro_unknown_` except it adds the guarantee that the value will
    /// never be changed again (or else it will result in errors if you try
    /// another `retro_*` function on `self`)
    pub fn retro_const_unknown_(&self) -> Result<(), Error> {
        self.0.retro_const_unknown_()
    }

    /// Temporally drives `self` with the value of an `EvalAwi`. Note that
    /// errors are raised if `Loop` and `Net` are undriven, you may want to
    /// use them instead unless this is at an interface. Returns `None` if
    /// bitwidths mismatch.
    pub fn drive<E: std::borrow::Borrow<EvalAwi>>(self, rhs: E) -> Result<(), Error> {
        self.0.drive(rhs)
    }

    /// Temporally drives `self` with the value of an `EvalAwi`, with a delay.
    /// Note that errors are raised if
    /// `Loop` and `Net` are undriven, you may want to
    /// use them instead unless this is at an interface. Returns `None` if
    /// bitwidths mismatch.
    pub fn drive_with_delay<E: std::borrow::Borrow<EvalAwi>, D: Into<Delay>>(
        self,
        rhs: E,
        delay: D,
    ) -> Result<(), Error> {
        self.0.drive_with_delay(rhs, delay)
    }

    /// Sets a debug name for `self` that is used in debug reporting and
    /// rendering
    pub fn set_debug_name<S: AsRef<str>>(&self, debug_name: S) -> Result<(), Error> {
        self.0.set_debug_name(debug_name)
    }
}

impl<const W: usize> Deref for In<W> {
    type Target = dag::Bits;

    #[track_caller]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<const W: usize> Index<RangeFull> for In<W> {
    type Output = dag::Bits;

    #[track_caller]
    fn index(&self, _i: RangeFull) -> &dag::Bits {
        self
    }
}

impl<const W: usize> std::borrow::Borrow<dag::Bits> for In<W> {
    #[track_caller]
    fn borrow(&self) -> &dag::Bits {
        self
    }
}

impl<const W: usize> AsRef<dag::Bits> for In<W> {
    #[track_caller]
    fn as_ref(&self) -> &dag::Bits {
        self
    }
}

impl<const W: usize> fmt::Debug for In<W> {
    /// Can only display some fields if the `Epoch` `self` was created in is
    /// active
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        format_auto_awi(&format!("In<{W}>"), self.p_external(), self.nzbw(), f)
    }
}

/// A wrapper around [crate::EvalAwi] that has a constant width.
///
/// Note that when constructing from `dag::Bits`, you need to use
/// `Out::from_bits` because there are blanket impl issues with implementing
/// `TryFrom` for `Out`.
pub struct Out<const W: usize>(EvalAwi);

impl<const W: usize> std::borrow::Borrow<EvalAwi> for Out<W> {
    fn borrow(&self) -> &EvalAwi {
        &self.0
    }
}

#[allow(clippy::from_over_into)]
impl<const W: usize> Into<EvalAwi> for Out<W> {
    fn into(self) -> EvalAwi {
        self.0
    }
}

macro_rules! from_impl {
    ($($fn:ident $t:ident $w:expr);*;) => {
        $(
            impl Out<$w> {
                #[track_caller]
                pub fn $fn(x: dag::$t) -> Self {
                    Self(EvalAwi::$fn(x))
                }
            }
        )*
    }
}

macro_rules! eval_primitives {
    ($($f:ident $x:ident $w:expr);*;) => {
        $(
            impl Out<{$w}> {
                pub fn $f(&self) -> Result<$x, Error> {
                    self.0.$f()
                }
            }
        )*
    };
}

from_impl!(
    from_bool bool 1;
    from_u8 u8 8;
    from_i8 i8 8;
    from_u16 u16 16;
    from_i16 i16 16;
    from_u32 u32 32;
    from_i32 i32 32;
    from_u64 u64 64;
    from_i64 i64 64;
    from_u128 u128 128;
    from_i128 i128 128;
);

eval_primitives!(
    eval_bool bool 1;
    eval_u8 u8 8;
    eval_i8 i8 8;
    eval_u16 u16 16;
    eval_i16 i16 16;
    eval_u32 u32 32;
    eval_i32 i32 32;
    eval_u64 u64 64;
    eval_i64 i64 64;
    eval_u128 u128 128;
    eval_i128 i128 128;
);

impl Out<{ USIZE_BITS }> {
    #[track_caller]
    pub fn from_usize(x: dag::usize) -> Self {
        Self(EvalAwi::from_usize(x))
    }

    #[track_caller]
    pub fn from_isize(x: dag::isize) -> Self {
        Self(EvalAwi::from_isize(x))
    }

    pub fn eval_usize(&self) -> Result<usize, Error> {
        self.0.eval_usize()
    }

    pub fn eval_isize(&self) -> Result<isize, Error> {
        self.0.eval_isize()
    }
}

impl<const W: usize> Out<W> {
    pub fn p_external(&self) -> PExternal {
        self.0.p_external()
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.0.nzbw()
    }

    pub fn bw(&self) -> usize {
        self.0.bw()
    }

    /// Used internally to create `Out<W>`s. Returns an error if the bitwidth of
    /// the `State` does not match `W`.
    ///
    /// # Panics
    ///
    /// If an `Epoch` does not exist or the `PState` was pruned
    #[track_caller]
    pub fn from_state(p_state: PState) -> Result<Self, Error> {
        let eval = EvalAwi::from_state(p_state);
        if eval.bw() != W {
            Err(Error::ConstBitwidthMismatch(eval.bw(), W))
        } else {
            Ok(Self(eval))
        }
    }

    /// Can panic if the state has been pruned
    #[track_caller]
    pub fn from_bits(bits: &dag::Bits) -> Result<Self, Error> {
        Self::from_state(bits.state())
    }

    /// Evaluates the value that `self` would evaluate to given the current
    /// state of any `LazyAwi`s. Depending on the conditions of internal LUTs,
    /// it may be possible to evaluate to a known value even if some inputs are
    /// `opaque`, but in general this will return an error that a bit could not
    /// be evaluated to a known value, if any upstream inputs are `opaque`.
    pub fn eval(&self) -> Result<awi::Awi, Error> {
        self.0.eval()
    }

    /// Like `EvalAwi::eval`, except it returns if the values are all unknowns
    pub fn eval_is_all_unknown(&self) -> Result<bool, Error> {
        self.0.eval_is_all_unknown()
    }

    /// Sets a debug name for `self` that is used in debug reporting and
    /// rendering
    pub fn set_debug_name<S: AsRef<str>>(&self, debug_name: S) -> Result<(), Error> {
        self.0.set_debug_name(debug_name)
    }
}

impl<const W: usize> fmt::Debug for Out<W> {
    /// Can only display some fields if the `Epoch` `self` was created in is
    /// active
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        format_auto_awi(&format!("Out<{W}>"), self.p_external(), self.nzbw(), f)
    }
}

// running into the stupid blanket impl
/*
impl<const W: usize, B: AsRef<dag::Bits>> TryFrom<B> for Out<W> {
    type Error = Error;

    #[track_caller]
    fn try_from(b: B) -> Result<Self, Self::Error> {
        Self::from_bits(b.as_ref())
    }
}
*/
