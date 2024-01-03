use std::{
    borrow::Borrow,
    fmt,
    num::NonZeroUsize,
    ops::{Deref, Index, RangeFull},
    thread::panicking,
};

use awint::{
    awint_dag::{dag, EvalError, Lineage, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{
    awi,
    ensemble::{BasicValue, BasicValueKind, CommonValue, Ensemble, PExternal},
    epoch::get_current_epoch,
};

// do not implement `Clone` for this, we would need a separate `LazyCellAwi`
// type

// Note: `mem::forget` can be used on `LazyAwi`s, but in this crate it should
// only be done in special cases like if a `EpochShared` is being force dropped
// by a panic or something that would necessitate giving up on `Epoch`
// invariants anyway

/// When other mimicking types are created from a reference of this, `retro_`
/// can later be called to retroactively change the input values of the DAG.
///
/// # Custom Drop
///
/// Upon being dropped, this will remove special references being kept by the
/// current `Epoch`
pub struct LazyAwi {
    opaque: dag::Awi,
    p_external: PExternal,
}

// NOTE: when changing this also remember to change `LazyInlAwi`
impl Drop for LazyAwi {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Some(epoch) = get_current_epoch() {
                let res = epoch
                    .epoch_data
                    .borrow_mut()
                    .ensemble
                    .remove_rnode(self.p_external);
                if res.is_err() {
                    panic!("most likely, a `LazyAwi` created in one `Epoch` was dropped in another")
                }
            }
            // else the epoch has been dropped
        }
    }
}

impl Lineage for LazyAwi {
    fn state(&self) -> PState {
        self.opaque.state()
    }
}

macro_rules! retro_primitives {
    ($($f:ident $x:ident);*;) => {
        $(
            /// Retroactively-assigns by `rhs`
            pub fn $f(&self, rhs: $x) -> Result<(), EvalError> {
                self.retro_(&awi::InlAwi::from(rhs))
            }
        )*
    };
}

macro_rules! init {
    ($($f:ident $retro_:ident);*;) => {
        $(
            /// Initializes a `LazyAwi` with the corresponding dynamic value
            pub fn $f(w: NonZeroUsize) -> Self {
                let res = Self::opaque(w);
                res.$retro_().unwrap();
                res
            }
        )*
    };
}

macro_rules! init_inl {
    ($($f:ident $retro_:ident);*;) => {
        $(
            /// Initializes a `LazyInlAwi` with the corresponding dynamic value
            pub fn $f() -> Self {
                let res = Self::opaque();
                res.$retro_().unwrap();
                res
            }
        )*
    };
}

macro_rules! retro {
    ($($f:ident $kind:ident);*;) => {
        $(
            /// Retroactively-assigns by `rhs`. Returns an error if this
            /// is being called after the corresponding Epoch is dropped.
            pub fn $f(&self) -> Result<(), EvalError> {
                Ensemble::change_thread_local_rnode_value(
                    self.p_external,
                    CommonValue::Basic(BasicValue {
                        kind: BasicValueKind::$kind,
                        nzbw: self.nzbw(),
                    }),
                    false,
                )
            }
        )*
    };
}

impl LazyAwi {
    retro_primitives!(
        retro_bool_ bool;
        retro_u8_ u8;
        retro_i8_ i8;
        retro_u16_ u16;
        retro_i16_ i16;
        retro_u32_ u32;
        retro_i32_ i32;
        retro_u64_ u64;
        retro_i64_ i64;
        retro_u128_ u128;
        retro_i128_ i128;
        retro_usize_ usize;
        retro_isize_ isize;
    );

    init!(
        zero retro_zero_;
        umax retro_umax_;
        imax retro_imax_;
        imin retro_imin_;
        uone retro_uone_;
    );

    retro!(
        retro_zero_ Zero;
        retro_umax_ Umax;
        retro_imax_ Imax;
        retro_imin_ Imin;
        retro_uone_ Uone;
    );

