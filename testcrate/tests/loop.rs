use std::num::NonZeroUsize;

use starlight::{awi, dag::*, Epoch, EvalAwi, LazyAwi, Loop};

#[test]
fn loop_invert() {
    let epoch = Epoch::new();
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
        epoch.drive_loops().unwrap();
        assert_eq!(eval_x.eval().unwrap(), awi!(0));
        epoch.drive_loops().unwrap();
        assert_eq!(eval_x.eval().unwrap(), awi!(1));
    }
    drop(epoch);
}

// tests an incrementing counter
#[test]
fn loop_incrementer() {
    let epoch = Epoch::new();
    let looper = Loop::zero(bw(4));
    let val = EvalAwi::from(&looper);
    let mut tmp = awi!(looper);
    tmp.inc_(true);
    looper.drive(&tmp).unwrap();

    {
        for i in 0..16 {
            awi::assert_eq!(i, val.eval().unwrap().to_usize());
            epoch.drive_loops().unwrap();
        }
    }
    drop(epoch);
}

#[test]
fn loop_net4() {
    let epoch = Epoch::new();
    let mut net = Net::zero(bw(4));
    net.push(&awi!(0xa_u4)).unwrap();
    net.push(&awi!(0xb_u4)).unwrap();
    net.push(&awi!(0xc_u4)).unwrap();
    net.push(&awi!(0xd_u4)).unwrap();
    let val = EvalAwi::from(&net);
    let inx = LazyAwi::opaque(bw(2));
    net.drive(&inx).unwrap();

    {
        use awi::{assert_eq, *};
        inx.retro_(&awi!(0_u2)).unwrap();
        epoch.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xa_u4));

        inx.retro_(&awi!(2_u2)).unwrap();
        epoch.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xc_u4));

        inx.retro_(&awi!(1_u2)).unwrap();
        epoch.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xb_u4));

        inx.retro_(&awi!(3_u2)).unwrap();
        epoch.drive_loops().unwrap();
        assert_eq!(val.eval().unwrap(), awi!(0xd_u4));
    }
    drop(epoch);
}

fn exhaustive_net_test(epoch: &Epoch, num_ports: awi::usize, diff: awi::isize) {
    let mut net = Net::zero(bw(5));
    for i in 0..num_ports {
        let mut port = awi!(0u5);
        port.usize_(i);
        net.push(&port).unwrap();
    }
    let min_w = num_ports.next_power_of_two().trailing_zeros() as awi::usize;
    let w = NonZeroUsize::new((min_w as awi::isize + diff) as awi::usize).unwrap();
    let lazy = LazyAwi::opaque(w);
    let eval_net = EvalAwi::from(&net);
    let res = net.drive(&lazy);
    let eval_res = EvalAwi::from_bool(res.is_none());
    {
        use awi::*;
        epoch.optimize().unwrap();
        for i in 0..(1 << w.get()) {
            let mut inx = Awi::zero(w);
            inx.usize_(i);
            lazy.retro_(&inx).unwrap();
            epoch.drive_loops().unwrap();
            awi::assert_eq!(eval_res.eval().unwrap().to_bool(), i >= num_ports);
            if i < num_ports {
                awi::assert_eq!(eval_net.eval().unwrap().to_usize(), i);
            }
        }
    }
}

#[test]
fn loop_net_no_ports() {
    let epoch = Epoch::new();
    // done separately because it results in an undriven `Loop`
    {
        let net = Net::zero(bw(5));
        let res = net.drive(&awi!(0));
        {
            use awi::assert;
            // always none
            assert!(res.is_none_at_runtime());
        }
    }
    drop(epoch);
}

#[test]
fn loop_net() {
    let epoch = Epoch::new();
    // one port
    {
        let mut net = Net::zero(bw(5));
        net.push(&awi!(0xa_u5)).unwrap();
        let lazy = LazyAwi::opaque(bw(1));
        let eval_net = EvalAwi::from(&net);
        let res = net.drive(&lazy);
        let eval_res = EvalAwi::from_bool(res.is_none());
        {
            use awi::{assert_eq, *};
            lazy.retro_(&awi!(0)).unwrap();
            epoch.drive_loops().unwrap();
            assert_eq!(eval_res.eval().unwrap(), awi!(0));
            assert_eq!(eval_net.eval().unwrap(), awi!(0xa_u5));
            // any nonzero index always returns a `None` from the function
            lazy.retro_(&awi!(1)).unwrap();
            epoch.drive_loops().unwrap();
            assert_eq!(eval_res.eval().unwrap(), awi!(1));
        }
    }
    for num_ports in 3..17 {
        // test with index size one less than needed to index all ports
        exhaustive_net_test(&epoch, num_ports, -1);
        exhaustive_net_test(&epoch, num_ports, 0);
        exhaustive_net_test(&epoch, num_ports, 1);
    }

    drop(epoch);
}
