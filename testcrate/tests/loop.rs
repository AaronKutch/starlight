use starlight::{awi, dag::*, Epoch, EvalAwi, LazyAwi, Loop};

#[test]
fn loop_invert() {
    let epoch0 = Epoch::new();
    let looper = Loop::zero(bw(1));
    let mut x = awi!(looper);
    let x_copy = x.clone();
    x.lut_(&inlawi!(10), &x_copy).unwrap();
    x.not_();
    let x_copy = x.clone();
    x.lut_(&inlawi!(10), &x_copy).unwrap();
    looper.drive(&x).unwrap();

    {
        use awi::{assert_eq, *};

        let eval_x = EvalAwi::from(&x);
        assert_eq!(eval_x.eval().unwrap(), awi!(1));
        epoch0.drive_loops().unwrap();
        assert_eq!(eval_x.eval().unwrap(), awi!(0));
        epoch0.drive_loops().unwrap();
        assert_eq!(eval_x.eval().unwrap(), awi!(1));
    }
    drop(epoch0);
}

// tests an incrementing counter
#[test]
fn loop_incrementer() {
    let epoch0 = Epoch::new();
    let looper = Loop::zero(bw(4));
    let val = EvalAwi::from(&looper);
    let mut tmp = awi!(looper);
    tmp.inc_(true);
    looper.drive(&tmp).unwrap();

    {
        for i in 0..16 {
            awi::assert_eq!(i, val.eval().unwrap().to_usize());
            epoch0.drive_loops().unwrap();
        }
    }
    drop(epoch0);
}

#[test]
fn loop_net() {
    let epoch0 = Epoch::new();
    let mut net = Net::zero(bw(4));
    net.push(&awi!(0xa_u4)).unwrap();
    net.push(&awi!(0xb_u4)).unwrap();
    net.push(&awi!(0xc_u4)).unwrap();
    net.push(&awi!(0xd_u4)).unwrap();
    let val = EvalAwi::from(&net);
    let inx = LazyAwi::opaque(bw(64));
    net.drive(inx.to_usize()).unwrap();

    {
        use awi::{assert_eq, *};
        inx.retro_(&awi!(0_u64)).unwrap();
        epoch0.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xa_u4));

        inx.retro_(&awi!(2_u64)).unwrap();
        epoch0.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xc_u4));

        inx.retro_(&awi!(1_u64)).unwrap();
        epoch0.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xb_u4));

        inx.retro_(&awi!(3_u64)).unwrap();
        epoch0.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xd_u4));
    }
    drop(epoch0);
}
