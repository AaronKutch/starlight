//! Using combined ordinary and mimick types to assist in lowering

use std::{cmp::min, mem, num::NonZeroUsize};

use awint::{
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        ConcatFieldsType,
    },
    bw,
};

use crate::{
    awi,
    awint_dag::{ConcatType, Lineage, Op},
    dag::{awi, inlawi, inlawi_ty, Awi, Bits, InlAwi},
};

const USIZE_BITS: usize = usize::BITS as usize;

// This code here is especially messy because we do not want to get into
// infinite lowering loops. These first few functions need to use manual
// concatenation and only literal macros within loop blocks.

// Everything used to be done through `get` and `set`, but going straight to
// `StaticLut` or `Concat` or `ConcatFields` is a massive performance boost.

// TODO In the future if we want something more, we should have some kind of
// caching for known optimization results.

// note that the $inx arguments are in order from least to most significant
macro_rules! static_lut {
    ($lhs:ident; $lut:expr; $($inx:expr),*) => {{
        let nzbw = $lhs.state_nzbw();
        let op = Op::StaticLut(
            ConcatType::from_iter([$(
                $inx.state(),
            )*]),
            {use awi::*; awi!($lut)}
        );
        $lhs.update_state(
            nzbw,
            op,
        ).unwrap_at_runtime()
    }};
}

/// Given `inx.bw()` bits, this returns `2^inx.bw()` signals for every possible
/// state of `inx`. The `i`th signal is true only if `inx.to_usize() == i`.
/// `cap` optionally restricts the number of signals. If `cap` is 0, there is
/// one signal line set to true unconditionally.
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
    let mut signals = vec![];
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
    Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(signals)))
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
    let mut signals = vec![];
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

                static_lut!(signal; 11111000; inx.get(j).unwrap(), prefix_equal, signal);

                static_lut!(prefix_equal; 0100; inx.get(j).unwrap(), prefix_equal);
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

                static_lut!(signal; 11111000; inx.get(j).unwrap(), prefix_equal, signal);

                static_lut!(prefix_equal; 0100; inx.get(j).unwrap(), prefix_equal);
            } else {
                // just update equality, the `j`th bit of `i` is 1 and cannot be less than
                // whatever the `inx` bit is
                static_lut!(prefix_equal; 1000; inx.get(j).unwrap(), prefix_equal);
            }
        }
        signals.push(signal.state());
    }
    Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(signals)))
}

pub fn mux_(x0: &Bits, x1: &Bits, inx: &Bits) -> Awi {
    assert_eq!(x0.bw(), x1.bw());
    assert_eq!(inx.bw(), 1);
    let nzbw = x0.nzbw();
    let mut signals = SmallVec::with_capacity(nzbw.get());
    for i in 0..x0.bw() {
        let mut tmp = inlawi!(0);
        static_lut!(tmp; 1100_1010; x0.get(i).unwrap(), x1.get(i).unwrap(), inx);
        signals.push(tmp.state());
    }
    Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(signals)))
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
    assert!(table.bw() == (out.bw().checked_mul(1 << inx.bw()).unwrap()));
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
    out.update_state(nzbw, Op::Concat(ConcatType::from_smallvec(tmp_output)))
        .unwrap_at_runtime();
}

pub fn dynamic_to_static_get(bits: &Bits, inx: &Bits) -> inlawi_ty!(1) {
    if bits.bw() == 1 {
        return InlAwi::from(bits.to_bool())
    }
    let signals = selector(inx, Some(bits.bw()));
    let mut out = inlawi!(0);
    for (i, signal) in signals.iter().enumerate() {
        static_lut!(out; 1111_1000; signal, bits.get(i).unwrap(), out);
    }
    out
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
    Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(out)))
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
        Awi::new(
            w,
            Op::Concat(ConcatType::from_smallvec(smallvec![
                x.state(),
                extension.state()
            ])),
        )
    } else {
        let zero = Awi::zero(NonZeroUsize::new(w.get() - x.bw()).unwrap());
        Awi::new(
            w,
            Op::Concat(ConcatType::from_smallvec(smallvec![
                x.state(),
                zero.state()
            ])),
        )
    }
}