    fn internal_as_ref(&self) -> &dag::Bits {
        &self.opaque
    }

    pub fn p_external(&self) -> PExternal {
        self.p_external
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        Ensemble::get_thread_local_rnode_nzbw(self.p_external).unwrap()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    /// Initializes a `LazyAwi` with an unknown dynamic value
    pub fn opaque(w: NonZeroUsize) -> Self {
        let opaque = dag::Awi::opaque(w);
        let p_external = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .make_rnode_for_pstate(opaque.state(), false)
            .unwrap();
        Self { opaque, p_external }
    }

    /// Retroactively-assigns by `rhs`. Returns an error if bitwidths mismatch
    /// or if this is being called after the corresponding Epoch is dropped.
    pub fn retro_(&self, rhs: &awi::Bits) -> Result<(), EvalError> {
        Ensemble::change_thread_local_rnode_value(self.p_external, CommonValue::Bits(rhs), false)
    }

    /// Retroactively-unknown-assigns, the same as `retro_` except it sets the
    /// bits to a dynamically unknown value
    pub fn retro_unknown_(&self) -> Result<(), EvalError> {
        Ensemble::change_thread_local_rnode_value(
            self.p_external,
            CommonValue::Basic(BasicValue {
                kind: BasicValueKind::Opaque,
                nzbw: self.nzbw(),
            }),
            false,
        )
    }

    /// Retroactively-constant-assigns by `rhs`, the same as `retro_` except it
    /// adds the guarantee that the value will never be changed again (or else
    /// it will result in errors if you try another `retro_*` function on
    /// `self`)
    pub fn retro_const_(&self, rhs: &awi::Bits) -> Result<(), EvalError> {
        Ensemble::change_thread_local_rnode_value(self.p_external, CommonValue::Bits(rhs), true)
    }
}

impl Deref for LazyAwi {
    type Target = dag::Bits;

    fn deref(&self) -> &Self::Target {
        self.internal_as_ref()
    }
}

impl Index<RangeFull> for LazyAwi {
    type Output = dag::Bits;

    fn index(&self, _i: RangeFull) -> &dag::Bits {
        self
    }
}

impl Borrow<dag::Bits> for LazyAwi {
    fn borrow(&self) -> &dag::Bits {
        self
    }
}

impl AsRef<dag::Bits> for LazyAwi {
    fn as_ref(&self) -> &dag::Bits {
        self
    }
}

impl fmt::Debug for LazyAwi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LazyAwi({:?})", self.state())
    }
}

forward_debug_fmt!(LazyAwi);

impl From<&LazyAwi> for dag::Awi {
    fn from(value: &LazyAwi) -> Self {
        dag::Awi::from(value.as_ref())
    }
}

impl From<&LazyAwi> for dag::ExtAwi {
    fn from(value: &LazyAwi) -> Self {
        dag::ExtAwi::from(value.as_ref())
    }
}

/// The same as [LazyAwi](crate::LazyAwi), except that it allows for checking
/// bitwidths at compile time.
#[derive(Clone)]
pub struct LazyInlAwi<const BW: usize, const LEN: usize> {
    opaque: dag::InlAwi<BW, LEN>,
    p_external: PExternal,
}

#[macro_export]
macro_rules! lazy_inlawi_ty {
    ($w:expr) => {
        LazyInlAwi::<
            { $w },
            {
                {
                    Bits::unstable_raw_digits({ $w })
                }
            },
        >
    };
}

impl<const BW: usize, const LEN: usize> Drop for LazyInlAwi<BW, LEN> {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Some(epoch) = get_current_epoch() {
                let res = epoch
                    .epoch_data
                    .borrow_mut()
                    .ensemble
                    .remove_rnode(self.p_external);
                if res.is_err() {
                    panic!(
                        "most likely, a `LazyInlAwi` created in one `Epoch` was dropped in another"
                    )
                }
            }
            // else the epoch has been dropped
        }
    }
}

