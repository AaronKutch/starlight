use std::{
    borrow::Borrow,
    fmt,
    num::NonZeroUsize,
    ops::{Deref, Index, RangeFull},
};

use awint::{
    awint_dag::{dag, EvalError, Lineage, PState},
    awint_internals::forward_debug_fmt,
};

use crate::{awi, ensemble::Evaluator};

// do not implement `Clone` for this, we would need a separate `LazyCellAwi`
// type

pub struct LazyAwi {
    // this must remain the same opaque and noted in order for `retro_` to work
    opaque: dag::Awi,
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
        self.opaque.nzbw()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn opaque(w: NonZeroUsize) -> Self {
        Self {
            opaque: dag::Awi::opaque(w),
        }
    }

    // TODO it probably does need to be an extra `Awi` in the `Opaque` variant
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
    pub fn retro_(&mut self, rhs: &awi::Bits) -> Result<(), EvalError> {
        let p_lhs = self.state();
        Evaluator::change_thread_local_state_value(p_lhs, rhs)
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

/*impl From<&awi::Bits> for LazyAwi {
    fn from(bits: &awi::Bits) -> LazyAwi {
        Self::from_bits(&bits)
    }
}

impl From<&awi::Awi> for LazyAwi {
    fn from(bits: &awi::Awi) -> LazyAwi {
        Self::from_bits(&bits)
    }
}

impl From<awi::Awi> for LazyAwi {
    fn from(bits: awi::Awi) -> LazyAwi {
        Self::from_bits(&bits)
    }
}*/
