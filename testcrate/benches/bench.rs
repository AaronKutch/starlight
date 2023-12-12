#![feature(test)]

extern crate test;
use starlight::{awi, dag::*, Epoch, EvalAwi, LazyAwi};
use test::Bencher;

#[bench]
fn lower_funnel(bencher: &mut Bencher) {
    bencher.iter(|| {
        let epoch0 = Epoch::new();

        let rhs = LazyAwi::opaque(bw(64));
        let s = LazyAwi::opaque(bw(5));
        let mut out = inlawi!(0u32);
        out.funnel_(&rhs, &s).unwrap();
        let _eval = EvalAwi::from(&out);
        epoch0.prune().unwrap();
        epoch0.lower().unwrap();
        epoch0.assert_assertions().unwrap();
    })
}

#[bench]
fn optimize_funnel(bencher: &mut Bencher) {
    bencher.iter(|| {
        let epoch0 = Epoch::new();

        let rhs = LazyAwi::opaque(bw(64));
        let s = LazyAwi::opaque(bw(5));
        let mut out = inlawi!(0u32);
        out.funnel_(&rhs, &s).unwrap();
        let _eval = EvalAwi::from(&out);
        epoch0.prune().unwrap();
        epoch0.optimize().unwrap();
        epoch0.assert_assertions().unwrap();
    })
}

#[bench]
fn loop_net(bencher: &mut Bencher) {
    let epoch0 = Epoch::new();

    let num_ports = 16;
    let mut net = Net::zero(bw(5));
    for i in 0..num_ports {
        let mut port = awi!(0u5);
        port.usize_(i);
        net.push(&port).unwrap();
    }
    let w = bw(4);
    let lazy = LazyAwi::opaque(w);
    let eval_net = EvalAwi::from(&net);
    let res = net.drive(&lazy);
    let eval_res = EvalAwi::from_bool(res.is_none());
    {
        use awi::*;
        epoch0.optimize().unwrap();
        bencher.iter(|| {
            for i in 0..(1 << w.get()) {
                let mut inx = Awi::zero(w);
                inx.usize_(i);
                lazy.retro_(&inx).unwrap();
                epoch0.drive_loops().unwrap();
                awi::assert_eq!(eval_res.eval().unwrap().to_bool(), i >= num_ports);
                if i < num_ports {
                    awi::assert_eq!(eval_net.eval().unwrap().to_usize(), i);
                }
            }
        });
        drop(epoch0);
    }
}