impl<const BW: usize, const LEN: usize> Lineage for LazyInlAwi<BW, LEN> {
    fn state(&self) -> PState {
        self.opaque.state()
    }
}

impl<const BW: usize, const LEN: usize> LazyInlAwi<BW, LEN> {
    init_inl!(
        zero retro_zero_;
        umax retro_umax_;
        imax retro_imax_;
        imin retro_imin_;
        uone retro_uone_;
    );

    retro!(
        retro_zero_ Zero;
        retro_umax_ Umax;
        retro_imax_ Imax;
        retro_imin_ Imin;
        retro_uone_ Uone;
    );

    pub fn p_external(&self) -> PExternal {
        self.p_external
    }

    fn internal_as_ref(&self) -> &dag::InlAwi<BW, LEN> {
        &self.opaque
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        Ensemble::get_thread_local_rnode_nzbw(self.p_external).unwrap()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    #[track_caller]
    pub fn opaque() -> Self {
        let opaque = dag::InlAwi::opaque();
        let p_external = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .make_rnode_for_pstate(opaque.state(), false)
            .unwrap();
        Self { opaque, p_external }
    }

    /// Retroactively-assigns by `rhs`. Returns an error if bitwidths mismatch
    /// or if this is being called after the corresponding Epoch is dropped.
    pub fn retro_(&self, rhs: &awi::Bits) -> Result<(), EvalError> {
        Ensemble::change_thread_local_rnode_value(self.p_external, CommonValue::Bits(rhs), false)
    }

    /// Retroactively-unknown-assigns, the same as `retro_` except it sets the
    /// bits to a dynamically unknown value
    pub fn retro_unknown_(&self) -> Result<(), EvalError> {
        Ensemble::change_thread_local_rnode_value(
            self.p_external,
            CommonValue::Basic(BasicValue {
                kind: BasicValueKind::Opaque,
                nzbw: self.nzbw(),
            }),
            false,
        )
    }

    /// Retroactively-constant-assigns by `rhs`, the same as `retro_` except it
    /// adds the guarantee that the value will never be changed again
    pub fn retro_const_(&self, rhs: &awi::Bits) -> Result<(), EvalError> {
        Ensemble::change_thread_local_rnode_value(self.p_external, CommonValue::Bits(rhs), true)
    }
}

impl<const BW: usize, const LEN: usize> Deref for LazyInlAwi<BW, LEN> {
    type Target = dag::InlAwi<BW, LEN>;

    fn deref(&self) -> &Self::Target {
        self.internal_as_ref()
    }
}

impl<const BW: usize, const LEN: usize> Index<RangeFull> for LazyInlAwi<BW, LEN> {
    type Output = dag::InlAwi<BW, LEN>;

    fn index(&self, _i: RangeFull) -> &dag::InlAwi<BW, LEN> {
        self
    }
}

impl<const BW: usize, const LEN: usize> Borrow<dag::InlAwi<BW, LEN>> for LazyInlAwi<BW, LEN> {
    fn borrow(&self) -> &dag::InlAwi<BW, LEN> {
        self
    }
}

impl<const BW: usize, const LEN: usize> AsRef<dag::InlAwi<BW, LEN>> for LazyInlAwi<BW, LEN> {
    fn as_ref(&self) -> &dag::InlAwi<BW, LEN> {
        self
    }
}

impl<const BW: usize, const LEN: usize> fmt::Debug for LazyInlAwi<BW, LEN> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LazyAwi({:?})", self.state())
    }
}

macro_rules! forward_lazyinlawi_fmt {
    ($($name:ident)*) => {
        $(
            impl<const BW: usize, const LEN: usize> fmt::$name for LazyInlAwi<BW, LEN> {
                fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                    fmt::Debug::fmt(self, f)
                }
            }
        )*
    };
}

forward_lazyinlawi_fmt!(Display LowerHex UpperHex Octal Binary);
