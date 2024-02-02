use std::{
    cmp::max,
    mem,
    num::{NonZeroU64, NonZeroUsize},
};

use awint::{
    awi,
    awint_dag::{
        smallvec,
        triple_arena::{Recast, Recaster, SurjectArena},
        PState,
    },
    Awi, Bits,
};
use smallvec::{smallvec, SmallVec};

use crate::{
    ensemble::{DynamicValue, Ensemble, Equiv, PBack, Referent, Value},
    triple_arena::ptr_struct,
    Error,
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
        let mut res = Ok(());
        self.inputs_mut(|inp| {
            if let Err(e) = inp.recast(recaster) {
                res = Err(e);
            }
        });
        res
    }
}

/// When the `i`th input to a LUT is known to be `bit`, this will reduce the LUT
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

/// When a LUT's output is determined to be independent of the `i`th bit, this
/// will reduce it and return true
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

/// Returns an equivalent LUT given that inputs `i` and `j` have been
/// swapped with each other
fn general_rotate_lut(lut: &mut Awi, i: usize, j: usize) {
    debug_assert!(lut.bw().is_power_of_two());
    debug_assert!(max(i, j) < (lut.bw().trailing_zeros() as usize));
    // rotates the zeroeth input with the `i`th input, `i > 0`
    fn general_basis_rotate(lut: &mut Awi, i: usize) {
        use awi::*;
        // it turns out that the rotation can be broken down into a stationary part, a
        // part that shifts left, and a part that shifts right. This generates the
        // masks.
        let one = inlawi!(01);
        let two = inlawi!(10);
        let mut tmp0 = Awi::zero(NonZeroUsize::new(1 << i).unwrap());
        let mut tmp1 = Awi::zero(NonZeroUsize::new(2 << i).unwrap());
        let mut mask0 = Awi::zero(lut.nzbw());
        tmp0.repeat_(&two);
        tmp1.resize_(&tmp0, false);
        mask0.repeat_(&tmp1);
        let mut mask1 = Awi::zero(lut.nzbw());
        tmp0.repeat_(&one);
        tmp1.field_to(tmp0.bw(), &tmp0, tmp0.bw()).unwrap();
        mask1.repeat_(&tmp1);
        let mut mask2 = Awi::zero(lut.nzbw());
        tmp0.repeat_(&one);
        tmp1.resize_(&tmp0, false);
        tmp0.repeat_(&two);
        tmp1.field_to(tmp0.bw(), &tmp0, tmp0.bw()).unwrap();
        mask2.repeat_(&tmp1);

        // apply the masks, shift, then OR them together to get the result
        let s = (1 << i) - 1;
        mask0.and_(lut).unwrap();
        mask0.shl_(s).unwrap();
        mask1.and_(lut).unwrap();
        mask1.lshr_(s).unwrap();
        lut.and_(&mask2).unwrap();
        lut.or_(&mask0).unwrap();
        lut.or_(&mask1).unwrap();
    }
    match (i == 0, j == 0) {
        (true, true) => (),
        (true, false) => general_basis_rotate(lut, j),
        (false, true) => general_basis_rotate(lut, i),
        (false, false) => {
            general_basis_rotate(lut, i);
            general_basis_rotate(lut, j);
            general_basis_rotate(lut, i);
        }
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
const R0: [u64; 5] = [
    0x2222_2222_2222_2222,
    0x0a0a_0a0a_0a0a_0a0a,
    0x00aa_00aa_00aa_00aa,
    0x0000_aaaa_0000_aaaa,
    0x0000_0000_aaaa_aaaa,
];
const R1: [u64; 5] = [
    0x4444_4444_4444_4444,
    0x5050_5050_5050_5050,
    0x5500_5500_5500_5500,
    0x5555_0000_5555_0000,
    0x5555_5555_0000_0000,
];
const R2: [u64; 5] = [
    0x9999_9999_9999_9999,
    0xa5a5_a5a5_a5a5_a5a5,
    0xaa55_aa55_aa55_aa55,
    0xaaaa_5555_aaaa_5555,
    0xaaaa_aaaa_5555_5555,
];
// Rotates the `i`th column with the 0th column, assumes `i > 0`
fn basis_rotate64(lut: u64, i: usize) -> u64 {
    debug_assert!((i > 0) && (i < 6));
    let s = (1 << i) - 1;
    // it can be broken into a part that shifts left, a part that shifts right, and
    // a stationary part
    ((lut & R0[i - 1]) << s) | ((lut & R1[i - 1]) >> s) | (lut & R2[i - 1])
}
// Rotates the `i`th column with the `j`th column
fn rotate64(lut: u64, i: usize, j: usize) -> u64 {
    match (i == 0, j == 0) {
        (true, true) => lut,
        (true, false) => basis_rotate64(lut, j),
        (false, true) => basis_rotate64(lut, i),
        (false, false) => basis_rotate64(basis_rotate64(basis_rotate64(lut, i), j), i),
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
        debug_assert!(i < (lut.bw().trailing_zeros() as usize));
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
        debug_assert!(i < (lut.len().trailing_zeros() as usize));
        let next_bw = lut.len() / 2;
        let mut next_lut = vec![DynamicValue::ConstUnknown; next_bw];
        let mut removed = Vec::with_capacity(next_bw);
        let w = 1 << i;
        let mut from = 0;
        let mut to = 0;
        while to < next_bw {
            for j in 0..w {
                let mut tmp0 = lut[from + j];
                let mut tmp1 = lut[from + w + j];
                if bit {
                    mem::swap(&mut tmp0, &mut tmp1);
                }
                next_lut[to + j] = tmp0;
                if let DynamicValue::Dynam(p_back) = tmp1 {
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
        debug_assert!(i < (lut.bw().trailing_zeros() as usize));
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

    /// The same as `reduce_independent_lut`, except it checks for independence
    /// regarding dynamic LUT bits with equal constants or source equivalences
    #[must_use]
    pub fn reduce_independent_dynamic_lut(
        backrefs: &SurjectArena<PBack, Referent, Equiv>,
        lut: &[DynamicValue],
        i: usize,
    ) -> Option<(Vec<DynamicValue>, Vec<PBack>)> {
        debug_assert!(lut.len().is_power_of_two());
        let next_bw = lut.len() / 2;
        let w = 1 << i;
        let mut from = 0;
        let mut to = 0;
        while to < next_bw {
            for j in 0..w {
                let tmp0 = &lut[from + j];
                let tmp1 = &lut[from + w + j];
                match tmp0 {
                    DynamicValue::ConstUnknown => return None,
                    DynamicValue::Const(b0) => match tmp1 {
                        DynamicValue::ConstUnknown => return None,
                        DynamicValue::Const(b1) => {
                            if *b0 != *b1 {
                                return None
                            }
                        }
                        DynamicValue::Dynam(_) => return None,
                    },
                    DynamicValue::Dynam(p0) => match tmp1 {
                        DynamicValue::ConstUnknown => return None,
                        DynamicValue::Const(_) => return None,
                        DynamicValue::Dynam(p1) => {
                            if !backrefs.in_same_set(*p0, *p1).unwrap() {
                                return None
                            }
                        }
                    },
                }
            }
            from += 2 * w;
            to += w;
        }
        // we can reduce if the loop did not terminate early
        let mut res = Vec::with_capacity(next_bw);
        let mut removed = Vec::with_capacity(next_bw);
        let mut from = 0;
        let mut to = 0;
        while to < next_bw {
            for j in 0..w {
                let tmp0 = &lut[from + j];
                let tmp1 = &lut[from + w + j];
                res.push(*tmp0);
                if let DynamicValue::Dynam(p) = tmp1 {
                    removed.push(*p);
                }
            }
            from += 2 * w;
            to += w;
        }
        Some((res, removed))
    }

    /// Returns an equivalent LUT given that inputs `i` and `j` have been
    /// swapped with each other
    pub fn rotate_lut(lut: &mut Awi, i: usize, j: usize) {
        debug_assert!(lut.bw().is_power_of_two());
        debug_assert!(max(i, j) < (lut.bw().trailing_zeros() as usize));
        if lut.bw() > 64 {
            general_rotate_lut(lut, i, j);
        } else {
            let rotated = rotate64(lut.to_u64(), i, j);
            lut.u64_(rotated);
        }
    }
}

impl Ensemble {
    /// Given the current values of the input equivalences to the `LNode` at
    /// `p_lnode`, computes a known lookup value if possible, or returns an
    /// unknown value. Also returns the partial order number for the evaluator
    /// to use.
    pub fn calculate_lnode_value(&mut self, p_lnode: PLNode) -> Result<(Value, NonZeroU64), Error> {
        // read current inputs
        let lnode = self.lnodes.get(p_lnode).unwrap();
        Ok(match &lnode.kind {
            LNodeKind::Copy(p_inp) => {
                let inp_equiv = self.backrefs.get_val(*p_inp).unwrap();
                let inp_partial_ord_num = inp_equiv.evaluator_partial_order;
                let inp_val = inp_equiv.val;
                (inp_val, inp_partial_ord_num)
            }
            LNodeKind::Lut(inp, original_lut) => {
                let len = inp.len();
                let mut lut = original_lut.clone();
                let mut max_partial_ord_num = NonZeroU64::new(1).unwrap();
                for i in (0..len).rev() {
                    let p_inp = inp[i];
                    let equiv = self.backrefs.get_val(p_inp).unwrap();
                    max_partial_ord_num = max(max_partial_ord_num, equiv.evaluator_partial_order);
                    if let Some(b) = equiv.val.known_value() {
                        LNode::reduce_lut(&mut lut, i, b);
                    }
                }

                // if the reduced LUT is all ones or all zeros, we can know that any unknown
                // changes will be unable to affect the output
                if lut.is_zero() {
                    (Value::Dynam(false), max_partial_ord_num)
                } else if lut.is_umax() {
                    (Value::Dynam(true), max_partial_ord_num)
                } else {
                    (Value::Unknown, max_partial_ord_num)
                }
            }
            LNodeKind::DynamicLut(inp, original_lut) => {
                let inp_len = NonZeroUsize::new(inp.len()).unwrap();
                let mut inp_val = Awi::zero(inp_len);
                let mut inp_known = Awi::zero(inp_len);
                let mut inp_len = inp_len.get();
                let mut max_partial_ord_num = NonZeroU64::new(1).unwrap();
                for i in 0..inp_len {
                    let p_inp = inp[i];
                    let equiv = self.backrefs.get_val(p_inp).unwrap();
                    max_partial_ord_num = max(max_partial_ord_num, equiv.evaluator_partial_order);
                    if let Some(b) = equiv.val.known_value() {
                        inp_val.set(i, b).unwrap();
                        inp_known.set(i, true).unwrap();
                    }
                }
                let lut_w = NonZeroUsize::new(original_lut.len()).unwrap();
                let mut lut = Awi::zero(lut_w);
                let mut lut_known = Awi::zero(lut_w);
                for (i, value) in original_lut.iter().enumerate() {
                    match value {
                        DynamicValue::ConstUnknown => (),
                        DynamicValue::Const(b) => {
                            lut_known.set(i, true).unwrap();
                            lut.set(i, *b).unwrap()
                        }
                        DynamicValue::Dynam(p) => {
                            let equiv = self.backrefs.get_val(*p).unwrap();
                            if let Some(b) = equiv.val.known_value() {
                                lut_known.set(i, true).unwrap();
                                lut.set(i, b).unwrap();
                            }
                        }
                    }
                }
                // TODO do this more efficiently
                let mut reduced_lut = original_lut.clone();
                // reduce the LUT based on fixed and known bits
                for i in (0..inp_len).rev() {
                    if inp_known.get(i).unwrap() {
                        let bit = inp_val.get(i).unwrap();
                        LNode::reduce_lut(&mut lut, i, bit);
                        LNode::reduce_lut(&mut lut_known, i, bit);
                        reduced_lut = LNode::reduce_dynamic_lut(&reduced_lut, i, bit).0;
                        // remove the input bits virtually
                        inp_len = inp_len.checked_sub(1).unwrap();
                    }
                }
                if inp_len == 0 {
                    // only one LUT bit left, no inputs
                    if lut_known.get(0).unwrap() {
                        return Ok((Value::Dynam(lut.get(0).unwrap()), max_partial_ord_num))
                    } else {
                        return Ok((Value::Unknown, max_partial_ord_num))
                    }
                }
                if lut_known.is_umax() {
                    if lut.is_zero() {
                        return Ok((Value::Dynam(false), max_partial_ord_num))
                    } else if lut.is_umax() {
                        return Ok((Value::Dynam(true), max_partial_ord_num))
                    }
                }
                (Value::Unknown, max_partial_ord_num)
            }
        })
    }

    /// Makes a single output bit lookup table `LNode` and returns a `PBack` to
    /// it. Returns `None` if the table length is incorrect or any of the
    /// `p_inxs` are invalid.
    #[must_use]
    pub fn make_lut(
        &mut self,
        p_inxs: &[Option<PBack>],
        lut: &Bits,
        lowered_from: Option<PState>,
    ) -> Option<PBack> {
        let num_entries = 1 << p_inxs.len();
        if lut.bw() != num_entries {
            return None
        }
        for p_inx in p_inxs {
            if let Some(p_inx) = p_inx {
                if !self.backrefs.contains(*p_inx) {
                    return None
                }
            }
        }
        let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::Unknown),
            )
        });
        let p_lnode = self.lnodes.insert_with(|p_lnode| {
            let p_self = self
                .backrefs
                .insert_key(p_equiv, Referent::ThisLNode(p_lnode))
                .unwrap();
            let mut inp = smallvec![];
            for p_inx in p_inxs {
                let p_back = self
                    .backrefs
                    .insert_key(p_inx.unwrap(), Referent::Input(p_lnode))
                    .unwrap();
                inp.push(p_back);
            }
            LNode::new(p_self, LNodeKind::Lut(inp, Awi::from(lut)), lowered_from)
        });
        // For DFS lowering, we want to calculate the current `Lut` value and set it to
        // prevent issues about change events that would happen if we didn't simply
        // calculate now. This is also where partial ordering is initialized in a way
        // that should preclude initial inefficiency in most cases
        let (init_val, source_partial_ordering) = self.calculate_lnode_value(p_lnode).unwrap();
        let equiv = self.backrefs.get_val_mut(p_equiv).unwrap();
        equiv.val = init_val;
        equiv.evaluator_partial_order = source_partial_ordering.checked_add(1).unwrap();
        Some(p_equiv)
    }

    /// Creates separate unique `Referent::Input`s as necessary
    #[must_use]
    pub fn make_dynamic_lut(
        &mut self,
        p_inxs: &[Option<PBack>],
        p_lut_bits: &[DynamicValue],
        lowered_from: Option<PState>,
    ) -> Option<PBack> {
        let num_entries = 1 << p_inxs.len();
        if p_lut_bits.len() != num_entries {
            return None
        }
        for p_inx in p_inxs {
            if let Some(p_inx) = p_inx {
                if !self.backrefs.contains(*p_inx) {
                    return None
                }
            }
        }
        let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::Unknown),
            )
        });
        let p_lnode = self.lnodes.insert_with(|p_lnode| {
            let p_self = self
                .backrefs
                .insert_key(p_equiv, Referent::ThisLNode(p_lnode))
                .unwrap();
            let mut inp = smallvec![];
            for p_inx in p_inxs {
                let p_back = self
                    .backrefs
                    .insert_key(p_inx.unwrap(), Referent::Input(p_lnode))
                    .unwrap();
                inp.push(p_back);
            }
            let mut lut = vec![];
            for p_lut_bit in p_lut_bits {
                if let DynamicValue::Dynam(p_lut_bit) = p_lut_bit {
                    let p_back = self
                        .backrefs
                        .insert_key(*p_lut_bit, Referent::Input(p_lnode))
                        .unwrap();
                    lut.push(DynamicValue::Dynam(p_back));
                } else {
                    lut.push(*p_lut_bit);
                }
            }
            LNode::new(p_self, LNodeKind::DynamicLut(inp, lut), lowered_from)
        });
        // same as in the static LUT case
        let (init_val, source_partial_ordering) = self.calculate_lnode_value(p_lnode).unwrap();
        let equiv = self.backrefs.get_val_mut(p_equiv).unwrap();
        equiv.val = init_val;
        equiv.evaluator_partial_order = source_partial_ordering.checked_add(1).unwrap();
        Some(p_equiv)
    }
}
