use std::num::NonZeroUsize;

use awint::{
    awint_dag::{
        smallvec,
        triple_arena::{Recast, Recaster},
        PState,
    },
    Awi,
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

fn general_reduce_lut(lut: &Awi, i: usize, bit: bool) -> Awi {
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

fn general_reduce_independent_lut(lut: &mut Awi, i: usize) -> bool {
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
        *lut = tmp0;
        true
    } else {
        false
    }
}

const M: [u64; 6] = [
    0x5555_5555_5555_5555,
    0x3333_3333_3333_3333,
    0x0f0f_0f0f_0f0f_0f0f,
    0x00ff_00ff_00ff_00ff,
    0x0000_ffff_0000_ffff,
    0x0000_0000_ffff_ffff,
];
const A: [u64; 5] = [
    0x1111_1111_1111_1111,
    0x0303_0303_0303_0303,
    0x000f_000f_000f_000f,
    0x0000_00ff_0000_00ff,
    0x0000_0000_0000_ffff,
];
// This can quickly reduce LUTs with bitwidths less than 64
fn reduce64(mut lut: u64, i: usize, bit: bool) -> u64 {
    lut >>= (bit as usize) << i;
    lut &= M[i];
    for i in i..5 {
        lut = (lut & A[i]) | ((lut & !A[i]) >> (1 << i));
    }
    lut
}
fn reduce_independent64(mut lut: u64, i: usize) -> Option<u64> {
    let tmp0 = lut & M[i];
    let tmp1 = lut & !M[i];
    if tmp0 == (tmp1 >> (1 << i)) {
        lut = tmp0;
        for i in i..5 {
            lut = (lut & A[i]) | ((lut & !A[i]) >> (1 << i));
        }
        Some(lut)
    } else {
        None
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
    pub fn reduce_lut(lut: &mut Awi, i: usize, bit: bool) {
        debug_assert!(lut.bw().is_power_of_two());
        let half = NonZeroUsize::new(lut.bw() / 2).unwrap();
        if lut.bw() > 64 {
            *lut = general_reduce_lut(lut, i, bit);
        } else {
            let halved = reduce64(lut.to_u64(), i, bit);
            lut.zero_resize(half);
            lut.u64_(halved);
        }
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
    pub fn reduce_independent_lut(lut: &mut Awi, i: usize) -> bool {
        debug_assert!(lut.bw().is_power_of_two());
        let half = NonZeroUsize::new(lut.bw() / 2).unwrap();
        if lut.bw() > 64 {
            general_reduce_independent_lut(lut, i)
        } else if let Some(halved) = reduce_independent64(lut.to_u64(), i) {
            lut.zero_resize(half);
            lut.u64_(halved);
            true
        } else {
            false
        }
    }
}
