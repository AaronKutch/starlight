//! Using combined ordinary and mimick types to assist in lowering

use std::{cmp::min, mem, num::NonZeroUsize};

use awint::{
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        ConcatFieldsType, PState,
    },
    bw,
};
use dag::{awi, inlawi, inlawi_ty, Awi, Bits, InlAwi};

use crate::{
    awi,
    awint_dag::{ConcatType, Lineage, Op},
    dag,
    ensemble::LNode,
};
const USIZE_BITS: usize = usize::BITS as usize;

// This code here is especially messy because we do not want to get into
// infinite lowering loops. These first few functions need to use manual
// concatenation and only literal macros within loop blocks.

// Everything used to be done through `get` and `set`, but going straight to
// `StaticLut` or `Concat` or `ConcatFields` is a massive performance boost.

// TODO In the future if we want something more, we should have some kind of
// caching for known optimization results.

// even though we have later stages that would optimize LUTs, we find it good to
// optimize as early as possible for this common case.
pub fn create_static_lut(
    mut inxs: SmallVec<[PState; 4]>,
    mut lut: awi::Awi,
) -> Result<Op<PState>, PState> {
    // acquire LUT inputs, for every constant input reduce the LUT
    let len = usize::from(u8::try_from(inxs.len()).unwrap());
    for i in (0..len).rev() {
        let p_state = inxs[i];
        if let Some(bit) = p_state.try_get_as_awi() {
            debug_assert_eq!(bit.bw(), 1);
            inxs.remove(i);
            crate::ensemble::LNode::reduce_lut(&mut lut, i, bit.to_bool());
        }
    }

    // now check for input independence, e.x. for 0101 the 2^1 bit changes nothing
    let len = inxs.len();
    for i in (0..len).rev() {
        if (lut.bw() > 1) && LNode::reduce_independent_lut(&mut lut, i) {
            // independent of the `i`th bit
            inxs.remove(i);
        }
    }

    // input independence automatically reduces all zeros and all ones LUTs, so just
    // need to check if the LUT is one bit for constant generation
    if lut.bw() == 1 {
        if lut.is_zero() {
            Ok(Op::Literal(awi::Awi::zero(bw(1))))
        } else {
            Ok(Op::Literal(awi::Awi::umax(bw(1))))
        }
    } else if (lut.bw() == 2) && lut.get(1).unwrap() {
        Err(inxs[0])
    } else {
        Ok(Op::StaticLut(
            ConcatType::from_iter(inxs.iter().cloned()),
            lut,
        ))
    }
}

// note that the $inx arguments are in order from least to most significant, and
// this assumes the LUT has a single output bit
macro_rules! static_lut {
    ($lhs:ident; $lut:expr; $($inx:expr),*) => {{
        //let nzbw = $lhs.state_nzbw();
        match create_static_lut(
            smallvec![$(
                $inx.state(),
            )*],
            {use awi::*; awi!($lut)}
        ) {
            Ok(op) => {
                $lhs.update_state(
                    bw(1),
                    op,
                ).unwrap_at_runtime();
            }
            Err(copy) => {
                $lhs.set_state(copy);
            }
        }
    }};
}

fn concat(nzbw: NonZeroUsize, vec: SmallVec<[PState; 4]>) -> Awi {
    if vec.len() == 1 {
        Awi::from_state(vec[0])
    } else {
        Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(vec)))
    }
}

fn concat_update(bits: &mut Bits, nzbw: NonZeroUsize, vec: SmallVec<[PState; 4]>) {
    if vec.len() == 1 {
        bits.set_state(vec[0]);
    } else {
        bits.update_state(nzbw, Op::Concat(ConcatType::from_smallvec(vec)))
            .unwrap_at_runtime();
    }
}

pub fn reverse(x: &Bits) -> Awi {
    let nzbw = x.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    for i in 0..x.bw() {
        out.push(x.get(x.bw() - 1 - i).unwrap().state())
    }
    concat(nzbw, out)
}

pub fn selector(inx: &Bits, cap: Option<usize>) -> Vec<inlawi_ty!(1)> {
    let num = cap.unwrap_or_else(|| 1usize << inx.bw());
    if num == 0 {
        // not sure if this should be reachable
        panic!();
    }
    if num == 1 {
        return vec![inlawi!(1)]
    }
    let lb_num = num.next_power_of_two().trailing_zeros() as usize;
    let mut signals = Vec::with_capacity(num);
    for i in 0..num {
        let mut signal = inlawi!(1);
        for j in 0..lb_num {
            // depending on the `j`th bit of `i`, keep the signal line true
            if (i & (1 << j)) == 0 {
                static_lut!(signal; 0100; inx.get(j).unwrap(), signal);
            } else {
                static_lut!(signal; 1000; inx.get(j).unwrap(), signal);
            }
        }
        signals.push(signal);
    }
    signals
}

pub fn selector_awi(inx: &Bits, cap: Option<usize>) -> Awi {
    let num = cap.unwrap_or_else(|| 1usize << inx.bw());
    if num == 0 {
        // not sure if this should be reachable
        panic!();
    }
    if num == 1 {
        return awi!(1)
    }
    let lb_num = num.next_power_of_two().trailing_zeros() as usize;
    let nzbw = NonZeroUsize::new(num).unwrap();
    let mut signals = SmallVec::with_capacity(num);
    for i in 0..num {
        let mut signal = inlawi!(1);
        for j in 0..lb_num {
            // depending on the `j`th bit of `i`, keep the signal line true
            if (i & (1 << j)) == 0 {
                static_lut!(signal; 0100; inx.get(j).unwrap(), signal);
            } else {
                static_lut!(signal; 1000; inx.get(j).unwrap(), signal);
            }
        }
        signals.push(signal.state());
    }
    concat(nzbw, signals)
}

pub fn static_mux(x0: &Bits, x1: &Bits, inx: &Bits) -> Awi {
    debug_assert_eq!(x0.bw(), x1.bw());
    debug_assert_eq!(inx.bw(), 1);
    let nzbw = x0.nzbw();
    let mut signals = SmallVec::with_capacity(nzbw.get());
    for i in 0..x0.bw() {
        let mut tmp = inlawi!(0);
        static_lut!(tmp; 1100_1010; x0.get(i).unwrap(), x1.get(i).unwrap(), inx);
        signals.push(tmp.state());
    }
    concat(nzbw, signals)
}

