use starlight::{awi, dag, Delay, Epoch, EvalAwi, LazyAwi};

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
        use awi::{assert_eq, *};
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
    x0.drive(&x3).unwrap();
    {
        use awi::{assert_eq, *};
        epoch.run(Delay::from(1)).unwrap();
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