pub fn resize_cond(x: &Bits, w: NonZeroUsize, signed: &Bits) -> Awi {
    assert_eq!(signed.bw(), 1);
    if w == x.nzbw() {
        Awi::from_bits(x)
    } else if w < x.nzbw() {
        Awi::new(
            w,
            Op::ConcatFields(ConcatFieldsType::from_iter([(x.state(), 0usize, w)])),
        )
    } else {
        let extend = x.msb() & signed.to_bool();
        let extension = Awi::new(
            NonZeroUsize::new(w.get() - x.bw()).unwrap(),
            Op::Repeat([extend.state()]),
        );
        Awi::new(
            w,
            Op::Concat(ConcatType::from_smallvec(smallvec![
                x.state(),
                extension.state()
            ])),
        )
    }
}

/// Returns (`lhs`, true) if there are invalid values
pub fn static_field(lhs: &Bits, to: usize, rhs: &Bits, from: usize, width: usize) -> (Awi, bool) {
    if (width > lhs.bw())
        || (width > rhs.bw())
        || (to > (lhs.bw() - width))
        || (from > (rhs.bw() - width))
    {
        return (Awi::from_bits(lhs), true);
    }
    let res = if let Some(width) = NonZeroUsize::new(width) {
        if let Some(lhs_rem_lo) = NonZeroUsize::new(to) {
            if let Some(lhs_rem_hi) = NonZeroUsize::new(from) {
                Awi::new(
                    lhs.nzbw(),
                    Op::ConcatFields(ConcatFieldsType::from_iter([
                        (lhs.state(), 0usize, lhs_rem_lo),
                        (rhs.state(), from, width),
                        (lhs.state(), to + width.get(), lhs_rem_hi),
                    ])),
                )
            } else {
                Awi::new(
                    lhs.nzbw(),
                    Op::ConcatFields(ConcatFieldsType::from_iter([
                        (lhs.state(), 0usize, lhs_rem_lo),
                        (rhs.state(), from, width),
                    ])),
                )
            }
        } else if let Some(lhs_rem_hi) = NonZeroUsize::new(lhs.bw() - width.get()) {
            Awi::new(
                lhs.nzbw(),
                Op::ConcatFields(ConcatFieldsType::from_iter([
                    (rhs.state(), from, width),
                    (lhs.state(), width.get(), lhs_rem_hi),
                ])),
            )
        } else {
            Awi::new(
                lhs.nzbw(),
                Op::ConcatFields(ConcatFieldsType::from_iter([(rhs.state(), from, width)])),
            )
        }
    } else {
        Awi::from_bits(lhs)
    };
    (res, false)
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
    let mux_part = Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(mux_part)));
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

/// Given the diagonal control lines and input of a crossbar with output width
/// s.t. `input.bw() + out.bw() - 1 = signals.bw()`, returns the output. The
/// `i`th input bit and `j`th output bit are controlled by the `out.bw()
/// - 1 + i - j`th control line. `signal_range` uses a virtual `..` range of the
///   possible signals.
pub fn crossbar(
    output: &mut Bits,
    input: &Bits,
    signals: &[inlawi_ty!(1)],
    signal_range: (usize, usize),
) {
    assert!(signal_range.0 < signal_range.1);
    assert_eq!(signal_range.1 - signal_range.0, signals.len());

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
    output
        .update_state(nzbw, Op::Concat(ConcatType::from_smallvec(tmp_output)))
        .unwrap_at_runtime();
}

pub fn funnel_(x: &Bits, s: &Bits) -> Awi {
    assert_eq!(x.bw() & 1, 0);
    assert_eq!(x.bw() / 2, 1 << s.bw());
    let mut out = Awi::zero(NonZeroUsize::new(x.bw() / 2).unwrap());
    let signals = selector(s, None);
    // select zero should connect the zeroeth crossbars, so the offset is `out.bw()
    // - 1 + 0 - 0`
    let range = (out.bw() - 1, out.bw() - 1 + out.bw());
    crossbar(&mut out, x, &signals, range);
    out
}