// uses dynamic LUTs to wholesale multiplex one or more inputs
pub fn general_mux(inputs: &[Awi], inx: &Bits) -> Awi {
    debug_assert!(!inputs.is_empty());
    let nzbw = inputs[0].nzbw();
    let lut_w = NonZeroUsize::new(inputs.len().next_power_of_two()).unwrap();
    debug_assert_eq!(1 << inx.bw(), lut_w.get());
    let mut out_signals = SmallVec::with_capacity(nzbw.get());
    let unknown = Awi::opaque(bw(1));
    for out_i in 0..nzbw.get() {
        let mut lut = Vec::with_capacity(lut_w.get());
        for input in inputs {
            lut.push((input.state(), out_i, bw(1)));
        }
        // fill up the rest of the way as necessary
        for _ in lut.len()..lut_w.get() {
            lut.push((unknown.state(), 0, bw(1)));
        }
        let lut = Awi::new(
            lut_w,
            Op::ConcatFields(ConcatFieldsType::from_iter(lut.iter().cloned())),
        );
        out_signals.push(Awi::new(bw(1), Op::Lut([lut.state(), inx.state()])).state());
    }
    concat(nzbw, out_signals)
}

// uses dynamic LUTs under the hood
pub fn dynamic_to_static_get(bits: &Bits, inx: &Bits) -> inlawi_ty!(1) {
    if bits.bw() == 1 {
        return InlAwi::from(bits.to_bool())
    }
    /*let signals = selector(inx, Some(bits.bw()));
    let mut out = inlawi!(0);
    for (i, signal) in signals.iter().enumerate() {
        static_lut!(out; 1111_1000; signal, bits.get(i).unwrap(), out);
    }
    out*/
    let lut_w = NonZeroUsize::new(bits.bw().next_power_of_two()).unwrap();
    let inx_w = NonZeroUsize::new(lut_w.get().trailing_zeros() as usize).unwrap();
    let mut true_inx = Awi::zero(inx_w);
    true_inx.field_width(inx, inx_w.get()).unwrap();
    let base = if bits.bw() == lut_w.get() {
        Awi::from(bits)
    } else {
        let unknowns =
            Awi::opaque(NonZeroUsize::new(lut_w.get().checked_sub(bits.bw()).unwrap()).unwrap());
        concat(lut_w, smallvec![bits.state(), unknowns.state()])
    };
    InlAwi::new(Op::Lut([base.state(), true_inx.state()]))
}

/// Trailing smear, given the value of `inx` it will set all bits in the vector
/// up to but not including the one indexed by `inx`. This means that
/// `inx.to_usize() == 0` sets no bits, and `inx.to_usize() == num_bits` sets
/// all the bits. Beware of off-by-one errors, if there are `n` bits then there
/// are `n + 1` possible unique smears.
pub fn tsmear_inx(inx: &Bits, num_signals: usize) -> Vec<inlawi_ty!(1)> {
    let next_pow = num_signals.next_power_of_two();
    let mut lb_num = next_pow.trailing_zeros() as usize;
    if next_pow == num_signals {
        // need extra bit to get all `n + 1`
        lb_num += 1;
    }
    let mut signals = Vec::with_capacity(num_signals);
    for i in 0..num_signals {
        // if `inx < i`
        let mut signal = inlawi!(0);
        // if the prefix up until now is equal
        let mut prefix_equal = inlawi!(1);
        for j in (0..lb_num).rev() {
            // starting with the msb going down
            if (i & (1 << j)) == 0 {
                // update equality, and if the prefix is true and the `j` bit of `inx` is set
                // then the signal is set

                let inx_j = inx.get(j).unwrap();
                static_lut!(signal; 11111000; inx_j, prefix_equal, signal);

                static_lut!(prefix_equal; 0100; inx_j, prefix_equal);
            } else {
                // just update equality, the `j`th bit of `i` is 1 and cannot be less than
                // whatever the `inx` bit is
                static_lut!(prefix_equal; 1000; inx.get(j).unwrap(), prefix_equal);
            }
        }
        signals.push(signal);
    }
    signals
}

pub fn tsmear_awi(inx: &Bits, num_signals: usize) -> Awi {
    let next_pow = num_signals.next_power_of_two();
    let mut lb_num = next_pow.trailing_zeros() as usize;
    if next_pow == num_signals {
        // need extra bit to get all `n + 1`
        lb_num += 1;
    }
    let nzbw = NonZeroUsize::new(num_signals).unwrap();
    let mut signals = SmallVec::with_capacity(num_signals);
    for i in 0..num_signals {
        // if `inx < i`
        let mut signal = inlawi!(0);
        // if the prefix up until now is equal
        let mut prefix_equal = inlawi!(1);
        for j in (0..lb_num).rev() {
            // starting with the msb going down
            if (i & (1 << j)) == 0 {
                // update equality, and if the prefix is true and the `j` bit of `inx` is set
                // then the signal is set

                let inx_j = inx.get(j).unwrap();
                static_lut!(signal; 11111000; inx_j, prefix_equal, signal);

                static_lut!(prefix_equal; 0100; inx_j, prefix_equal);
            } else {
                // just update equality, the `j`th bit of `i` is 1 and cannot be less than
                // whatever the `inx` bit is
                static_lut!(prefix_equal; 1000; inx.get(j).unwrap(), prefix_equal);
            }
        }
        signals.push(signal.state());
    }
    concat(nzbw, signals)
}

/*
Normalize. Table size explodes really fast if trying
to keep as a single LUT, let's use a meta LUT.

e.x.
i_1 i_0
  0   0 x_0_0 x_1_0
  0   1 x_0_1 x_1_1
  1   0 x_0_2 x_1_2
  1   1 x_0_3 x_1_3
        y_0   y_1
=>
// a signal line for each row
s_0 = (!i_1) && (!i_0)
s_1 = (!i_1) && i_0
y_0 = (s_0 && x_0_0) || (s_1 && x_0_1) || ...
y_1 = (s_0 && x_1_0) || (s_1 && x_1_1) || ...
...
*/
pub fn dynamic_to_static_lut(out: &mut Bits, table: &Bits, inx: &Bits) {
    // if this is broken it breaks a lot of stuff
    debug_assert!(table.bw() == (out.bw().checked_mul(1 << inx.bw()).unwrap()));
    let signals = selector(inx, None);
    let nzbw = out.nzbw();
    let mut tmp_output = SmallVec::with_capacity(nzbw.get());
    for j in 0..out.bw() {
        let mut column = inlawi!(0);
        for (i, signal) in signals.iter().enumerate() {
            static_lut!(column; 1111_1000; signal, table.get((i * out.bw()) + j).unwrap(), column);
        }
        tmp_output.push(column.state());
    }
    concat_update(out, nzbw, tmp_output)
}

