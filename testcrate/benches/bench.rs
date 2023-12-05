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
        epoch0.lower().unwrap();
        epoch0.assert_assertions().unwrap();
        awi::assert_eq!(epoch0.ensemble().stator.states.len(), 7045);
        awi::assert_eq!(epoch0.ensemble().backrefs.len_keys(), 26250);
        awi::assert_eq!(epoch0.ensemble().backrefs.len_vals(), 4773);
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
        awi::assert_eq!(epoch0.ensemble().stator.states.len(), 7044);
        awi::assert_eq!(epoch0.ensemble().backrefs.len_keys(), 15304);
        awi::assert_eq!(epoch0.ensemble().backrefs.len_vals(), 1236);
    })
}
