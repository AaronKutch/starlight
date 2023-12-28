use starlight::{awi, dag::*, Epoch, EvalAwi, LazyAwi};

// this is done separately from the benchmarks because getting the `ensemble` is
// expensive
#[test]
fn stats_optimize_funnel() {
    let epoch = Epoch::new();

    let rhs = LazyAwi::opaque(bw(64));
    let s = LazyAwi::opaque(bw(5));
    let mut out = inlawi!(0u32);
    out.funnel_(&rhs, &s).unwrap();
    let _eval = EvalAwi::from(&out);
    epoch.prune().unwrap();
    epoch.lower().unwrap();
    epoch.assert_assertions(true).unwrap();
    epoch.ensemble(|ensemble| {
        awi::assert_eq!(ensemble.stator.states.len(), 2436);
        awi::assert_eq!(ensemble.backrefs.len_keys(), 8559);
        awi::assert_eq!(ensemble.backrefs.len_vals(), 1317);
    });
    epoch.optimize().unwrap();
    epoch.assert_assertions(true).unwrap();
    epoch.ensemble(|ensemble| {
        awi::assert_eq!(ensemble.stator.states.len(), 0);
        awi::assert_eq!(ensemble.backrefs.len_keys(), 5818);
        awi::assert_eq!(ensemble.backrefs.len_vals(), 1237);
    });
}