pub fn dynamic_to_static_set(bits: &Bits, inx: &Bits, bit: &Bits) -> Awi {
    if bits.bw() == 1 {
        return Awi::from(bit)
    }
    let signals = selector(inx, Some(bits.bw()));
    let nzbw = bits.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    for (i, signal) in signals.iter().enumerate() {
        // multiplex between using `bits` or the `bit` depending on the signal
        let mut tmp = inlawi!(0);
        static_lut!(tmp; 1101_1000; signal, bit, bits.get(i).unwrap());
        out.push(tmp.state());
    }
    concat(nzbw, out)
}

pub fn resize(x: &Bits, w: NonZeroUsize, signed: bool) -> Awi {
    if w == x.nzbw() {
        Awi::from_bits(x)
    } else if w < x.nzbw() {
        Awi::new(
            w,
            Op::ConcatFields(ConcatFieldsType::from_iter([(x.state(), 0usize, w)])),
        )
    } else if signed {
        let extension = Awi::new(
            NonZeroUsize::new(w.get() - x.bw()).unwrap(),
            Op::Repeat([x.msb().state()]),
        );
        concat(w, smallvec![x.state(), extension.state()])
    } else {
        let zero = Awi::zero(NonZeroUsize::new(w.get() - x.bw()).unwrap());
        concat(w, smallvec![x.state(), zero.state()])
    }
}

pub fn resize_cond(x: &Bits, w: NonZeroUsize, signed: &Bits) -> Awi {
    debug_assert_eq!(signed.bw(), 1);
    if w == x.nzbw() {
        Awi::from_bits(x)
    } else if w < x.nzbw() {
        Awi::new(
            w,
            Op::ConcatFields(ConcatFieldsType::from_iter([(x.state(), 0usize, w)])),
        )
    } else {
        let extension = Awi::new(
            NonZeroUsize::new(w.get() - x.bw()).unwrap(),
            Op::Repeat([signed.state()]),
        );
        concat(w, smallvec![x.state(), extension.state()])
    }
}

/// This does not handle invalid arguments; set `width` to zero to cause no-ops
pub fn field_width(lhs: &Bits, rhs: &Bits, width: &Bits) -> Awi {
    let min_w = min(lhs.bw(), rhs.bw());
    let signals = tsmear_inx(width, min_w);
    let nzbw = NonZeroUsize::new(signals.len()).unwrap();
    let mut mux_part = SmallVec::with_capacity(nzbw.get());
    for (i, signal) in signals.into_iter().enumerate() {
        // mux_ between `lhs` or `rhs` based on the signal
        let mut tmp = inlawi!(0);
        static_lut!(tmp; 1100_1010; lhs.get(i).unwrap(), rhs.get(i).unwrap(), signal);
        mux_part.push(tmp.state());
    }
    let mux_part = concat(nzbw, mux_part);
    if let Some(lhs_rem_hi) = NonZeroUsize::new(lhs.bw() - nzbw.get()) {
        Awi::new(
            lhs.nzbw(),
            Op::ConcatFields(ConcatFieldsType::from_iter([
                (mux_part.state(), 0usize, nzbw),
                (lhs.state(), nzbw.get(), lhs_rem_hi),
            ])),
        )
    } else {
        mux_part
    }
}

// old static strategy if we need it
/*
/// Given the diagonal control lines and input of a crossbar with output width
/// s.t. `input.bw() + out.bw() - 1 = signals.bw()`, returns the output. The
/// `i`th input bit and `j`th output bit are controlled by the
/// `out.bw() - 1 + i - j`th control line.
/// `signal_range` uses a virtual `..` range of the possible signals.
pub fn crossbar(
    output: &mut Bits,
    input: &Bits,
    signals: &[inlawi_ty!(1)],
    signal_range: (usize, usize),
) {
    debug_assert!(signal_range.0 < signal_range.1);
    debug_assert_eq!(signal_range.1 - signal_range.0, signals.len());

    let nzbw = output.nzbw();
    let mut tmp_output = SmallVec::with_capacity(nzbw.get());
    for j in 0..output.bw() {
        // output bar for ORing
        let mut out_bar = inlawi!(0);
        for i in 0..input.bw() {
            let signal_inx = output.bw() - 1 + i - j;
            if (signal_inx >= signal_range.0) && (signal_inx < signal_range.1) {
                static_lut!(out_bar; 1111_1000;
                    input.get(i).unwrap(),
                    signals[signal_inx - signal_range.0],
                    out_bar
                );
            }
        }
        tmp_output.push(out_bar.state());
    }
    concat_update(output, nzbw, tmp_output)
}
*/

/*pub fn funnel(x: &Bits, s: &Bits) -> Awi {
    debug_assert_eq!(x.bw() & 1, 0);
    debug_assert_eq!(x.bw() / 2, 1 << s.bw());
    let mut out = Awi::zero(NonZeroUsize::new(x.bw() / 2).unwrap());
    let signals = selector(s, None);
    // select zero should connect the zeroeth crossbars, so the offset is `out.bw()
    // - 1 + 0 - 0`
    let range = (out.bw() - 1, out.bw() - 1 + out.bw());
    crossbar(&mut out, x, &signals, range);
    out
}*/

pub fn funnel(x: &Bits, s: &Bits) -> Awi {
    debug_assert!((s.bw() < (USIZE_BITS - 1)) && ((2usize << s.bw()) == x.bw()));
    let out_w = NonZeroUsize::new(1 << s.bw()).unwrap();
    let mut output = SmallVec::with_capacity(out_w.get());
    for j in 0..out_w.get() {
        let lut = Awi::new(
            out_w,
            Op::ConcatFields(ConcatFieldsType::from_iter([(x.state(), j, out_w)])),
        );
        output.push(Awi::new(bw(1), Op::Lut([lut.state(), s.state()])).state());
    }
    concat(out_w, output)
}

