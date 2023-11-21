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

use crate::{awi, ensemble::Value, epoch::get_tdag_mut};

pub struct LazyAwi {
    state: dag::Awi,
}

// TODO how to handle?
/*impl Clone for LazyAwi {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}*/

impl Lineage for LazyAwi {
    fn state(&self) -> PState {
        self.state.state()
    }
}

impl LazyAwi {
    fn internal_as_ref(&self) -> &dag::Bits {
        &self.state
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.state.nzbw()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    /// Note that this and the corresponding `From<Bits>` impl insert an opaque
    /// intermediate.
    pub fn from_bits(bits: &dag::Bits) -> Self {
        let mut res = Self::zero(bits.nzbw());
        res.state.opaque_with_(&[bits], None);
        res
    }

    pub fn zero(w: NonZeroUsize) -> Self {
        Self {
            state: dag::Awi::zero(w),
        }
    }

    /*
    /// Retroactively-assigns by `rhs`. Returns `None` if bitwidths mismatch or
    /// if this is being called after the corresponding Epoch is dropped and
    /// states have been pruned.
    pub fn retro_(&mut self, rhs: &dag::Bits) -> Option<()> {
        let p_lhs = self.state();
        let p_rhs = rhs.state();
        get_tdag_mut(|tdag| {
            if let Some(lhs) = tdag.states.get(p_lhs) {
                if let Some(rhs) = tdag.states.get(p_rhs) {
                    if lhs.nzbw != rhs.nzbw {
                        return None
                    }
                }
            }
            // initialize if needed
            tdag.initialize_state_bits_if_needed(p_lhs).unwrap();
            tdag.initialize_state_bits_if_needed(p_rhs).unwrap();
            let visit_gen = tdag.visit_gen();
            let mut bits: SmallVec<[Value; 4]> = smallvec![];
            if let Some(rhs) = tdag.states.get(p_rhs) {
                for bit in &rhs.p_self_bits {
                    bits.push(tdag.backrefs.get_val(*bit).unwrap().val);
                }
            }
            if let Some(lhs) = tdag.states.get_mut(p_lhs) {
                for (i, value) in bits.iter().enumerate() {
                    let p_bit = lhs.p_self_bits[i];
                    let bit = tdag.backrefs.get_val_mut(p_bit).unwrap();
                    bit.val = value.const_to_dynam(visit_gen);
                }
            }
            Some(())
        })
        */

    /// Retroactively-assigns by `rhs`. Returns `None` if bitwidths mismatch or
    /// if this is being called after the corresponding Epoch is dropped and
    /// states have been pruned.
    pub fn retro_(&mut self, rhs: &awi::Bits) -> Option<()> {
        let p_lhs = self.state();
        get_tdag_mut(|tdag| {
            if let Some(lhs) = tdag.states.get(p_lhs) {
                if lhs.nzbw != rhs.nzbw() {
                    return None
                }
            }
            // initialize if needed
            tdag.initialize_state_bits_if_needed(p_lhs).unwrap();
            if let Some(lhs) = tdag.states.get_mut(p_lhs) {
                for i in 0..rhs.bw() {
                    let p_bit = lhs.p_self_bits[i];
                    let bit = tdag.backrefs.get_val_mut(p_bit).unwrap();
                    bit.val = Value::Dynam(rhs.get(i).unwrap());
                }
            }
            Some(())
        })
    }

    pub fn eval(&mut self) -> Result<awi::Awi, EvalError> {
        let nzbw = self.nzbw();
        // DFS from leaf to roots
        get_tdag_mut(|tdag| {
            let p_self = self.state();
            tdag.initialize_state_bits_if_needed(p_self).unwrap();
            let mut res = awi::Awi::zero(nzbw);
            for i in 0..res.bw() {
                let bit = tdag.states.get(p_self).unwrap().p_self_bits[i];
                let val = tdag.request_value(bit)?;
                if let Some(val) = val.known_value() {
                    res.set(i, val).unwrap();
                } else {
                    return Err(EvalError::OtherStr("could not eval bit to known value"))
                }
            }
            Ok(res)
        })
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
        write!(f, "Awi({:?})", self.state())
    }
}

forward_debug_fmt!(LazyAwi);

impl From<&dag::Bits> for LazyAwi {
    fn from(bits: &dag::Bits) -> LazyAwi {
        Self::from_bits(bits)
    }
}

impl From<&awi::Bits> for LazyAwi {
    fn from(bits: &awi::Bits) -> LazyAwi {
        let tmp = dag::Awi::from(bits);
        Self::from_bits(&tmp)
    }
}

impl From<&awi::Awi> for LazyAwi {
    fn from(bits: &awi::Awi) -> LazyAwi {
        let tmp = dag::Awi::from(bits);
        Self::from_bits(&tmp)
    }
}

impl From<awi::Awi> for LazyAwi {
    fn from(bits: awi::Awi) -> LazyAwi {
        let tmp = dag::Awi::from(bits);
        Self::from_bits(&tmp)
    }
}
