use starlight::{
    awi,
    awi::*,
    awint_dag::{epoch::register_assertion_bit_for_current_epoch, Location},
    dag, Epoch, EvalAwi, LazyAwi,
};

#[test]
fn lazy_awi() {
    use dag::*;
    let epoch = Epoch::new();

    let x = LazyAwi::opaque(bw(1));
    let mut a = awi!(x);
    a.not_();
    let y = EvalAwi::from(a);

    {
        use awi::*;

        // TODO the solution is to use the `bits` macro in these places
        x.retro_(&awi!(0)).unwrap();

        epoch.verify_integrity().unwrap();
        awi::assert_eq!(y.eval().unwrap(), awi!(1));
        epoch.verify_integrity().unwrap();

        x.retro_(&awi!(1)).unwrap();

        awi::assert_eq!(y.eval().unwrap(), awi!(0));
        epoch.verify_integrity().unwrap();
    }

    // cleans up everything not still used by `LazyAwi`s, `LazyAwi`s deregister
    // rnodes when dropped
    drop(epoch);
}

#[test]
fn invert_twice() {
    use dag::*;
    let epoch = Epoch::new();
    let x = LazyAwi::opaque(bw(1));
    let mut a = awi!(x);
    a.not_();
    let a_copy = a.clone();
    a.lut_(&inlawi!(10), &a_copy).unwrap();
    a.not_();
    let y = EvalAwi::from(a);

    {
        use awi::assert;

        x.retro_bool_(false).unwrap();
        assert!(!y.eval_bool().unwrap());
        epoch.verify_integrity().unwrap();
        x.retro_bool_(true).unwrap();
        assert!(y.eval_bool().unwrap());
    }
    drop(epoch);
}

#[test]
fn multiplier() {
    use dag::*;
    let epoch = Epoch::new();
    let input_a = LazyAwi::opaque(bw(16));
    let input_b = LazyAwi::opaque(bw(16));
    let mut output = inlawi!(zero: ..32);
    output.arb_umul_add_(&input_a, &input_b);
    let output = EvalAwi::from(output);

    {
        input_a.retro_u16_(123u16).unwrap();
        input_b.retro_u16_(77u16).unwrap();
        std::assert_eq!(output.eval_u32().unwrap(), 9471u32);

        epoch.optimize().unwrap();

        input_a.retro_u16_(10u16).unwrap();
        std::assert_eq!(output.eval_u32().unwrap(), 770u32);
    }
    drop(epoch);
}

#[test]
fn const_assertion_fail() {
    let epoch = Epoch::new();
    // directly register because most of the functions calling this have their own
    // handling
    register_assertion_bit_for_current_epoch(false.into(), Location::dummy());
    {
        awi::assert!(epoch.assert_assertions(false).is_err());
    }
    drop(epoch);
}

// make sure that the `opaque` that is masked off does not cause downstream
// `Unknown`s when the field does not actually use it
#[test]
fn unknown_masking() {
    use dag::*;
    let epoch = Epoch::new();
    let x = awi!(opaque: ..3, 1);
    let mut out = awi!(0u3);
    let width = LazyAwi::uone(bw(2));
    out.field_width(&x, width.to_usize()).unwrap();
    let eval = EvalAwi::from(&out);
    {
        use awi::*;
        awi::assert_eq!(eval.eval().unwrap(), awi!(1u3));
        epoch.optimize().unwrap();
        awi::assert_eq!(eval.eval().unwrap(), awi!(1u3));
    }
    drop(epoch);
}

