use std::{
    fmt,
    num::NonZeroUsize,
    ops::{Deref, Index, RangeFull},
    thread::panicking,
};

use awint::{
    awint_dag::{dag, smallvec::smallvec, Lineage, Location, Op, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{
    awi,
    ensemble::{BasicValue, BasicValueKind, CommonValue, Ensemble, PExternal},
    epoch::get_current_epoch,
    misc::DisplayStr,
    Delay, Error, EvalAwi,
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

impl Drop for LazyAwi {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            self.drop_internal();
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
            pub fn $f(&self, rhs: $x) -> Result<(), Error> {
                self.retro_(&awi::InlAwi::from(rhs))
            }
        )*
    };
}

macro_rules! init {
    ($($f:ident $retro_:ident);*;) => {
        $(
            /// Initializes a `LazyAwi` with the corresponding dynamic value
            #[track_caller]
            pub fn $f(w: NonZeroUsize) -> Self {
                let res = Self::opaque(w);
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
            pub fn $f(&self) -> Result<(), Error> {
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

    fn drop_internal(&self) {
        if let Ok(epoch) = get_current_epoch() {
            let mut lock = epoch.epoch_data.borrow_mut();
            let _ = lock.ensemble.remove_rnode(self.p_external);
        }
        // else the epoch has been dropped
    }

    fn internal_as_ref(&self) -> &dag::Bits {
        &self.opaque
    }

    pub fn p_external(&self) -> PExternal {
        self.p_external
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

    /// Initializes a `LazyAwi` with an unknown dynamic value
    #[track_caller]
    pub fn opaque(w: NonZeroUsize) -> Self {
        let tmp = std::panic::Location::caller();
        let location = Location {
            file: tmp.file(),
            line: tmp.line(),
            col: tmp.column(),
        };
        let opaque = dag::Awi::new(w, Op::Opaque(smallvec![], Some("LazyOpaque")));
        let p_external = get_current_epoch()
            .expect("attempted to create a `LazyAwi` when no active `starlight::Epoch` exists")
            .epoch_data
            .borrow_mut()
            .ensemble
            .make_rnode_for_pstate(opaque.state(), Some(location), false, false)
            .unwrap();
        Self { opaque, p_external }
    }

    /// Retroactively-assigns by `rhs`. Returns an error if bitwidths mismatch
    /// or if this is being called after the corresponding Epoch is dropped.
    pub fn retro_(&self, rhs: &awi::Bits) -> Result<(), Error> {
        Ensemble::change_thread_local_rnode_value(self.p_external, CommonValue::Bits(rhs), false)
    }

    /// Retroactively-unknown-assigns, the same as `retro_` except it sets the
    /// bits to a dynamically unknown value
    pub fn retro_unknown_(&self) -> Result<(), Error> {
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
    pub fn retro_const_(&self, rhs: &awi::Bits) -> Result<(), Error> {
        Ensemble::change_thread_local_rnode_value(self.p_external, CommonValue::Bits(rhs), true)
    }

    /// Retroactively-constant-unknown-assigns by `rhs`, the same as
    /// `retro_unknown_` except it adds the guarantee that the value will
    /// never be changed again (or else it will result in errors if you try
    /// another `retro_*` function on `self`)
    pub fn retro_const_unknown_(&self) -> Result<(), Error> {
        Ensemble::change_thread_local_rnode_value(
            self.p_external,
            CommonValue::Basic(BasicValue {
                kind: BasicValueKind::Opaque,
                nzbw: self.nzbw(),
            }),
            true,
        )
    }

    /// Temporally drives `self` with the value of an `EvalAwi`. Note that
    /// `Loop` and `Net` implicitly warn if they are undriven, you may want to
    /// use them instead. Returns `None` if bitwidths mismatch.
    pub fn drive(self, rhs: &EvalAwi) -> Result<(), Error> {
        self.drive_with_delay(rhs, Delay::zero())
    }

    /// Temporally drives `self` with the value of an `EvalAwi`, with a delay.
    /// Note that `Loop` and `Net` implicitly warn if they are undriven, you
    /// may want to use them instead. Returns `None` if bitwidths mismatch.
    pub fn drive_with_delay<D: Into<Delay>>(self, rhs: &EvalAwi, delay: D) -> Result<(), Error> {
        if self.try_get_nzbw()? != rhs.try_get_nzbw()? {
            return Err(Error::WrongBitwidth)
        }
        let delay = delay.into();
        for i in 0..self.bw() {
            Ensemble::tnode_drive_thread_local_rnode(
                self.p_external,
                i,
                rhs.p_external(),
                i,
                delay,
            )?
        }
        Ok(())
    }

    /// Sets a debug name for `self` that is used in debug reporting and
    /// rendering
    pub fn set_debug_name<S: AsRef<str>>(&self, debug_name: S) -> Result<(), Error> {
        Ensemble::thread_local_rnode_set_debug_name(self.p_external, Some(debug_name.as_ref()))
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

impl std::borrow::Borrow<dag::Bits> for LazyAwi {
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
    /// Can only display some fields if the `Epoch` `self` was created in is
    /// active
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut tmp = f.debug_struct("LazyAwi");
        tmp.field("p_external", &self.p_external());
        if let Ok(epoch) = get_current_epoch() {
            let lock = epoch.epoch_data.borrow();
            if let Ok((_, rnode)) = lock.ensemble.notary.get_rnode(self.p_external()) {
                if let Some(ref name) = rnode.debug_name {
                    tmp.field("debug_name", &DisplayStr(name));
                }
                // only really useful for the `EvalAwi` case
                /*if let Some(s) = lock
                    .ensemble
                    .get_state_debug(self.state())
                {
                    tmp.field("state", &DisplayStr(&s));
                }*/
                tmp.field("bits", &rnode.bits());
            }
        }
        tmp.finish()
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