/// Assumes that `start` and `end` are their small versions. Setting `end` to 0
/// guarantees a no-op.
pub fn range_or(x: &Bits, start: &Bits, end: &Bits) -> Awi {
    // trailing mask that trails `start`, exclusive
    let tmask0 = tsmear_inx(start, x.bw());
    // trailing mask that trails `end`, exclusive
    let tmask1 = tsmear_inx(end, x.bw());

    // or with `x` based on the masks, note that any case where `tmask1` is zero
    // needs to result in no-op
    let mut out = SmallVec::with_capacity(x.bw());
    for i in 0..x.bw() {
        let mut signal = inlawi!(0);
        static_lut!(signal; 1111_0100; tmask0[i], tmask1[i], x.get(i).unwrap());
        out.push(signal.state());
    }
    concat(x.nzbw(), out)
}

/// Assumes that `start` and `end` are their small versions. Must be set to a
/// full range for a no-op
pub fn range_and(x: &Bits, start: &Bits, end: &Bits) -> Awi {
    // trailing mask that trails `start`, exclusive
    let tmask0 = tsmear_inx(start, x.bw());
    // trailing mask that trails `end`, exclusive
    let tmask1 = tsmear_inx(end, x.bw());

    // and with `x` based on the masks, the fourth case can be any bit we choose
    let mut out = SmallVec::with_capacity(x.bw());
    for i in 0..x.bw() {
        let mut signal = inlawi!(0);
        static_lut!(signal; 0100_0000; tmask0[i], tmask1[i], x.get(i).unwrap());
        out.push(signal.state());
    }
    concat(x.nzbw(), out)
}

/// Assumes that `start` and `end` are their small versions. Setting `end` to 0
/// guarantees a no-op.
pub fn range_xor(x: &Bits, start: &Bits, end: &Bits) -> Awi {
    // trailing mask that trails `start`, exclusive
    let tmask0 = tsmear_inx(start, x.bw());
    // trailing mask that trails `end`, exclusive
    let tmask1 = tsmear_inx(end, x.bw());

    // xor with `x` based on the masks, note that any case where `tmask1` is zero
    // needs to result in no-op
    let mut out = SmallVec::with_capacity(x.bw());
    for i in 0..x.bw() {
        let mut signal = inlawi!(0);
        static_lut!(signal; 1011_0100; tmask0[i], tmask1[i], x.get(i).unwrap());
        out.push(signal.state());
    }
    concat(x.nzbw(), out)
}

/// Assumes that `from` and `width` is in range, however setting `width` to 0
/// guarantees that nothing happens to `lhs` even with `from` being out of range
pub fn field_from(lhs: &Bits, rhs: &Bits, from: &Bits, width: &Bits) -> Awi {
    let mut out = Awi::from_bits(lhs);
    // the max shift value that can be anything but an effective no-op
    if let Some(s_w) = Bits::nontrivial_bits(rhs.bw() - 1) {
        let mut s = Awi::zero(s_w);
        s.resize_(from, false);
        let mut x = Awi::opaque(NonZeroUsize::new(2 << s_w.get()).unwrap());
        // this is done on purpose so there are opaque bits
        let w = rhs.bw();
        let _ = x.field_width(rhs, w);
        let tmp = funnel(&x, &s);

        let max_width = min(lhs.bw(), rhs.bw());
        let mut small_width = Awi::zero(Bits::nontrivial_bits(max_width).unwrap());
        small_width.resize_(width, false);
        let _ = out.field_width(&tmp, small_width.to_usize());
    } else {
        let small_width = Awi::from_bool(width.lsb());
        let _ = out.field_width(rhs, small_width.to_usize());
    }
    out
}

/// Assumes that `s` is in range
pub fn shl(x: &Bits, s: &Bits) -> Awi {
    let mut out = Awi::zero(x.nzbw());
    if let Some(small_s_w) = Bits::nontrivial_bits(x.bw() - 1) {
        let mut small_s = Awi::zero(small_s_w);
        small_s.resize_(s, false);
        let mut wide_x = Awi::opaque(NonZeroUsize::new(2 << small_s_w.get()).unwrap());
        // need zeros for the bits that are shifted in
        let _ = wide_x.field_to(x.bw(), &Awi::zero(x.nzbw()), x.bw() - 1);
        let mut rev_x = Awi::zero(x.nzbw());
        rev_x.copy_(x).unwrap();
        // we have two reversals so that the shift acts leftward
        rev_x.rev_();
        let _ = wide_x.field_width(&rev_x, x.bw());
        let tmp = funnel(&wide_x, &small_s);
        out.resize_(&tmp, false);
        out.rev_();
    } else {
        let small_width = Awi::from_bool(s.lsb());
        out.resize_(x, false);
        let _ = out.field_width(x, small_width.to_usize());
    }
    out
}

/// Assumes that `s` is in range
pub fn lshr(x: &Bits, s: &Bits) -> Awi {
    let mut out = Awi::zero(x.nzbw());
    if let Some(small_s_w) = Bits::nontrivial_bits(x.bw() - 1) {
        let mut small_s = Awi::zero(small_s_w);
        small_s.resize_(s, false);
        let mut wide_x = Awi::opaque(NonZeroUsize::new(2 << small_s_w.get()).unwrap());
        // need zeros for the bits that are shifted in
        let _ = wide_x.field_to(x.bw(), &Awi::zero(x.nzbw()), x.bw() - 1);
        let _ = wide_x.field_width(x, x.bw());
        let tmp = funnel(&wide_x, &small_s);
        out.resize_(&tmp, false);
    } else {
        let small_width = Awi::from_bool(s.lsb());
        out.resize_(x, false);
        let _ = out.field_width(x, small_width.to_usize());
    }
    out
}

/// Assumes that `s` is in range
pub fn ashr(x: &Bits, s: &Bits) -> Awi {
    let mut out = Awi::zero(x.nzbw());
    if let Some(small_s_w) = Bits::nontrivial_bits(x.bw() - 1) {
        let mut small_s = Awi::zero(small_s_w);
        small_s.resize_(s, false);
        let mut wide_x = Awi::opaque(NonZeroUsize::new(2 << small_s_w.get()).unwrap());
        // extension for the bits that are shifted in
        let _ = wide_x.field_to(
            x.bw(),
            &Awi::new(x.nzbw(), Op::Repeat([x.msb().state()])),
            x.bw() - 1,
        );
        let _ = wide_x.field_width(x, x.bw());
        let tmp = funnel(&wide_x, &small_s);
        out.resize_(&tmp, false);
    } else {
        let small_width = Awi::from_bool(s.lsb());
        out.resize_(x, false);
        let _ = out.field_width(x, small_width.to_usize());
    }
    out
}