/// Setting `width` to 0 guarantees that nothing happens even with other
/// arguments being invalid
pub fn field_from(lhs: &Bits, rhs: &Bits, from: &Bits, width: &Bits) -> Awi {
    assert_eq!(from.bw(), USIZE_BITS);
    assert_eq!(width.bw(), USIZE_BITS);
    let mut out = Awi::from_bits(lhs);
    // the `width == 0` case will result in a no-op from the later `field_width`
    // part, so we need to be able to handle just `rhs.bw()` possible shifts for
    // `width == 1` cases. There are `rhs.bw()` output bars needed. `from == 0`
    // should connect the zeroeth crossbars, so the offset is `rhs.bw() - 1 + 0 -
    // 0`. `j` stays zero and we have `0 <= i < rhs.bw()`
    let signals = selector(from, Some(rhs.bw()));
    let range = (rhs.bw() - 1, 2 * rhs.bw() - 1);
    let mut tmp = Awi::zero(rhs.nzbw());
    crossbar(&mut tmp, rhs, &signals, range);
    out.field_width(&tmp, width.to_usize()).unwrap();
    out
}

pub fn shl(x: &Bits, s: &Bits) -> Awi {
    assert_eq!(s.bw(), USIZE_BITS);
    let mut signals = selector(s, Some(x.bw()));
    signals.reverse();
    let mut out = Awi::zero(x.nzbw());
    crossbar(&mut out, x, &signals, (0, x.bw()));
    out
}

pub fn lshr(x: &Bits, s: &Bits) -> Awi {
    assert_eq!(s.bw(), USIZE_BITS);
    let signals = selector(s, Some(x.bw()));
    let mut out = Awi::zero(x.nzbw());
    crossbar(&mut out, x, &signals, (x.bw() - 1, 2 * x.bw() - 1));
    out
}

pub fn ashr(x: &Bits, s: &Bits) -> Awi {
    assert_eq!(s.bw(), USIZE_BITS);
    let signals = selector(s, Some(x.bw()));
    let mut out = Awi::zero(x.nzbw());
    crossbar(&mut out, x, &signals, (x.bw() - 1, 2 * x.bw() - 1));
    // Not sure if there is a better way to do this. If we try to use the crossbar
    // signals in some way, we are guaranteed some kind of > O(1) time thing.

    let msb = x.msb();
    // get the `lb_num` that `tsmear_inx` uses, it can be `x.bw() - 1` because of
    // the `s < x.bw()` requirement, this single bit of difference is important
    // for powers of two because of the `lb_num += 1` condition it avoids.
    let num = x.bw() - 1;
    let next_pow = num.next_power_of_two();
    let mut lb_num = next_pow.trailing_zeros() as usize;
    if next_pow == num {
        // need extra bit to get all `n + 1`
        lb_num += 1;
    }
    if let Some(w) = NonZeroUsize::new(lb_num) {
        let mut gated_s = Awi::zero(w);
        // `gated_s` will be zero if `x.msb()` is zero, in which case `tsmear_inx`
        // produces all zeros to be ORed
        for i in 0..gated_s.bw() {
            let mut tmp1 = inlawi!(0);
            static_lut!(tmp1; 1000; s.get(i).unwrap(), msb);
            gated_s.set(i, tmp1.to_bool()).unwrap();
        }
        let or_mask = tsmear_awi(&gated_s, num);
        for i in 0..or_mask.bw() {
            let out_i = out.bw() - 1 - i;
            let mut tmp1 = inlawi!(0);
            static_lut!(tmp1; 1110; out.get(out_i).unwrap(), or_mask.get(i).unwrap());
            out.set(out_i, tmp1.to_bool()).unwrap();
        }
    }

    out
}

pub fn rotl(x: &Bits, s: &Bits) -> Awi {
    assert_eq!(s.bw(), USIZE_BITS);
    let signals = selector(s, Some(x.bw()));
    // we will use the whole cross bar, with every signal controlling two diagonals
    // for the wraparound except for the `x.bw() - 1` one
    let mut rolled_signals = vec![inlawi!(0); 2 * x.bw() - 1];
    rolled_signals[x.bw() - 1].copy_(&signals[0]).unwrap();
    for i in 0..(x.bw() - 1) {
        rolled_signals[i].copy_(&signals[i + 1]).unwrap();
        rolled_signals[i + x.bw()].copy_(&signals[i + 1]).unwrap();
    }
    rolled_signals.reverse();
    let mut out = Awi::zero(x.nzbw());
    crossbar(&mut out, x, &rolled_signals, (0, 2 * x.bw() - 1));
    out
}

