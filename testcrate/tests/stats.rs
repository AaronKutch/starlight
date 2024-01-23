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
    epoch.prune_unused_states().unwrap();
    epoch.lower().unwrap();
    epoch.assert_assertions(true).unwrap();
    epoch.ensemble(|ensemble| {
        awi::assert_eq!(ensemble.stator.states.len(), 68);
        awi::assert_eq!(ensemble.backrefs.len_keys(), 2607);
        awi::assert_eq!(ensemble.backrefs.len_vals(), 101);
    });
    epoch.optimize().unwrap();
    epoch.assert_assertions(true).unwrap();
    epoch.ensemble(|ensemble| {
        awi::assert_eq!(ensemble.stator.states.len(), 0);
        awi::assert_eq!(ensemble.backrefs.len_keys(), 1418);
        awi::assert_eq!(ensemble.backrefs.len_vals(), 101);
    });
}

// checks that states are being lowered and pruned at the right times and in the
// expected amounts, and also that some optimizations are working
#[test]
fn stats_different_prunings() {
    let epoch = Epoch::new();

    let num_ports = 2;
    let w = bw(1);
    let mut net = Net::zero(w);
    for i in 0..num_ports {
        let mut port = awi!(0u1);
        port.usize_(i);
        net.push(&port).unwrap();
    }
    let lazy = LazyAwi::opaque(w);
    let eval_net = EvalAwi::from(&net);
    let res = net.drive(&lazy);
    let eval_res = EvalAwi::from_bool(res.is_none());
    {
        use awi::{assert_eq, *};

        epoch.ensemble(|ensemble| {
            assert_eq!(ensemble.notary.rnodes().len(), 3);
            assert_eq!(ensemble.stator.states.len(), 14);
            assert_eq!(ensemble.backrefs.len_keys(), 0);
            assert_eq!(ensemble.backrefs.len_vals(), 0);
        });
        epoch.verify_integrity().unwrap();
        epoch.lower().unwrap();
        epoch.verify_integrity().unwrap();
        epoch.ensemble(|ensemble| {
            assert_eq!(ensemble.notary.rnodes().len(), 3);
            assert_eq!(ensemble.stator.states.len(), 11);
            assert_eq!(ensemble.backrefs.len_keys(), 15);
            assert_eq!(ensemble.backrefs.len_vals(), 4);
        });
        epoch.lower_and_prune().unwrap();
        epoch.verify_integrity().unwrap();
        epoch.ensemble(|ensemble| {
            assert_eq!(ensemble.notary.rnodes().len(), 3);
            assert_eq!(ensemble.stator.states.len(), 0);
            assert_eq!(ensemble.backrefs.len_keys(), 11);
            assert_eq!(ensemble.backrefs.len_vals(), 4);
        });
        epoch.optimize().unwrap();
        epoch.verify_integrity().unwrap();
        epoch.ensemble(|ensemble| {
            assert_eq!(ensemble.notary.rnodes().len(), 3);
            assert_eq!(ensemble.stator.states.len(), 0);
            assert_eq!(ensemble.backrefs.len_keys(), 8);
            assert_eq!(ensemble.backrefs.len_vals(), 3);
        });

        for i in 0..(1 << w.get()) {
            let mut inx = Awi::zero(w);
            inx.usize_(i);
            lazy.retro_(&inx).unwrap();
            epoch.drive_loops().unwrap();
            awi::assert_eq!(eval_res.eval_bool().unwrap(), i >= num_ports);
            if i < num_ports {
                awi::assert_eq!(eval_net.eval().unwrap().to_usize(), i);
            }
        }
        drop(epoch);
    }
}
