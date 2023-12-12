use std::num::NonZeroUsize;

use awint::{
    awint_dag::{smallvec, PState},
    Awi, Bits,
};
use smallvec::SmallVec;

use crate::{ensemble::PBack, triple_arena::ptr_struct};

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode);

/// A lookup table node
#[derive(Debug, Clone)]
pub struct TNode {
    pub p_self: PBack,
    /// Inputs
    pub inp: SmallVec<[PBack; 4]>,
    /// Lookup Table that outputs one bit
    pub lut: Option<Awi>,
    pub lowered_from: Option<PState>,
}

impl TNode {
    pub fn new(p_self: PBack, lowered_from: Option<PState>) -> Self {
        Self {
            p_self,
            inp: SmallVec::new(),
            lut: None,
            lowered_from,
        }
    }

    /// Reduce a LUT in half by saving entries indexed by setting the `i`th
    /// input bit to `bit`
    pub fn reduce_lut(lut: &Bits, i: usize, bit: bool) -> Awi {
        assert!(lut.bw().is_power_of_two());
        let next_bw = lut.bw() / 2;
        let mut next_lut = Awi::zero(NonZeroUsize::new(next_bw).unwrap());
        let w = 1 << i;
        let mut from = 0;
        let mut to = 0;
        while to < next_bw {
            next_lut
                .field(to, lut, if bit { from + w } else { from }, w)
                .unwrap();
            from += 2 * w;
            to += w;
        }
        next_lut
    }

    /// Returns an equivalent reduced LUT (with the `i`th index removed) if the
    /// LUT output is independent with respect to the `i`th bit
    #[must_use]
    pub fn reduce_independent_lut(lut: &Bits, i: usize) -> Option<Awi> {
        let nzbw = lut.nzbw();
        assert!(nzbw.get().is_power_of_two());
        let next_bw = nzbw.get() / 2;
        let next_nzbw = NonZeroUsize::new(next_bw).unwrap();
        let mut tmp0 = Awi::zero(next_nzbw);
        let mut tmp1 = Awi::zero(next_nzbw);
        let w = 1 << i;
        // LUT if the `i`th bit were 0
        let mut from = 0;
        let mut to = 0;
        while to < next_bw {
            tmp0.field(to, lut, from, w).unwrap();
            from += 2 * w;
            to += w;
        }
        // LUT if the `i`th bit were 1
        from = w;
        to = 0;
        while to < next_bw {
            tmp1.field(to, lut, from, w).unwrap();
            from += 2 * w;
            to += w;
        }
        if tmp0 == tmp1 {
            Some(tmp0)
        } else {
            None
        }
    }
}