pub fn rotr(x: &Bits, s: &Bits) -> Awi {
    assert_eq!(s.bw(), USIZE_BITS);
    let signals = selector(s, Some(x.bw()));
    // we will use the whole cross bar, with every signal controlling two diagonals
    // for the wraparound except for the `x.bw() - 1` one
    let mut rolled_signals = vec![inlawi!(0); 2 * x.bw() - 1];
    rolled_signals[x.bw() - 1].copy_(&signals[0]).unwrap();
    for i in 0..(x.bw() - 1) {
        rolled_signals[i].copy_(&signals[i + 1]).unwrap();
        rolled_signals[i + x.bw()].copy_(&signals[i + 1]).unwrap();
    }
    let mut out = Awi::zero(x.nzbw());
    crossbar(&mut out, x, &rolled_signals, (0, 2 * x.bw() - 1));
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
    Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(out)))
}

pub fn bitwise(lhs: &Bits, rhs: &Bits, lut: awi::Awi) -> Awi {
    assert_eq!(lhs.bw(), rhs.bw());
    assert_eq!(lut.bw(), 4);
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
    Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(out)))
}

pub fn incrementer(x: &Bits, cin: &Bits, dec: bool) -> (Awi, inlawi_ty!(1)) {
    assert_eq!(cin.bw(), 1);
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
    (
        Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(out))),
        carry,
    )
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
    assert_eq!(cin.bw(), 1);
    assert_eq!(lhs.bw(), rhs.bw());
    let w = lhs.bw();
    let nzbw = lhs.nzbw();
    let mut out = SmallVec::with_capacity(nzbw.get());
    let mut carry = InlAwi::from(cin.to_bool());
    for i in 0..w {
        let mut carry_sum = inlawi!(00);
        static_lut!(carry_sum; 1110_1001_1001_0100; carry, lhs.get(i).unwrap(), rhs.get(i).unwrap());
        out.push(carry_sum.get(0).unwrap().state());
        carry.bool_(carry_sum.get(1).unwrap());
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
    (
        Awi::new(nzbw, Op::Concat(ConcatType::from_smallvec(out))),
        carry,
        signed_overflow,
    )
}

pub fn negator(x: &Bits, neg: &Bits) -> Awi {
    assert_eq!(neg.bw(), 1);
    // half adder with input inversion control
    let lut = inlawi!(0100_1001_1001_0100);
    let mut out = Awi::zero(x.nzbw());
    let mut carry = InlAwi::from(neg.to_bool());
    for i in 0..x.bw() {
        let mut carry_sum = inlawi!(00);
        let mut inx = inlawi!(000);
        inx.set(0, carry.to_bool()).unwrap();
        inx.set(1, x.get(i).unwrap()).unwrap();
        inx.set(2, neg.to_bool()).unwrap();
        carry_sum.lut_(&lut, &inx).unwrap();
        out.set(i, carry_sum.get(0).unwrap()).unwrap();
        carry.bool_(carry_sum.get(1).unwrap());
    }
    out
}