pub fn rotl(x: &Bits, s: &Bits) -> Awi {
    let mut out = Awi::zero(x.nzbw());
    if let Some(small_s_w) = Bits::nontrivial_bits(x.bw() - 1) {
        let mut small_s = Awi::zero(small_s_w);
        small_s.resize_(s, false);

        let mut rev_x = Awi::zero(x.nzbw());
        rev_x.copy_(x).unwrap();
        rev_x.rev_();

        let mut wide_x = Awi::opaque(NonZeroUsize::new(2 << small_s_w.get()).unwrap());
        // extension for the bits that are shifted in
        let _ = wide_x.field_to(x.bw(), &rev_x, x.bw() - 1);
        let _ = wide_x.field_width(&rev_x, x.bw());
        let tmp = funnel(&wide_x, &small_s);
        out.resize_(&tmp, false);
        out.rev_();
    } else {
        let small_width = Awi::from_bool(s.lsb());
        out.resize_(x, false);
        let _ = out.field_width(x, small_width.to_usize());
    }
    out
}

pub fn rotr(x: &Bits, s: &Bits) -> Awi {
    let mut out = Awi::zero(x.nzbw());
    if let Some(small_s_w) = Bits::nontrivial_bits(x.bw() - 1) {
        let mut small_s = Awi::zero(small_s_w);
        small_s.resize_(s, false);
        let mut wide_x = Awi::opaque(NonZeroUsize::new(2 << small_s_w.get()).unwrap());
        // extension for the bits that are shifted in
        let _ = wide_x.field_to(x.bw(), x, x.bw() - 1);
        let _ = wide_x.field_width(x, x.bw());
        let tmp = funnel(&wide_x, &small_s);
        out.resize_(&tmp, false);
    } else {
        let small_width = Awi::from_bool(s.lsb());
        out.resize_(x, false);
        let _ = out.field_width(x, small_width.to_usize());
    }
    out
}

pub fn bitwise_not(x: &Bits) -> Awi {
    let nzbw = x.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    for i in 0..x.bw() {
        let mut tmp = inlawi!(0);
        static_lut!(tmp; 01; x.get(i).unwrap());
        out.push(tmp.state());
    }
    concat(nzbw, out)
}

pub fn bitwise(lhs: &Bits, rhs: &Bits, lut: awi::Awi) -> Awi {
    debug_assert_eq!(lhs.bw(), rhs.bw());
    debug_assert_eq!(lut.bw(), 4);
    let nzbw = lhs.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    for i in 0..lhs.bw() {
        let mut tmp = inlawi!(0);
        tmp.update_state(
            bw(1),
            Op::StaticLut(
                ConcatType::from_iter([lhs.get(i).unwrap().state(), rhs.get(i).unwrap().state()]),
                lut.clone(),
            ),
        )
        .unwrap_at_runtime();
        out.push(tmp.state());
    }
    concat(nzbw, out)
}

pub fn incrementer(x: &Bits, cin: &Bits, dec: bool) -> (Awi, inlawi_ty!(1)) {
    debug_assert_eq!(cin.bw(), 1);
    let nzbw = x.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    let mut carry = InlAwi::from(cin.to_bool());
    if dec {
        for i in 0..x.bw() {
            let mut tmp = inlawi!(0);
            let b = x.get(i).unwrap();
            // half subtractor
            static_lut!(tmp; 1001; carry, b);
            out.push(tmp.state());
            static_lut!(carry; 1110; carry, b);
        }
    } else {
        for i in 0..x.bw() {
            let mut tmp = inlawi!(0);
            let b = x.get(i).unwrap();
            // half adder
            static_lut!(tmp; 0110; carry, b);
            out.push(tmp.state());
            static_lut!(carry; 1000; carry, b);
        }
    }
    (concat(nzbw, out), carry)
}

// TODO select carry adder
/*
// for every pair of bits, calculate their sums and couts assuming 0 or 1 cins.
let mut s0_i = a ^ b; // a ^ b ^ 0
let mut s1_i = !s0_i; // a ^ b ^ 1
let mut c0_i = a & b; // carry of a + b + 0
let mut c1_i = a | b; // carry of a + b + 1
for i in 0..lb {
    let s0_tmp = carry_block_mux(c0_i, s0_i, s1_i, i).0;
    let s1_tmp = carry_block_mux(c1_i, s0_i, s1_i, i).1;
    let c0_tmp = carry_block_mux(c0_i, c0_i, c1_i, i).0;
    let c1_tmp = carry_block_mux(c1_i, c0_i, c1_i, i).1;
    s0_i = s0_tmp;
    s1_i = s1_tmp;
    c0_i = c0_tmp;
    c1_i = c1_tmp;
}
*/
pub fn cin_sum(cin: &Bits, lhs: &Bits, rhs: &Bits) -> (Awi, inlawi_ty!(1), inlawi_ty!(1)) {
    debug_assert_eq!(cin.bw(), 1);
    debug_assert_eq!(lhs.bw(), rhs.bw());
    let w = lhs.bw();
    let nzbw = lhs.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    let mut carry = InlAwi::from(cin.to_bool());
    for i in 0..w {
        let mut sum = inlawi!(0);
        let mut next_carry = inlawi!(0);
        static_lut!(sum; 1001_0110;
            carry,
            lhs.get(i).unwrap(),
            rhs.get(i).unwrap()
        );
        static_lut!(next_carry; 1110_1000;
            carry,
            lhs.get(i).unwrap(),
            rhs.get(i).unwrap()
        );
        out.push(sum.state());
        carry = next_carry;
    }
    let mut signed_overflow = inlawi!(0);
    let a = lhs.get(w - 1).unwrap().state();
    let b = rhs.get(w - 1).unwrap().state();
    let c = *out.get(w - 1).unwrap();
    signed_overflow
        .update_state(
            bw(1),
            Op::StaticLut(ConcatType::from_iter([a, b, c]), {
                use awi::*;
                awi!(0001_1000)
            }),
        )
        .unwrap_at_runtime();
    (concat(nzbw, out), carry, signed_overflow)
}

pub fn negator(x: &Bits, neg: &Bits) -> Awi {
    debug_assert_eq!(neg.bw(), 1);
    let nzbw = x.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    let mut carry = InlAwi::from(neg.to_bool());
    for i in 0..x.bw() {
        let mut sum = inlawi!(0);
        let mut next_carry = inlawi!(0);
        // half adder with input inversion control
        static_lut!(sum; 1001_0110; carry, x.get(i).unwrap(), neg);
        static_lut!(next_carry; 0010_1000; carry, x.get(i).unwrap(), neg);
        out.push(sum.state());
        carry = next_carry;
    }
    concat(nzbw, out)
}

