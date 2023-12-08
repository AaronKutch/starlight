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
    ensemble::{Ensemble, PNote},
    epoch::get_current_epoch,
};

// do not implement `Clone` for this, we would need a separate `LazyCellAwi`
// type

/// When other mimicking types are created from a reference of this, `retro_`
/// can later be called to retroactively change the input values of the DAG.
///
/// # Custom Drop
///
/// Upon being dropped, this will remove special references being kept by the
/// current `Epoch`
pub struct LazyAwi {
    opaque: dag::Awi,
    p_note: PNote,
}

impl Drop for LazyAwi {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            if let Some(epoch) = get_current_epoch() {
                let res = epoch
                    .epoch_data
                    .borrow_mut()
                    .ensemble
                    .remove_note(self.p_note);
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

impl LazyAwi {
    fn internal_as_ref(&self) -> &dag::Bits {
        &self.opaque
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        Ensemble::get_thread_local_note_nzbw(self.p_note).unwrap()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn opaque(w: NonZeroUsize) -> Self {
        let opaque = dag::Awi::opaque(w);
        let p_note = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .note_pstate(opaque.state())
            .unwrap();
        Self { opaque, p_note }
    }

    // TODO it probably does need to be an extra `Awi` in the `Opaque` variant,
    // or does this make sense at all?
    /*pub fn from_bits(bits: &awi::Bits) -> Self {
        Self { opaque: dag::Awi::opaque(bits.nzbw()), lazy_value: Some(awi::Awi::from_bits(bits)) }
    }*/

    /*pub fn zero(w: NonZeroUsize) -> Self {
        let mut res = Self {
            opaque: dag::Awi::opaque(w),
        };
        //res.retro_(&awi!(zero: ..w.get()).unwrap()).unwrap();
        res
    }*/

    /*pub fn umax(w: NonZeroUsize) -> Self {
        Self::from_bits(&awi::Awi::umax(w))
    }

    pub fn imax(w: NonZeroUsize) -> Self {
        Self::from_bits(&awi::Awi::imax(w))
    }

    pub fn imin(w: NonZeroUsize) -> Self {
        Self::from_bits(&awi::Awi::imin(w))
    }

    pub fn uone(w: NonZeroUsize) -> Self {
        Self::from_bits(&awi::Awi::uone(w))
    }*/

    /// Retroactively-assigns by `rhs`. Returns `None` if bitwidths mismatch or
    /// if this is being called after the corresponding Epoch is dropped and
    /// states have been pruned.
    pub fn retro_(&self, rhs: &awi::Bits) -> Result<(), EvalError> {
        Ensemble::change_thread_local_note_value(self.p_note, rhs)
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

/// The same as [LazyAwi](crate::LazyAwi), except that it allows for checking
/// bitwidths at compile time.
#[derive(Clone, Copy)]
pub struct LazyInlAwi<const BW: usize, const LEN: usize> {
    opaque: dag::InlAwi<BW, LEN>,
    p_note: PNote,
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

impl<const BW: usize, const LEN: usize> Lineage for LazyInlAwi<BW, LEN> {
    fn state(&self) -> PState {
        self.opaque.state()
    }
}

impl<const BW: usize, const LEN: usize> LazyInlAwi<BW, LEN> {
    fn internal_as_ref(&self) -> &dag::InlAwi<BW, LEN> {
        &self.opaque
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.opaque.nzbw()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn p_note(&self) -> PNote {
        self.p_note
    }

    pub fn opaque() -> Self {
        let opaque = dag::InlAwi::opaque();
        let p_note = get_current_epoch()
            .unwrap()
            .epoch_data
            .borrow_mut()
            .ensemble
            .note_pstate(opaque.state())
            .unwrap();
        Self { opaque, p_note }
    }

    /// Retroactively-assigns by `rhs`. Returns `None` if bitwidths mismatch or
    /// if this is being called after the corresponding Epoch is dropped and
    /// states have been pruned.
    pub fn retro_(&self, rhs: &awi::Bits) -> Result<(), EvalError> {
        Ensemble::change_thread_local_note_value(self.p_note, rhs)
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
