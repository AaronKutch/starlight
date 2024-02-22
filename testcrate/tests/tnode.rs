use starlight::{awi, dag, delay, Delay, Epoch, EvalAwi, LazyAwi};

// Note: these tests have duplications between versions with quiescence testing,
// because `EvalAwi`s and quiescence testing both do lowering stuff, and we need
// to make sure both can immediately start without lowering

#[test]
fn tnode_simple() {
    use dag::*;
    let epoch = Epoch::new();
    let x0 = LazyAwi::zero(bw(1));
    let x1 = EvalAwi::from(&x0);
    let x2 = LazyAwi::opaque(bw(1));
    let x3 = EvalAwi::from(&x2);
    x2.drive(&x1).unwrap();
    {
        use awi::*;
        assert_eq!(x3.eval().unwrap(), awi!(0));
        x0.retro_umax_().unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(1));
        epoch.optimize().unwrap();
        x0.retro_zero_().unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(0));
        x0.retro_umax_().unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(1));
    }
    drop(epoch);
}

#[test]
fn tnode_simple_quiescence() {
    use dag::*;
    let epoch = Epoch::new();
    let x0 = LazyAwi::zero(bw(1));
    let x1 = EvalAwi::from(&x0);
    let x2 = LazyAwi::opaque(bw(1));
    let x3 = EvalAwi::from(&x2);
    x2.drive(&x1).unwrap();
    {
        use awi::*;
        // because there is no delay or infinite looping
        assert!(epoch.quiesced().unwrap());
        assert_eq!(x3.eval().unwrap(), awi!(0));
        assert!(epoch.quiesced().unwrap());
        x0.retro_umax_().unwrap();
        assert!(epoch.quiesced().unwrap());
        assert_eq!(x3.eval().unwrap(), awi!(1));
        epoch.optimize().unwrap();
        x0.retro_zero_().unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(0));
        x0.retro_umax_().unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(1));
    }
    drop(epoch);
}

#[test]
fn tnode_loop() {
    use dag::*;
    let epoch = Epoch::new();
    let x0 = LazyAwi::zero(bw(1));
    let mut tmp = awi!(x0);
    tmp.not_();
    let x1 = EvalAwi::from(&tmp);
    let x2 = LazyAwi::opaque(bw(1));
    let x3 = EvalAwi::from(&x2);
    x2.drive(&x1).unwrap();
    x0.drive_with_delay(&x3, 1).unwrap();
    {
        use awi::*;
        assert_eq!(x3.eval().unwrap(), awi!(1));
        epoch.run(Delay::from(1)).unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(0));
        epoch.optimize().unwrap();
        epoch.run(Delay::from(1)).unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(1));
        epoch.run(Delay::from(1)).unwrap();
        assert_eq!(x3.eval().unwrap(), awi!(0));
    }
    drop(epoch);
}

#[test]
fn tnode_loop_quiescence() {
    use dag::*;
    let epoch = Epoch::new();
    let x0 = LazyAwi::zero(bw(1));
    let mut tmp = awi!(x0);
    tmp.not_();
    let x1 = EvalAwi::from(&tmp);
    let x2 = LazyAwi::opaque(bw(1));
    let x3 = EvalAwi::from(&x2);
    x2.drive(&x1).unwrap();
    x0.drive_with_delay(&x3, 1).unwrap();
    {
        use awi::*;
        assert!(!epoch.quiesced().unwrap());
        assert_eq!(x3.eval().unwrap(), awi!(1));
        assert!(!epoch.quiesced().unwrap());
        epoch.run(Delay::from(1)).unwrap();
        assert!(!epoch.quiesced().unwrap());
        assert_eq!(x3.eval().unwrap(), awi!(0));
        assert!(!epoch.quiesced().unwrap());
        epoch.optimize().unwrap();
        assert!(!epoch.quiesced().unwrap());
        epoch.run(Delay::from(1)).unwrap();
        assert!(!epoch.quiesced().unwrap());
        assert_eq!(x3.eval().unwrap(), awi!(1));
        assert!(!epoch.quiesced().unwrap());
        epoch.run(Delay::from(1)).unwrap();
        assert!(!epoch.quiesced().unwrap());
        assert_eq!(x3.eval().unwrap(), awi!(0));
        assert!(!epoch.quiesced().unwrap());
    }
    drop(epoch);
}

#[test]
fn tnode_delay() {
    use dag::*;
    let epoch = Epoch::new();
    let mut x = awi!(0xa_u4);
    delay(&mut x, 10);
    let y = EvalAwi::from(&x);
    {
        use awi::*;
        assert!(y.eval().is_err());
        assert!(!epoch.quiesced().unwrap());
        epoch.run(9).unwrap();
        assert!(!epoch.quiesced().unwrap());
        assert!(y.eval().is_err());
        epoch.run(1).unwrap();
        assert!(epoch.quiesced().unwrap());
        assert_eq!(y.eval().unwrap(), awi!(0xa_u4));
    }
    drop(epoch);
}

#[test]
fn tnode_delay_lowered() {
    use dag::*;
    let epoch = Epoch::new();
    let x = LazyAwi::opaque(bw(4));
    let mut y = awi!(x);
    delay(&mut y, 3);
    let y = EvalAwi::from(&y);
    {
        use awi::*;
        epoch.lower_and_prune().unwrap();
        x.retro_(&awi!(0xa_u4)).unwrap();
        epoch.run(3).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(0xa_u4));
    }
    drop(epoch);
}

#[test]
fn tnode_delay_opaque_quiesced_lowered() {
    use dag::*;
    let epoch = Epoch::new();
    let x = LazyAwi::opaque(bw(4));
    let mut y = awi!(x);
    delay(&mut y, 10);
    let y = EvalAwi::from(&y);
    {
        use awi::*;
        epoch.lower().unwrap();
        // check that we are immediately quiesced when the driver was already opaque
        assert!(epoch.quiesced().unwrap());
        // one more cycle
        x.retro_(&awi!(0xb_u4)).unwrap();
        // check immediately after the `retro_`
        assert!(!epoch.quiesced().unwrap());
        assert!(y.eval_is_all_unknown().unwrap());
        epoch.run(10).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(0xb_u4));
        assert!(epoch.quiesced().unwrap());
    }
    drop(epoch);
}

#[test]
fn tnode_delay_opaque_quiesced() {
    use dag::*;
    let epoch = Epoch::new();
    let x = LazyAwi::opaque(bw(4));
    let mut y = awi!(x);
    delay(&mut y, 10);
    let y = EvalAwi::from(&y);
    {
        use awi::*;
        // check that we are immediately quiesced when the driver was already opaque
        assert!(epoch.quiesced().unwrap());
        // one more cycle
        x.retro_(&awi!(0xb_u4)).unwrap();
        // check immediately after the `retro_`
        assert!(!epoch.quiesced().unwrap());
        assert!(y.eval_is_all_unknown().unwrap());
        epoch.run(10).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(0xb_u4));
        assert!(epoch.quiesced().unwrap());
    }
    drop(epoch);
}