/// Setting `width` to 0 guarantees that nothing happens even with other
/// arguments being invalid
pub fn field_to(lhs: &Bits, to: &Bits, rhs: &Bits, width: &Bits) -> Awi {
    // the max shift value that can be anything but an effective no-op
    if let Some(s_w) = Bits::nontrivial_bits(lhs.bw() - 1) {
        // first, create the shifted image of `rhs`
        let mut s = Awi::zero(s_w);
        s.resize_(to, false);
        let mut wide_rhs = Awi::opaque(NonZeroUsize::new(2 << s_w.get()).unwrap());
        let mut rev_rhs = Awi::zero(rhs.nzbw());
        rev_rhs.copy_(rhs).unwrap();
        rev_rhs.rev_();
        if let Some(field_to) = lhs.bw().checked_sub(rhs.bw()) {
            let _ = wide_rhs.field_to(field_to, &rev_rhs, rhs.bw());
        } else {
            let field_from = rhs.bw().wrapping_sub(lhs.bw());
            let _ = wide_rhs.field_from(&rev_rhs, field_from, lhs.bw());
        }
        let tmp = funnel(&wide_rhs, &s);
        let mut funnel_res = Awi::zero(lhs.nzbw());
        funnel_res.resize_(&tmp, false);
        funnel_res.rev_();

        // second, we need two masks to indicate where the `width`-sized window is
        // placed

        // need an extra bit for the `tsmear_inx` to work in all circumstances
        let s_w = NonZeroUsize::new(s_w.get().checked_add(1).unwrap()).unwrap();
        let mut small_to = Awi::zero(s_w);
        small_to.usize_(to.to_usize());
        let mut small_width = Awi::zero(s_w);
        small_width.usize_(width.to_usize());
        // to + width
        let mut to_plus_width = small_width;
        to_plus_width.add_(&small_to).unwrap();
        // trailing mask that trails `to + width`, exclusive
        let tmask = tsmear_inx(&to_plus_width, lhs.bw());
        // leading mask that leads `to`, inclusive, implemented by negating a trailing
        // mask of `to`
        let lmask = tsmear_inx(&small_to, lhs.bw());

        // third, multiplex based on the masks
        let mut out = SmallVec::with_capacity(lhs.bw());
        for i in 0..lhs.bw() {
            let mut signal = inlawi!(0);
            static_lut!(
                signal; 1111_1011_0100_0000;
                lmask[i], tmask[i], funnel_res.get(i).unwrap(), lhs.get(i).unwrap()
            );
            out.push(signal.state());
        }

        concat(lhs.nzbw(), out)
    } else {
        let mut out = Awi::from_bits(lhs);
        let small_width = Awi::from_bool(width.lsb());
        let _ = out.field_width(rhs, small_width.to_usize());
        out
    }
}

/// Setting `width` to 0 guarantees that nothing happens even with other
/// arguments being invalid
pub fn field(lhs: &Bits, to: &Bits, rhs: &Bits, from: &Bits, width: &Bits) -> Awi {
    // we can shift both ways now, from the msb of `rhs` to the lsb of `lhs` and the
    // lsb of `rhs` to the msb of `lhs`.
    if let Some(s_w) = Bits::nontrivial_bits(lhs.bw() + rhs.bw() - 2) {
        // we do this to achieve fielding with a single shift construct

        // `from` cannot be more than `rhs.bw() - 1` under valid no-op conditions, so we
        // calculate `to - from` offsetted by `rhs.bw() - 1` to keep it positive. The
        // opposite extreme of `to == lhs.bw() - 1` and `from == 0` cannot overflow
        // because of the way `s_w` was made.
        let mut s = Awi::zero(s_w);
        let mut small_from = Awi::zero(s_w);
        let mut small_to = Awi::zero(s_w);
        small_from.resize_(from, false);
        small_to.resize_(to, false);
        s.usize_(rhs.bw() - 1);
        s.sub_(&small_from).unwrap();
        s.add_(&small_to).unwrap();

        // first, create the shifted image of `rhs`
        let mut wide_rhs = Awi::opaque(NonZeroUsize::new(2 << s_w.get()).unwrap());
        let mut rev_rhs = Awi::zero(rhs.nzbw());
        rev_rhs.copy_(rhs).unwrap();
        rev_rhs.rev_();
        let _ = wide_rhs.field_to(lhs.bw() - 1, &rev_rhs, rhs.bw());
        let tmp = funnel(&wide_rhs, &s);
        let mut funnel_res = Awi::zero(lhs.nzbw());
        funnel_res.resize_(&tmp, false);
        funnel_res.rev_();

        // second, we need two masks to indicate where the `width`-sized window is
        // placed

        // need an extra bit for the `tsmear_inx` to work in all circumstances
        let s_w = NonZeroUsize::new(s_w.get().checked_add(1).unwrap()).unwrap();
        let mut small_to = Awi::zero(s_w);
        small_to.usize_(to.to_usize());
        let mut small_width = Awi::zero(s_w);
        small_width.usize_(width.to_usize());
        // to + width
        let mut to_plus_width = small_width;
        to_plus_width.add_(&small_to).unwrap();
        // trailing mask that trails `to + width`, exclusive
        let tmask = tsmear_inx(&to_plus_width, lhs.bw());
        // leading mask that leads `to`, inclusive, implemented by negating a trailing
        // mask of `to`
        let lmask = tsmear_inx(&small_to, lhs.bw());

        // third, multiplex based on the masks
        let mut out = SmallVec::with_capacity(lhs.bw());
        for i in 0..lhs.bw() {
            let mut signal = inlawi!(0);
            static_lut!(
                signal; 1111_1011_0100_0000;
                lmask[i], tmask[i], funnel_res.get(i).unwrap(), lhs.get(i).unwrap()
            );
            out.push(signal.state());
        }

        concat(lhs.nzbw(), out)
    } else {
        let mut out = Awi::from_bits(lhs);
        let small_width = Awi::from_bool(width.lsb());
        let _ = out.field_width(rhs, small_width.to_usize());
        out
    }
}

