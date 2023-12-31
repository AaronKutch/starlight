use starlight::{awi, dag::*, Epoch, EvalAwi, LazyAwi};

#[test]
fn lazy_awi() -> Option<()> {
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

    Some(())
}

#[test]
fn invert_twice() {
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