/// Setting `width` to 0 guarantees that nothing happens even with other
/// arguments being invalid
pub fn field_to(lhs: &Bits, to: &Bits, rhs: &Bits, width: &Bits) -> Awi {
    assert_eq!(to.bw(), USIZE_BITS);
    assert_eq!(width.bw(), USIZE_BITS);

    // simplified version of `field` below

    let num = lhs.bw();
    let next_pow = num.next_power_of_two();
    let mut lb_num = next_pow.trailing_zeros() as usize;
    if next_pow == num {
        // need extra bit to get all `n + 1`
        lb_num += 1;
    }
    if let Some(w) = NonZeroUsize::new(lb_num) {
        let mut signals = selector(to, Some(num));
        signals.reverse();

        let mut rhs_to_lhs = Awi::zero(lhs.nzbw());
        crossbar(&mut rhs_to_lhs, rhs, &signals, (0, lhs.bw()));

        // to + width
        let mut tmp = Awi::zero(w);
        tmp.usize_(to.to_usize());
        tmp.add_(&awi!(width[..(w.get())]).unwrap()).unwrap();
        let tmask = tsmear_inx(&tmp, lhs.bw());
        // lhs.bw() - to
        let mut tmp = Awi::zero(w);
        tmp.usize_(lhs.bw());
        tmp.sub_(&awi!(to[..(w.get())]).unwrap()).unwrap();
        let mut lmask = tsmear_inx(&tmp, lhs.bw());
        lmask.reverse();

        let mut out = Awi::from_bits(lhs);
        let lut = inlawi!(1011_1111_1000_0000);
        for i in 0..lhs.bw() {
            let mut tmp = inlawi!(0000);
            tmp.set(0, rhs_to_lhs.get(i).unwrap()).unwrap();
            tmp.set(1, tmask[i].to_bool()).unwrap();
            tmp.set(2, lmask[i].to_bool()).unwrap();
            tmp.set(3, lhs.get(i).unwrap()).unwrap();
            let mut lut_out = inlawi!(0);
            lut_out.lut_(&lut, &tmp).unwrap();
            out.set(i, lut_out.to_bool()).unwrap();
        }
        out
    } else {
        let lut = inlawi!(rhs[0], lhs[0]).unwrap();
        let mut out = awi!(0);
        out.lut_(&lut, width).unwrap();
        out
    }
}

/// Setting `width` to 0 guarantees that nothing happens even with other
/// arguments being invalid
pub fn field(lhs: &Bits, to: &Bits, rhs: &Bits, from: &Bits, width: &Bits) -> Awi {
    assert_eq!(to.bw(), USIZE_BITS);
    assert_eq!(from.bw(), USIZE_BITS);
    assert_eq!(width.bw(), USIZE_BITS);

    // we use some summation to get the fielding done with a single crossbar

    // the basic shift offset is based on `to - from`, to keep the shift value
    // positive in case of `to == 0` and `from == rhs.bw()` we add `rhs.bw()` to
    // this value. The opposite extreme is therefore `to == lhs.bw()` and `from ==
    // 0`, which will be equal to `lhs.bw() + rhs.bw()` because of the added
    // `rhs.bw()`.
    let num = lhs.bw() + rhs.bw();
    let lb_num = num.next_power_of_two().trailing_zeros() as usize;
    if let Some(w) = NonZeroUsize::new(lb_num) {
        let mut shift = Awi::zero(w);
        shift.usize_(rhs.bw());
        shift.add_(&awi!(to[..(w.get())]).unwrap()).unwrap();
        shift.sub_(&awi!(from[..(w.get())]).unwrap()).unwrap();

        let mut signals = selector(&shift, Some(num));
        signals.reverse();

        let mut rhs_to_lhs = Awi::zero(lhs.nzbw());
        // really what `field` is is a well defined full crossbar, the masking part
        // after this is optimized to nothing if `rhs` is zero.
        crossbar(&mut rhs_to_lhs, rhs, &signals, (0, num));

        // `rhs` is now shifted correctly but we need a mask to overwrite the correct
        // bits of `lhs`. We use opposing `tsmears` and AND them together to get the
        // `width` window in the correct spot.

        // to + width
        let mut tmp = Awi::zero(w);
        tmp.usize_(to.to_usize());
        tmp.add_(&awi!(width[..(w.get())]).unwrap()).unwrap();
        let tmask = tsmear_inx(&tmp, lhs.bw());
        // lhs.bw() - to
        let mut tmp = Awi::zero(w);
        tmp.usize_(lhs.bw());
        tmp.sub_(&awi!(to[..(w.get())]).unwrap()).unwrap();
        let mut lmask = tsmear_inx(&tmp, lhs.bw());
        lmask.reverse();

        let mut out = Awi::from_bits(lhs);
        // when `tmask` and `lmask` are both set, mux_ in `rhs`
        let lut = inlawi!(1011_1111_1000_0000);
        for i in 0..lhs.bw() {
            let mut tmp = inlawi!(0000);
            tmp.set(0, rhs_to_lhs.get(i).unwrap()).unwrap();
            tmp.set(1, tmask[i].to_bool()).unwrap();
            tmp.set(2, lmask[i].to_bool()).unwrap();
            tmp.set(3, lhs.get(i).unwrap()).unwrap();
            let mut lut_out = inlawi!(0);
            lut_out.lut_(&lut, &tmp).unwrap();
            out.set(i, lut_out.to_bool()).unwrap();
        }
        out
    } else {
        // `lhs.bw() == 1`, `rhs.bw() == 1`, `width` is the only thing that matters
        let lut = inlawi!(rhs[0], lhs[0]).unwrap();
        let mut out = awi!(0);
        out.lut_(&lut, width).unwrap();
        out
    }
}