pub fn equal(lhs: &Bits, rhs: &Bits) -> inlawi_ty!(1) {
    let mut ranks = vec![vec![]];
    for i in 0..lhs.bw() {
        let mut tmp1 = inlawi!(0);
        static_lut!(tmp1; 1001; lhs.get(i).unwrap(), rhs.get(i).unwrap());
        ranks[0].push(tmp1);
    }
    // binary tree reduce
    loop {
        let prev_rank = ranks.last().unwrap();
        let rank_len = prev_rank.len();
        if rank_len == 1 {
            break prev_rank[0]
        }
        let mut next_rank = vec![];
        for i in 0..(rank_len / 2) {
            let mut tmp1 = inlawi!(0);
            static_lut!(tmp1; 1000; prev_rank[2 * i], prev_rank[2 * i + 1]);
            next_rank.push(tmp1);
        }
        if (rank_len & 1) != 0 {
            next_rank.push(*prev_rank.last().unwrap())
        }
        ranks.push(next_rank);
    }
}

/// Uses the minimum number of bits to handle all cases, you may need to call
/// `to_usize` on the result
pub fn count_ones(x: &Bits) -> Awi {
    // a tuple of an intermediate sum and the max possible value of that sum
    let mut ranks: Vec<Vec<(Awi, awi::Awi)>> = vec![vec![]];
    for i in 0..x.bw() {
        ranks[0].push((Awi::from(x.get(i).unwrap()), awi::Awi::from(true)));
    }
    loop {
        let prev_rank = ranks.last().unwrap();
        let rank_len = prev_rank.len();
        if rank_len == 1 {
            break prev_rank[0].0.clone()
        }
        let mut next_rank = vec![];
        let mut i = 0;
        loop {
            if i >= rank_len {
                break
            }
            // each rank adds another bit, keep adding until overflow
            let mut next_sum = awi!(0, prev_rank[i].0);
            let mut next_max = {
                use awi::*;
                awi!(0, prev_rank[i].1)
            };
            loop {
                i += 1;
                if i >= rank_len {
                    break
                }
                let w = next_max.bw();
                {
                    use awi::*;
                    let mut tmp = Awi::zero(next_max.nzbw());
                    if tmp
                        .cin_sum_(
                            false,
                            &awi!(zero: .., prev_rank[i].1; ..w).unwrap(),
                            &next_max,
                        )
                        .unwrap()
                        .0
                    {
                        // do not add another previous sum to this sum because of overflow
                        break
                    }
                    cc!(tmp; next_max).unwrap();
                }
                next_sum
                    .add_(&awi!(zero: .., prev_rank[i].0; ..w).unwrap())
                    .unwrap();
            }
            next_rank.push((next_sum, next_max));
        }
        ranks.push(next_rank);
    }
}

// If there is a set bit, it and the bits less significant than it will be set
pub fn tsmear(x: &Bits) -> Awi {
    let mut tmp0 = Awi::from(x);
    let mut lvl = 0;
    // exponentially OR cascade the smear
    loop {
        let s = 1 << lvl;
        if s >= x.bw() {
            break tmp0
        }
        let mut tmp1 = tmp0.clone();
        tmp1.lshr_(s).unwrap();
        tmp0.or_(&tmp1).unwrap();
        lvl += 1;
    }
}

pub fn leading_zeros(x: &Bits) -> Awi {
    let mut tmp = tsmear(x);
    tmp.not_();
    count_ones(&tmp)
}

pub fn trailing_zeros(x: &Bits) -> Awi {
    let mut tmp = Awi::from_bits(x);
    tmp.rev_();
    let mut tmp = tsmear(&tmp);
    tmp.not_();
    count_ones(&tmp)
}

pub fn significant_bits(x: &Bits) -> Awi {
    count_ones(&tsmear(x))
}

pub fn lut_set(table: &Bits, entry: &Bits, inx: &Bits) -> Awi {
    let num_entries = 1 << inx.bw();
    debug_assert_eq!(table.bw(), entry.bw() * num_entries);
    let signals = selector(inx, Some(num_entries));
    let mut out = Awi::from_bits(table);
    for (j, signal) in signals.into_iter().enumerate() {
        for i in 0..entry.bw() {
            let lut_inx = i + (j * entry.bw());
            // mux_ between `lhs` or `entry` based on the signal
            let mut tmp1 = inlawi!(0);
            static_lut!(tmp1; 1100_1010;
                table.get(lut_inx).unwrap(),
                entry.get(i).unwrap(),
                signal
            );
            out.set(lut_inx, tmp1.to_bool()).unwrap();
        }
    }
    out
}

pub fn mul_add(out_w: NonZeroUsize, add: Option<&Bits>, lhs: &Bits, rhs: &Bits) -> Awi {
    // make `rhs` the smaller side, column size will be minimized
    let (lhs, rhs) = if lhs.bw() < rhs.bw() {
        (rhs, lhs)
    } else {
        (lhs, rhs)
    };

    let place_map0: &mut Vec<Vec<inlawi_ty!(1)>> = &mut vec![];
    let place_map1: &mut Vec<Vec<inlawi_ty!(1)>> = &mut vec![];
    for _ in 0..out_w.get() {
        place_map0.push(vec![]);
        place_map1.push(vec![]);
    }
    for j in 0..rhs.bw() {
        let rhs_j = rhs.get(j).unwrap();
        for i in 0..lhs.bw() {
            if let Some(place) = place_map0.get_mut(i + j) {
                let mut ji = inlawi!(0);
                static_lut!(ji; 1000; rhs_j, lhs.get(i).unwrap());
                place.push(ji);
            }
        }
    }
    if let Some(add) = add {
        for i in 0..add.bw() {
            if let Some(place) = place_map0.get_mut(i) {
                place.push(inlawi!(add[i]).unwrap());
            }
        }
    }

    // after every bit that will be added is in its place, the columns of bits
    // sharing the same place are counted, resulting in a new set of columns, and
    // the process is repeated again. This reduces very quickly e.g. 65 -> 7 -> 3 ->
    // 2. The final set of 2 deep columns is added together with a fast adder.

    loop {
        let mut gt2 = false;
        for i in 0..place_map0.len() {
            if place_map0[i].len() > 2 {
                gt2 = true;
            }
        }
        if !gt2 {
            // if all columns 2 or less in height, break and use a fast adder
            break
        }
        for i in 0..place_map0.len() {
            if let Some(w) = NonZeroUsize::new(place_map0[i].len()) {
                let mut column = Awi::zero(w);
                for (i, bit) in place_map0[i].drain(..).enumerate() {
                    column.set(i, bit.to_bool()).unwrap();
                }
                let row = count_ones(&column);
                for j in 0..row.bw() {
                    if let Some(place) = place_map1.get_mut(i + j) {
                        place.push(inlawi!(row[j]).unwrap())
                    }
                }
            }
        }
        mem::swap(place_map0, place_map1);
    }

    let mut out = Awi::zero(out_w);
    let mut tmp = Awi::zero(out_w);
    for i in 0..out.bw() {
        for (j, bit) in place_map0[i].iter().enumerate() {
            if j == 0 {
                out.set(i, bit.to_bool()).unwrap();
            } else if j == 1 {
                tmp.set(i, bit.to_bool()).unwrap();
            } else {
                unreachable!()
            }
        }
    }
    out.add_(&tmp).unwrap();
    out
}