#[test]
fn all_variations() {
    let epoch = Epoch::new();

    let x1 = LazyAwi::opaque(bw(1));
    let x7 = LazyAwi::opaque(bw(7));
    let x8 = LazyAwi::opaque(bw(8));
    let x16 = LazyAwi::opaque(bw(16));
    let x32 = LazyAwi::opaque(bw(32));
    let x64 = LazyAwi::opaque(bw(64));
    let x128 = LazyAwi::opaque(bw(128));
    let x_zero = LazyAwi::zero(bw(7));
    let x_umax = LazyAwi::umax(bw(7));
    let x_imax = LazyAwi::imax(bw(7));
    let x_imin = LazyAwi::imin(bw(7));
    let x_uone = LazyAwi::uone(bw(7));

    let y1 = EvalAwi::from(&x1);
    let y7 = EvalAwi::from(&x7);
    let y8 = EvalAwi::from(&x8);
    let y16 = EvalAwi::from(&x16);
    let y32 = EvalAwi::from(&x32);
    let y64 = EvalAwi::from(&x64);
    let y128 = EvalAwi::from(&x128);
    let y_zero = EvalAwi::from(&x_zero);
    let y_umax = EvalAwi::from(&x_umax);
    let y_imax = EvalAwi::from(&x_imax);
    let y_imin = EvalAwi::from(&x_imin);
    let y_uone = EvalAwi::from(&x_uone);

    epoch.verify_integrity().unwrap();
    assert!(y1.eval().is_err());
    x1.retro_bool_(true).unwrap();
    assert!(y1.eval_bool().unwrap());
    assert!(y8.eval().is_err());
    x8.retro_u8_(u8::MAX).unwrap();
    assert_eq!(y8.eval_u8().unwrap(), u8::MAX);
    x8.retro_i8_(i8::MAX).unwrap();
    assert_eq!(y8.eval_i8().unwrap(), i8::MAX);
    assert!(y16.eval().is_err());
    x16.retro_u16_(u16::MAX).unwrap();
    assert_eq!(y16.eval_u16().unwrap(), u16::MAX);
    x16.retro_i16_(i16::MAX).unwrap();
    assert_eq!(y16.eval_i16().unwrap(), i16::MAX);
    assert!(y32.eval().is_err());
    x32.retro_u32_(u32::MAX).unwrap();
    assert_eq!(y32.eval_u32().unwrap(), u32::MAX);
    x32.retro_i32_(i32::MAX).unwrap();
    assert_eq!(y32.eval_i32().unwrap(), i32::MAX);
    assert!(y64.eval().is_err());
    x64.retro_u64_(u64::MAX).unwrap();
    assert_eq!(y64.eval_u64().unwrap(), u64::MAX);
    x64.retro_i64_(i64::MAX).unwrap();
    assert_eq!(y64.eval_i64().unwrap(), i64::MAX);
    assert!(y128.eval().is_err());
    x128.retro_u128_(u128::MAX).unwrap();
    assert_eq!(y128.eval_u128().unwrap(), u128::MAX);
    x128.retro_i128_(i128::MAX).unwrap();
    assert_eq!(y128.eval_i128().unwrap(), i128::MAX);
    assert_eq!(y_zero.eval().unwrap(), awi!(0u7));
    assert_eq!(y_umax.eval().unwrap(), awi!(umax: ..7));
    assert_eq!(y_imax.eval().unwrap(), awi!(imax: ..7));
    assert_eq!(y_imin.eval().unwrap(), awi!(imin: ..7));
    assert_eq!(y_uone.eval().unwrap(), awi!(uone: ..7));
    x7.retro_zero_().unwrap();
    assert_eq!(y7.eval().unwrap(), awi!(zero: ..7));
    x7.retro_umax_().unwrap();
    assert_eq!(y7.eval().unwrap(), awi!(umax: ..7));
    x7.retro_imax_().unwrap();
    assert_eq!(y7.eval().unwrap(), awi!(imax: ..7));
    x7.retro_imin_().unwrap();
    assert_eq!(y7.eval().unwrap(), awi!(imin: ..7));
    x7.retro_uone_().unwrap();
    assert_eq!(y7.eval().unwrap(), awi!(uone: ..7));
    x7.retro_unknown_().unwrap();
    assert!(y7.eval().is_err());
    x7.retro_const_(&awi!(-2i7)).unwrap();
    assert_eq!(y7.eval().unwrap(), awi!(-2i7));
    assert!(x7.retro_unknown_().is_err());

    drop(epoch);
}
