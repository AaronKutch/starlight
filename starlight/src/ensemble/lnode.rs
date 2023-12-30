use std::num::NonZeroUsize;

use awint::{
    awint_dag::{
        smallvec,
        triple_arena::{Recast, Recaster},
        PState,
    },
    Awi, Bits,
};
use smallvec::SmallVec;

use crate::{
    ensemble::{DynamicValue, PBack},
    triple_arena::ptr_struct,
};

// We use this because our algorithms depend on generation counters
ptr_struct!(PLNode);

#[derive(Debug, Clone)]
pub enum LNodeKind {
    /// Copy a single input bit
    Copy(PBack),
    /// Static Lookup Table that outputs one bit, the `Awi` is the table and the
    /// `SmallVec` is the inputs
    Lut(SmallVec<[PBack; 4]>, Awi),
    /// A Dynamic Lookup Table with the inputs and then the `Vec` is the table
    DynamicLut(SmallVec<[PBack; 4]>, Vec<DynamicValue>),
}

/// A lookup table node
#[derive(Debug, Clone)]
pub struct LNode {
    pub p_self: PBack,
    pub kind: LNodeKind,
    pub lowered_from: Option<PState>,
}

impl Recast<PBack> for LNode {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_self.recast(recaster)?;
        self.inputs_mut(|inp| {
            inp.recast(recaster).unwrap();
        });
        Ok(())
    }
}

impl LNode {
    pub fn new(p_self: PBack, kind: LNodeKind, lowered_from: Option<PState>) -> Self {
        Self {
            p_self,
            kind,
            lowered_from,
        }
    }

    /// Calls `f` for each `PBack` in the inputs of this `LNode`
    pub fn inputs<F: FnMut(PBack)>(&self, mut f: F) {
        match &self.kind {
            LNodeKind::Copy(inp) => f(*inp),
            LNodeKind::Lut(inp, _) => {
                for inp in inp.iter() {
                    f(*inp);
                }
            }
            LNodeKind::DynamicLut(inp, lut) => {
                for inp in inp.iter() {
                    f(*inp);
                }
                for inp in lut.iter() {
                    if let DynamicValue::Dynam(inp) = inp {
                        f(*inp);
                    }
                }
            }
        }
    }

    /// Calls `f` for each `&mut PBack` in the inputs of this `LNode`
    pub fn inputs_mut<F: FnMut(&mut PBack)>(&mut self, mut f: F) {
        match &mut self.kind {
            LNodeKind::Copy(inp) => f(inp),
            LNodeKind::Lut(inp, _) => {
                for inp in inp.iter_mut() {
                    f(inp);
                }
            }
            LNodeKind::DynamicLut(inp, lut) => {
                for inp in inp.iter_mut() {
                    f(inp);
                }
                for inp in lut.iter_mut() {
                    if let DynamicValue::Dynam(inp) = inp {
                        f(inp);
                    }
                }
            }
        }
    }

    /// Reduce a LUT in half by saving entries indexed by setting the `i`th
    /// input bit to `bit`
    pub fn reduce_lut(lut: &Bits, i: usize, bit: bool) -> Awi {
        debug_assert!(lut.bw().is_power_of_two());
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

    /// The same as `reduce_lut`, except for a dynamic table, and it returns
    /// removed `PBack`s
    pub fn reduce_dynamic_lut(
        lut: &[DynamicValue],
        i: usize,
        bit: bool,
    ) -> (Vec<DynamicValue>, Vec<PBack>) {
        debug_assert!(lut.len().is_power_of_two());
        let next_bw = lut.len() / 2;
        let mut next_lut = vec![DynamicValue::Unknown; next_bw];
        let mut removed = Vec::with_capacity(next_bw);
        let w = 1 << i;
        let mut from = 0;
        let mut to = 0;
        while to < next_bw {
            for j in 0..w {
                next_lut[to + j] = lut[if bit { from + j } else { from }];
                if let DynamicValue::Dynam(p_back) = lut[if !bit { from + j } else { from }] {
                    removed.push(p_back);
                }
            }
            from += 2 * w;
            to += w;
        }
        (next_lut, removed)
    }

    /// Returns an equivalent reduced LUT (with the `i`th index removed) if the
    /// LUT output is independent with respect to the `i`th bit
    #[must_use]
    pub fn reduce_independent_lut(lut: &Bits, i: usize) -> Option<Awi> {
        let nzbw = lut.nzbw();
        debug_assert!(nzbw.get().is_power_of_two());
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