/// DAG version of division, most implementations should probably use a fast
/// multiplier and a combination of the algorithms in the `specialized-div-rem`
/// crate, or Goldschmidt division. TODO if `div` is constant or there are
/// enough divisions sharing the same divisor, use fixed point inverses and
/// multiplication. TODO try out other algorithms in the `specialized-div-rem`
/// crate for this implementation.
pub fn division(duo: &Bits, div: &Bits) -> (Awi, Awi) {
    debug_assert_eq!(duo.bw(), div.bw());

    // this uses the nonrestoring SWAR algorithm, with `duo` and `div` extended by
    // one bit so we don't need one of the edge case handlers. TODO can we
    // remove or optimize more of the prelude?

    let original_w = duo.nzbw();
    let w = NonZeroUsize::new(original_w.get() + 1).unwrap();
    let mut tmp = Awi::zero(w);
    tmp.resize_(duo, false);
    let duo = tmp;
    let mut tmp = Awi::zero(w);
    tmp.resize_(div, false);
    let div = tmp;

    let div_original = div.clone();

    /*
    if div == 0 {
        $zero_div_fn()
    }
    if duo < div {
        return (0, duo)
    }
    // SWAR opening
    let div_original = div;

    let mut shl = (div.leading_zeros() - duo.leading_zeros()) as usize;
    if duo < (div << shl) {
        // when the msb of `duo` and `div` are aligned, the resulting `div` may be
        // larger than `duo`, so we decrease the shift by 1.
        shl -= 1;
    }
    let mut div: $uX = (div << shl);
    duo = duo.wrapping_sub(div);
    let mut quo: $uX = 1 << shl;
    if duo < div_original {
        return (quo, duo);
    }
    // NOTE: only with extended `duo` and `div` can we do this
    let mask: $uX = (1 << shl) - 1;

    // central loop
    let div: $uX = div.wrapping_sub(1);
    let mut i = shl;
    loop {
        if i == 0 {
            break;
        }
        i -= 1;
        // note: the `wrapping_shl(1)` can be factored out, but would require another
        // restoring division step to prevent `(duo as $iX)` from overflowing
        if (duo as $iX) < 0 {
            // Negated binary long division step.
            duo = duo.wrapping_shl(1).wrapping_add(div);
        } else {
            // Normal long division step.
            duo = duo.wrapping_shl(1).wrapping_sub(div);
        }
    }
    if (duo as $iX) < 0 {
        // Restore. This was not needed in the original nonrestoring algorithm because of
        // the `duo < div_original` checks.
        duo = duo.wrapping_add(div);
    }
    // unpack
    return ((duo & mask) | quo, duo >> shl);
    */

    let duo_lt_div = duo.ult(&div).unwrap();

    // if there is a shortcut value it gets put in here and the `short`cut flag is
    // set to disable downstream shortcuts
    let mut short_quo = Awi::zero(w);
    let mut short_rem = Awi::zero(w);
    // leave `short_quo` as zero in both cases
    short_rem.mux_(&duo, duo_lt_div).unwrap();
    let mut short = duo_lt_div;

    let mut shl = leading_zeros(&div);
    shl.sub_(&leading_zeros(&duo)).unwrap();
    // if duo < (div << shl)
    let mut shifted_div = Awi::from_bits(&div);
    shifted_div.shl_(shl.to_usize()).unwrap();
    let reshift = duo.ult(&shifted_div).unwrap();
    shl.dec_(!reshift);

    // if we need to reshift to correct for the shl decrement
    let mut reshifted = shifted_div.clone();
    reshifted.lshr_(1).unwrap();
    let mut div = shifted_div;
    div.mux_(&reshifted, reshift).unwrap();

    let mut duo = Awi::from_bits(&duo);
    duo.sub_(&div).unwrap();
    // 1 << shl efficiently
    let tmp = selector_awi(&shl, Some(w.get()));
    let mut quo = Awi::zero(w);
    quo.resize_(&tmp, false);

    // if duo < div_original
    let b = duo.ult(&div_original).unwrap();
    short_quo.mux_(&quo, b & !short).unwrap();
    short_rem.mux_(&duo, b & !short).unwrap();
    short |= b;
    let mut mask = quo.clone();
    mask.dec_(false);

    // central loop
    div.dec_(false);

    let mut i = shl.clone();
    for _ in 0..w.get() {
        let b = i.is_zero();
        i.dec_(b);

        // Normal or Negated binary long division step.
        let mut tmp0 = div.clone();
        tmp0.neg_(!duo.msb());
        let mut tmp1 = duo.clone();
        tmp1.shl_(1).unwrap();
        tmp1.add_(&tmp0).unwrap();
        duo.mux_(&tmp1, !b).unwrap();
    }
    // final restore
    let mut tmp = Awi::zero(w);
    tmp.mux_(&div, duo.msb()).unwrap();
    duo.add_(&tmp).unwrap();

    // unpack

    let mut tmp_quo = duo.clone();
    tmp_quo.and_(&mask).unwrap();
    tmp_quo.or_(&quo).unwrap();
    let mut tmp_rem = duo.clone();
    tmp_rem.lshr_(shl.to_usize()).unwrap();

    short_quo.mux_(&tmp_quo, !short).unwrap();
    short_rem.mux_(&tmp_rem, !short).unwrap();

    let mut tmp0 = Awi::zero(original_w);
    let mut tmp1 = Awi::zero(original_w);
    tmp0.resize_(&short_quo, false);
    tmp1.resize_(&short_rem, false);
    (tmp0, tmp1)
}
