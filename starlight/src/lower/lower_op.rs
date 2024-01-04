//! Lowers everything into LUT form

// TODO https://github.com/rust-lang/rust-clippy/issues/10577
#![allow(clippy::redundant_clone)]

use std::{cmp::min, num::NonZeroUsize};

use awint::{
    awint_dag::{
        triple_arena::Ptr,
        DummyDefault, EvalError, Lineage,
        Op::{self, *},
        PState,
    },
    bw,
    dag::{awi, inlawi, Awi, Bits, InlAwi},
};

use super::meta::*;
use crate::awi;

pub trait LowerManagement<P: Ptr + DummyDefault> {
    fn graft(&mut self, output_and_operands: &[PState]);
    fn get_nzbw(&self, p: P) -> NonZeroUsize;
    fn is_literal(&self, p: P) -> bool;
    fn usize(&self, p: P) -> usize;
    fn bool(&self, p: P) -> bool;
    fn dec_rc(&mut self, p: P);
}

/// Returns if the lowering is done
pub fn lower_op<P: Ptr + DummyDefault>(
    start_op: Op<P>,
    out_w: NonZeroUsize,
    mut m: impl LowerManagement<P>,
) -> Result<bool, EvalError> {
    match start_op {
        Invalid => return Err(EvalError::OtherStr("encountered `Invalid` in lowering")),
        Opaque(..) | Literal(_) | Assert(_) | Copy(_) | StaticGet(..) | Concat(_)
        | ConcatFields(_) | Repeat(_) | StaticLut(..) => return Ok(true),
        Lut([lut, inx]) => {
            if m.is_literal(lut) {
                return Err(EvalError::OtherStr(
                    "this needs to be handled before this function",
                ));
            } else {
                let mut out = Awi::zero(out_w);
                let lut = Awi::opaque(m.get_nzbw(lut));
                let inx = Awi::opaque(m.get_nzbw(inx));
                dynamic_to_static_lut(&mut out, &lut, &inx);
                m.graft(&[out.state(), lut.state(), inx.state()]);
            }
        }
        Get([bits, inx]) => {
            if m.is_literal(inx) {
                return Err(EvalError::OtherStr(
                    "this needs to be handled before this function",
                ));
            } else {
                let bits = Awi::opaque(m.get_nzbw(bits));
                let inx = Awi::opaque(m.get_nzbw(inx));
                let out = dynamic_to_static_get(&bits, &inx);
                m.graft(&[out.state(), bits.state(), inx.state()]);
            }
        }
        Set([bits, inx, bit]) => {
            if m.is_literal(inx) {
                return Err(EvalError::OtherStr(
                    "this needs to be handled before this function",
                ));
            } else {
                let bits = Awi::opaque(m.get_nzbw(bits));
                let inx = Awi::opaque(m.get_nzbw(inx));
                let bit = Awi::opaque(m.get_nzbw(bit));
                let out = dynamic_to_static_set(&bits, &inx, &bit);
                m.graft(&[out.state(), bits.state(), inx.state(), bit.state()]);
            }
        }
        FieldBit([lhs, to, rhs, from]) => {
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let from = Awi::opaque(m.get_nzbw(from));
            let bit = rhs.get(from.to_usize()).unwrap();
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let to = Awi::opaque(m.get_nzbw(to));
            // keep `lhs` the same, `out` has the set bit
            let mut out = lhs.clone();
            out.set(to.to_usize(), bit).unwrap();
            m.graft(&[
                out.state(),
                lhs.state(),
                to.state(),
                rhs.state(),
                from.state(),
            ]);
        }
        ZeroResize([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = resize(&x, out_w, false);
            m.graft(&[out.state(), x.state()]);
        }
        SignResize([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = resize(&x, out_w, true);
            m.graft(&[out.state(), x.state()]);
        }
        Resize([x, b]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let b = Awi::opaque(m.get_nzbw(b));
            let out = resize_cond(&x, out_w, &b);
            m.graft(&[out.state(), x.state(), b.state()]);
        }
        Lsb([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = x.get(0).unwrap();
            m.graft(&[out.state(), x.state()]);
        }
        Msb([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = x.get(x.bw() - 1).unwrap();
            m.graft(&[out.state(), x.state()]);
        }
        FieldWidth([lhs, rhs, width]) => {
            let lhs_w = m.get_nzbw(lhs);
            let rhs_w = m.get_nzbw(rhs);
            let width_w = m.get_nzbw(width);
            if m.is_literal(width) {
                let width_u = m.usize(width);
                let lhs = Awi::opaque(lhs_w);
                let rhs = Awi::opaque(rhs_w);
                // If `width_u` is out of bounds `out` is created as a no-op of `lhs` as
                // expected
                let out = static_field(&lhs, 0, &rhs, 0, width_u).0;
                m.graft(&[
                    out.state(),
                    lhs.state(),
                    rhs.state(),
                    Awi::opaque(width_w).state(),
                ]);
            } else {
                let lhs = Awi::opaque(lhs_w);
                let rhs = Awi::opaque(rhs_w);
                let width = Awi::opaque(width_w);
                let max = min(lhs_w, rhs_w).get();
                let success = Bits::efficient_ule(width.to_usize(), max);
                let max_width_w = Bits::nontrivial_bits(max).unwrap();
                let width_small =
                    Bits::static_field(&Awi::zero(max_width_w), 0, &width, 0, max_width_w.get())
                        .unwrap();
                // to achieve a no-op we simply set the width to zero
                let mut tmp_width = Awi::zero(max_width_w);
                tmp_width.mux_(&width_small, success.is_some()).unwrap();
                let out = field_width(&lhs, &rhs, &tmp_width);
                m.graft(&[out.state(), lhs.state(), rhs.state(), width.state()]);
            }
        }
        Funnel([x, s]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let s = Awi::opaque(m.get_nzbw(s));
            let out = funnel(&x, &s);
            m.graft(&[out.state(), x.state(), s.state()]);
        }
        FieldFrom([lhs, rhs, from, width]) => {
            let lhs_w = m.get_nzbw(lhs);
            let rhs_w = m.get_nzbw(rhs);
            let width_w = m.get_nzbw(width);
            if m.is_literal(from) {
                let lhs = Awi::opaque(lhs_w);
                let rhs = Awi::opaque(rhs_w);
                let width = Awi::opaque(m.get_nzbw(width));
                let from_u = m.usize(from);
                let out = if rhs.bw() <= from_u {
                    lhs.clone()
                } else {
                    // since `from_u` is known the less significant part of `rhs` can be disregarded
                    let sub_rhs_w = rhs.bw() - from_u;
                    if let Some(w) = NonZeroUsize::new(sub_rhs_w) {
                        let tmp0 = Awi::zero(w);
                        let (tmp1, o) = static_field(&tmp0, 0, &rhs, from_u, sub_rhs_w);
                        let mut out = lhs.clone();
                        if o {
                            out
                        } else {
                            let _ = out.field_width(&tmp1, width.to_usize());
                            out
                        }
                    } else {
                        lhs.clone()
                    }
                };
                m.graft(&[
                    out.state(),
                    lhs.state(),
                    rhs.state(),
                    Awi::opaque(m.get_nzbw(from)).state(),
                    width.state(),
                ]);
            } else {
                let lhs = Awi::opaque(lhs_w);
                let rhs = Awi::opaque(rhs_w);
                let from = Awi::opaque(m.get_nzbw(from));
                let width = Awi::opaque(width_w);

                let success =
                    Bits::efficient_add_then_ule(from.to_usize(), width.to_usize(), rhs.bw());
                let max_width_w = Bits::nontrivial_bits(rhs.bw()).unwrap();
                let width_small =
                    Bits::static_field(&Awi::zero(max_width_w), 0, &width, 0, max_width_w.get())
                        .unwrap();
                // to achieve a no-op we simply set the width to zero
                let mut tmp_width = Awi::zero(max_width_w);
                tmp_width.mux_(&width_small, success.is_some()).unwrap();
                // the optimizations on `width` are done later on an inner `field_width` call
                let out = field_from(&lhs, &rhs, &from, &tmp_width);
                m.graft(&[
                    out.state(),
                    lhs.state(),
                    rhs.state(),
                    from.state(),
                    width.state(),
                ]);
            }
        }
        Shl([x, s]) => {
            if m.is_literal(s) {
                let x = Awi::opaque(m.get_nzbw(x));
                let s_u = m.usize(s);
                let out = if (s_u == 0) || (x.bw() <= s_u) {
                    x.clone()
                } else {
                    let tmp = Awi::zero(x.nzbw());
                    static_field(&tmp, s_u, &x, 0, x.bw() - s_u).0
                };
                m.graft(&[out.state(), x.state(), Awi::opaque(m.get_nzbw(s)).state()]);
            } else {
                let x = Awi::opaque(m.get_nzbw(x));
                let s = Awi::opaque(m.get_nzbw(s));

                let success = Bits::efficient_ule(s.to_usize(), x.bw() - 1);
                let max_s_w = Bits::nontrivial_bits(x.bw() - 1).unwrap_or(bw(1));
                let s_small =
                    Bits::static_field(&Awi::zero(max_s_w), 0, &s, 0, max_s_w.get()).unwrap();
                // to achieve a no-op we simply set the shift to zero
                let mut tmp_s = Awi::zero(max_s_w);
                tmp_s.mux_(&s_small, success.is_some()).unwrap();
                let out = shl(&x, &tmp_s);
                m.graft(&[out.state(), x.state(), s.state()]);
            }
        }
        Lshr([x, s]) => {
            if m.is_literal(s) {
                let x = Awi::opaque(m.get_nzbw(x));
                let s_u = m.usize(s);
                let out = if (s_u == 0) || (x.bw() <= s_u) {
                    x.clone()
                } else {
                    let tmp = Awi::zero(x.nzbw());
                    static_field(&tmp, 0, &x, s_u, x.bw() - s_u).0
                };
                m.graft(&[out.state(), x.state(), Awi::opaque(m.get_nzbw(s)).state()]);
            } else {
                let x = Awi::opaque(m.get_nzbw(x));
                let s = Awi::opaque(m.get_nzbw(s));

                let success = Bits::efficient_ule(s.to_usize(), x.bw() - 1);
                let max_s_w = Bits::nontrivial_bits(x.bw() - 1).unwrap_or(bw(1));
                let s_small =
                    Bits::static_field(&Awi::zero(max_s_w), 0, &s, 0, max_s_w.get()).unwrap();
                let mut tmp_s = Awi::zero(max_s_w);
                tmp_s.mux_(&s_small, success.is_some()).unwrap();
                let out = lshr(&x, &tmp_s);
                m.graft(&[out.state(), x.state(), s.state()]);
            }
        }
        Ashr([x, s]) => {
            if m.is_literal(s) {
                let x = Awi::opaque(m.get_nzbw(x));
                let s_u = m.usize(s);
                let out = if (s_u == 0) || (x.bw() <= s_u) {
                    x.clone()
                } else {
                    let mut tmp = Awi::zero(x.nzbw());
                    for i in 0..x.bw() {
                        tmp.set(i, x.msb()).unwrap();
                    }
                    static_field(&tmp, 0, &x, s_u, x.bw() - s_u).0
                };
                m.graft(&[out.state(), x.state(), Awi::opaque(m.get_nzbw(s)).state()]);
            } else {
                let x = Awi::opaque(m.get_nzbw(x));
                let s = Awi::opaque(m.get_nzbw(s));

                let success = Bits::efficient_ule(s.to_usize(), x.bw() - 1);
                let max_s_w = Bits::nontrivial_bits(x.bw() - 1).unwrap_or(bw(1));
                let s_small =
                    Bits::static_field(&Awi::zero(max_s_w), 0, &s, 0, max_s_w.get()).unwrap();
                let mut tmp_s = Awi::zero(max_s_w);
                tmp_s.mux_(&s_small, success.is_some()).unwrap();
                let out = ashr(&x, &tmp_s);
                m.graft(&[out.state(), x.state(), s.state()]);
            }
        }
        Rotl([x, s]) => {
            if m.is_literal(s) {
                let x = Awi::opaque(m.get_nzbw(x));
                let s_u = m.usize(s);
                let out = if (s_u == 0) || (x.bw() <= s_u) {
                    x.clone()
                } else {
                    let tmp = static_field(&Awi::zero(x.nzbw()), s_u, &x, 0, x.bw() - s_u).0;
                    static_field(&tmp, 0, &x, x.bw() - s_u, s_u).0
                };
                m.graft(&[out.state(), x.state(), Awi::opaque(m.get_nzbw(s)).state()]);
            } else {
                let x = Awi::opaque(m.get_nzbw(x));
                let s = Awi::opaque(m.get_nzbw(s));

                let success = Bits::efficient_ule(s.to_usize(), x.bw() - 1);
                let max_s_w = Bits::nontrivial_bits(x.bw() - 1).unwrap_or(bw(1));
                let s_small =
                    Bits::static_field(&Awi::zero(max_s_w), 0, &s, 0, max_s_w.get()).unwrap();
                let mut tmp_s = Awi::zero(max_s_w);
                tmp_s.mux_(&s_small, success.is_some()).unwrap();
                let out = rotl(&x, &tmp_s);
                m.graft(&[out.state(), x.state(), s.state()]);
            }
        }
        Rotr([x, s]) => {
            if m.is_literal(s) {
                let x = Awi::opaque(m.get_nzbw(x));
                let s_u = m.usize(s);
                let out = if (s_u == 0) || (x.bw() <= s_u) {
                    x.clone()
                } else {
                    let tmp = static_field(&Awi::zero(x.nzbw()), 0, &x, s_u, x.bw() - s_u).0;
                    static_field(&tmp, x.bw() - s_u, &x, 0, s_u).0
                };
                m.graft(&[out.state(), x.state(), Awi::opaque(m.get_nzbw(s)).state()]);
            } else {
                let x = Awi::opaque(m.get_nzbw(x));
                let s = Awi::opaque(m.get_nzbw(s));

                let success = Bits::efficient_ule(s.to_usize(), x.bw() - 1);
                let max_s_w = Bits::nontrivial_bits(x.bw() - 1).unwrap_or(bw(1));
                let s_small =
                    Bits::static_field(&Awi::zero(max_s_w), 0, &s, 0, max_s_w.get()).unwrap();
                let mut tmp_s = Awi::zero(max_s_w);
                tmp_s.mux_(&s_small, success.is_some()).unwrap();
                let out = rotr(&x, &tmp_s);
                m.graft(&[out.state(), x.state(), s.state()]);
            }
        }
        Not([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = bitwise_not(&x);
            m.graft(&[out.state(), x.state()]);
        }
        Or([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = bitwise(&lhs, &rhs, {
                use awi::*;
                awi!(1110)
            });
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        And([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = bitwise(&lhs, &rhs, {
                use awi::*;
                awi!(1000)
            });
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Xor([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = bitwise(&lhs, &rhs, {
                use awi::*;
                awi!(0110)
            });
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Inc([x, cin]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let cin = Awi::opaque(m.get_nzbw(cin));
            let out = incrementer(&x, &cin, false).0;
            m.graft(&[out.state(), x.state(), cin.state()]);
        }
        IncCout([x, cin]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let cin = Awi::opaque(m.get_nzbw(cin));
            let out = incrementer(&x, &cin, false).1;
            m.graft(&[out.state(), x.state(), cin.state()]);
        }
        Dec([x, cin]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let cin = Awi::opaque(m.get_nzbw(cin));
            let out = incrementer(&x, &cin, true).0;
            m.graft(&[out.state(), x.state(), cin.state()]);
        }
        DecCout([x, cin]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let cin = Awi::opaque(m.get_nzbw(cin));
            let out = incrementer(&x, &cin, true).1;
            m.graft(&[out.state(), x.state(), cin.state()]);
        }
        CinSum([cin, lhs, rhs]) => {
            let cin = Awi::opaque(m.get_nzbw(cin));
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = cin_sum(&cin, &lhs, &rhs).0;
            m.graft(&[out.state(), cin.state(), lhs.state(), rhs.state()]);
        }
        UnsignedOverflow([cin, lhs, rhs]) => {
            let cin = Awi::opaque(m.get_nzbw(cin));
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = cin_sum(&cin, &lhs, &rhs).1;
            m.graft(&[out.state(), cin.state(), lhs.state(), rhs.state()]);
        }
        SignedOverflow([cin, lhs, rhs]) => {
            let cin = Awi::opaque(m.get_nzbw(cin));
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = cin_sum(&cin, &lhs, &rhs).2;
            m.graft(&[out.state(), cin.state(), lhs.state(), rhs.state()]);
        }
        Neg([x, neg]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let neg = Awi::opaque(m.get_nzbw(neg));
            debug_assert_eq!(neg.bw(), 1);
            let out = negator(&x, &neg);
            m.graft(&[out.state(), x.state(), neg.state()]);
        }
        Abs([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let mut out = x.clone();
            out.neg_(x.msb());
            m.graft(&[out.state(), x.state()]);
        }
        Add([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = cin_sum(&inlawi!(0), &lhs, &rhs).0;
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Sub([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut rhs_tmp = rhs.clone();
            rhs_tmp.neg_(true);
            let mut out = lhs.clone();
            out.add_(&rhs_tmp).unwrap();
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Rsb([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut out = lhs.clone();
            out.neg_(true);
            out.add_(&rhs).unwrap();
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        FieldTo([lhs, to, rhs, width]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let width = Awi::opaque(m.get_nzbw(width));
            if m.is_literal(to) {
                let to_u = m.usize(to);

                let out = if lhs.bw() < to_u {
                    lhs.clone()
                } else if let Some(w) = NonZeroUsize::new(lhs.bw() - to_u) {
                    let (mut lhs_hi, o) = static_field(&Awi::zero(w), 0, &lhs, to_u, w.get());
                    let _ = lhs_hi.field_width(&rhs, width.to_usize());
                    if o {
                        lhs.clone()
                    } else {
                        static_field(&lhs, to_u, &lhs_hi, 0, w.get()).0
                    }
                } else {
                    lhs.clone()
                };
                m.graft(&[
                    out.state(),
                    lhs.state(),
                    Awi::opaque(m.get_nzbw(to)).state(),
                    rhs.state(),
                    width.state(),
                ]);
            } else {
                let to = Awi::opaque(m.get_nzbw(to));
                let out = field_to(&lhs, &to, &rhs, &width);
                m.graft(&[
                    out.state(),
                    lhs.state(),
                    to.state(),
                    rhs.state(),
                    width.state(),
                ]);
            }
        }
        Field([lhs, to, rhs, from, width]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let width = Awi::opaque(m.get_nzbw(width));
            if m.is_literal(to) || m.is_literal(from) {
                let to = Awi::opaque(m.get_nzbw(to));
                let from = Awi::opaque(m.get_nzbw(from));
                let min_w = min(lhs.bw(), rhs.bw());
                let mut tmp = Awi::zero(NonZeroUsize::new(min_w).unwrap());
                let _ = tmp.field_from(&rhs, from.to_usize(), width.to_usize());
                let mut out = lhs.clone();
                let _ = out.field_to(to.to_usize(), &tmp, width.to_usize());

                m.graft(&[
                    out.state(),
                    lhs.state(),
                    to.state(),
                    rhs.state(),
                    from.state(),
                    width.state(),
                ]);
            } else {
                let to = Awi::opaque(m.get_nzbw(to));
                let from = Awi::opaque(m.get_nzbw(from));
                let out = field(&lhs, &to, &rhs, &from, &width);
                m.graft(&[
                    out.state(),
                    lhs.state(),
                    to.state(),
                    rhs.state(),
                    from.state(),
                    width.state(),
                ]);
            }
        }
        Rev([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = reverse(&x);
            m.graft(&[out.state(), x.state()]);
        }
        Eq([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = equal(&lhs, &rhs);
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Ne([lhs, rhs]) => {
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut out = equal(&lhs, &rhs);
            out.not_();
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Ult([lhs, rhs]) => {
            let w = m.get_nzbw(lhs);
            let lhs = Awi::opaque(w);
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut not_lhs = lhs.clone();
            not_lhs.not_();
            let mut tmp = Awi::zero(w);
            // TODO should probably use some short termination circuit like what
            // `tsmear_inx` uses
            let (out, _) = tmp.cin_sum_(false, &not_lhs, &rhs).unwrap();
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Ule([lhs, rhs]) => {
            let w = m.get_nzbw(lhs);
            let lhs = Awi::opaque(w);
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut not_lhs = lhs.clone();
            not_lhs.not_();
            let mut tmp = Awi::zero(w);
            let (out, _) = tmp.cin_sum_(true, &not_lhs, &rhs).unwrap();
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Ilt([lhs, rhs]) => {
            let w = m.get_nzbw(lhs);
            let lhs = Awi::opaque(w);
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut out = inlawi!(0);
            if w.get() == 1 {
                let mut tmp = inlawi!(00);
                tmp.set(0, lhs.msb()).unwrap();
                tmp.set(1, rhs.msb()).unwrap();
                out.lut_(&inlawi!(0010), &tmp).unwrap();
            } else {
                let lhs_lo = awi!(lhs[..(lhs.bw() - 1)]).unwrap();
                let rhs_lo = awi!(rhs[..(rhs.bw() - 1)]).unwrap();
                let lo_lt = lhs_lo.ult(&rhs_lo).unwrap();
                let mut tmp = inlawi!(000);
                tmp.set(0, lo_lt).unwrap();
                tmp.set(1, lhs.msb()).unwrap();
                tmp.set(2, rhs.msb()).unwrap();
                // if `lhs.msb() != rhs.msb()` then `lhs.msb()` determines signed-less-than,
                // otherwise `lo_lt` determines
                out.lut_(&inlawi!(10001110), &tmp).unwrap();
            }
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        Ile([lhs, rhs]) => {
            let w = m.get_nzbw(lhs);
            let lhs = Awi::opaque(w);
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let mut out = inlawi!(0);
            if w.get() == 1 {
                let mut tmp = inlawi!(00);
                tmp.set(0, lhs.msb()).unwrap();
                tmp.set(1, rhs.msb()).unwrap();
                out.lut_(&inlawi!(1011), &tmp).unwrap();
            } else {
                let lhs_lo = awi!(lhs[..(lhs.bw() - 1)]).unwrap();
                let rhs_lo = awi!(rhs[..(rhs.bw() - 1)]).unwrap();
                let lo_lt = lhs_lo.ule(&rhs_lo).unwrap();
                let mut tmp = inlawi!(000);
                tmp.set(0, lo_lt).unwrap();
                tmp.set(1, lhs.msb()).unwrap();
                tmp.set(2, rhs.msb()).unwrap();
                out.lut_(&inlawi!(10001110), &tmp).unwrap();
            }
            m.graft(&[out.state(), lhs.state(), rhs.state()]);
        }
        op @ (IsZero(_) | IsUmax(_) | IsImax(_) | IsImin(_) | IsUone(_)) => {
            let x = Awi::opaque(m.get_nzbw(op.operands()[0]));
            let w = x.nzbw();
            let out = InlAwi::from(match op {
                IsZero(_) => x.const_eq(&Awi::zero(w)).unwrap(),
                IsUmax(_) => x.const_eq(&Awi::umax(w)).unwrap(),
                IsImax(_) => x.const_eq(&Awi::imax(w)).unwrap(),
                IsImin(_) => x.const_eq(&Awi::imin(w)).unwrap(),
                IsUone(_) => x.const_eq(&Awi::uone(w)).unwrap(),
                _ => unreachable!(),
            });
            m.graft(&[out.state(), x.state()]);
        }
        CountOnes([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = count_ones(&x).to_usize();
            m.graft(&[out.state(), x.state()]);
        }
        Lz([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = leading_zeros(&x).to_usize();
            m.graft(&[out.state(), x.state()]);
        }
        Tz([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = trailing_zeros(&x).to_usize();
            m.graft(&[out.state(), x.state()]);
        }
        Sig([x]) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let out = significant_bits(&x).to_usize();
            m.graft(&[out.state(), x.state()]);
        }
        LutSet([table, entry, inx]) => {
            let table = Awi::opaque(m.get_nzbw(table));
            let entry = Awi::opaque(m.get_nzbw(entry));
            let inx = Awi::opaque(m.get_nzbw(inx));
            let out = lut_set(&table, &entry, &inx);
            m.graft(&[out.state(), table.state(), entry.state(), inx.state()]);
        }
        ZeroResizeOverflow([x], w) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let mut out = Awi::zero(bw(1));
            let w = w.get();
            if w < x.bw() {
                out.bool_(!awi!(x[w..]).unwrap().is_zero());
            }
            m.graft(&[out.state(), x.state()]);
        }
        SignResizeOverflow([x], w) => {
            let x = Awi::opaque(m.get_nzbw(x));
            let mut out = Awi::zero(bw(1));
            let w = w.get();
            if w < x.bw() {
                // the new msb and the bits above it should equal the old msb
                let critical = awi!(x[(w - 1)..]).unwrap();
                let mut tmp = inlawi!(00);
                tmp.set(0, critical.is_zero()).unwrap();
                tmp.set(1, critical.is_umax()).unwrap();
                out.lut_(&inlawi!(1001), &tmp).unwrap();
            }
            m.graft(&[out.state(), x.state()]);
        }
        ArbMulAdd([add, lhs, rhs]) => {
            let w = m.get_nzbw(add);
            let add = Awi::opaque(w);
            let lhs = Awi::opaque(m.get_nzbw(lhs));
            let rhs = Awi::opaque(m.get_nzbw(rhs));
            let out = mul_add(w, Some(&add), &lhs, &rhs);
            m.graft(&[out.state(), add.state(), lhs.state(), rhs.state()]);
        }
        Mux([x0, x1, inx]) => {
            let x0 = Awi::opaque(m.get_nzbw(x0));
            let x1 = Awi::opaque(m.get_nzbw(x1));
            let inx_tmp = Awi::opaque(m.get_nzbw(inx));
            let out = if m.is_literal(inx) {
                let b = m.bool(inx);
                if b {
                    x1.clone()
                } else {
                    x0.clone()
                }
            } else {
                static_mux(&x0, &x1, &inx_tmp)
            };
            m.graft(&[out.state(), x0.state(), x1.state(), inx_tmp.state()]);
        }
        // TODO in the divisions especially and in other operations, we need to look at the
        // operand tree and combine multiple ops together in a single lowering operation
        UQuo([duo, div]) => {
            let duo = Awi::opaque(m.get_nzbw(duo));
            let div = Awi::opaque(m.get_nzbw(div));
            let quo = division(&duo, &div).0;
            m.graft(&[quo.state(), duo.state(), div.state()]);
        }
        URem([duo, div]) => {
            let duo = Awi::opaque(m.get_nzbw(duo));
            let div = Awi::opaque(m.get_nzbw(div));
            let rem = division(&duo, &div).1;
            m.graft(&[rem.state(), duo.state(), div.state()]);
        }
        IQuo([duo, div]) => {
            let duo = Awi::opaque(m.get_nzbw(duo));
            let div = Awi::opaque(m.get_nzbw(div));
            let duo_msb = duo.msb();
            let div_msb = div.msb();
            // keeping arguments opaque
            let mut tmp_duo = duo.clone();
            let mut tmp_div = div.clone();
            tmp_duo.neg_(duo_msb);
            tmp_div.neg_(div_msb);
            let mut quo = division(&tmp_duo, &tmp_div).0;
            let mut tmp0 = InlAwi::from(duo_msb);
            let tmp1 = InlAwi::from(div_msb);
            tmp0.xor_(&tmp1).unwrap();
            quo.neg_(tmp0.to_bool());
            m.graft(&[quo.state(), duo.state(), div.state()]);
        }
        IRem([duo, div]) => {
            let duo = Awi::opaque(m.get_nzbw(duo));
            let div = Awi::opaque(m.get_nzbw(div));
            let duo_msb = duo.msb();
            let div_msb = div.msb();
            // keeping arguments opaque
            let mut tmp_duo = duo.clone();
            let mut tmp_div = div.clone();
            tmp_duo.neg_(duo_msb);
            tmp_div.neg_(div_msb);
            let mut rem = division(&tmp_duo, &tmp_div).1;
            rem.neg_(duo_msb);
            m.graft(&[rem.state(), duo.state(), div.state()]);
        }
    }
    Ok(false)
}