pub fn equal(lhs: &Bits, rhs: &Bits) -> inlawi_ty!(1) {
    let mut ranks = vec![vec![]];
    let lut_xnor = inlawi!(1001);
    for i in 0..lhs.bw() {
        let mut tmp0 = inlawi!(00);
        tmp0.set(0, lhs.get(i).unwrap()).unwrap();
        tmp0.set(1, rhs.get(i).unwrap()).unwrap();
        let mut tmp1 = inlawi!(0);
        tmp1.lut_(&lut_xnor, &tmp0).unwrap();
        ranks[0].push(tmp1);
    }
    // binary tree reduce
    let lut_and = inlawi!(1000);
    loop {
        let prev_rank = ranks.last().unwrap();
        let rank_len = prev_rank.len();
        if rank_len == 1 {
            break prev_rank[0]
        }
        let mut next_rank = vec![];
        for i in 0..(rank_len / 2) {
            let mut tmp0 = inlawi!(00);
            tmp0.set(0, prev_rank[2 * i].to_bool()).unwrap();
            tmp0.set(1, prev_rank[2 * i + 1].to_bool()).unwrap();
            let mut tmp1 = inlawi!(0);
            tmp1.lut_(&lut_and, &tmp0).unwrap();
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
    assert_eq!(table.bw(), entry.bw() * num_entries);
    let signals = selector(inx, Some(num_entries));
    let mut out = Awi::from_bits(table);
    let lut_mux = inlawi!(1100_1010);
    for (j, signal) in signals.into_iter().enumerate() {
        for i in 0..entry.bw() {
            let lut_inx = i + (j * entry.bw());
            // mux_ between `lhs` or `entry` based on the signal
            let mut tmp0 = inlawi!(000);
            tmp0.set(0, table.get(lut_inx).unwrap()).unwrap();
            tmp0.set(1, entry.get(i).unwrap()).unwrap();
            tmp0.set(2, signal.to_bool()).unwrap();
            let mut tmp1 = inlawi!(0);
            tmp1.lut_(&lut_mux, &tmp0).unwrap();
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

    let and = inlawi!(1000);
    let place_map0: &mut Vec<Vec<inlawi_ty!(1)>> = &mut vec![];
    let place_map1: &mut Vec<Vec<inlawi_ty!(1)>> = &mut vec![];
    for _ in 0..out_w.get() {
        place_map0.push(vec![]);
        place_map1.push(vec![]);
    }
    for j in 0..rhs.bw() {
        for i in 0..lhs.bw() {
            if let Some(place) = place_map0.get_mut(i + j) {
                let mut tmp = inlawi!(00);
                tmp.set(0, rhs.get(j).unwrap()).unwrap();
                tmp.set(1, lhs.get(i).unwrap()).unwrap();
                let mut ji = inlawi!(0);
                ji.lut_(&and, &tmp).unwrap();
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
    assert_eq!(duo.bw(), div.bw());

    // this uses the nonrestoring SWAR algorithm, with `duo` and `div` extended by
    // one bit so we don't need one of the edge case handlers. TODO can we
    // remove or optimize more of the prelude?

    let original_w = duo.nzbw();
    let w = NonZeroUsize::new(original_w.get() + 1).unwrap();
    let mut tmp = Awi::zero(w);
    tmp.zero_resize_(duo);
    let duo = tmp;
    let mut tmp = Awi::zero(w);
    tmp.zero_resize_(div);
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
    quo.zero_resize_(&tmp);

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
    tmp0.zero_resize_(&short_quo);
    tmp1.zero_resize_(&short_rem);
    (tmp0, tmp1)
}
