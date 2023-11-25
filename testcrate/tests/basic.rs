use std::path::PathBuf;

use starlight::{
    awi,
    awint_dag::EvalError,
    dag::{self, *},
    Epoch, EvalAwi, LazyAwi, StarRng,
};

fn _render(epoch: &Epoch) -> awi::Result<(), EvalError> {
    epoch.render_to_svgs_in_dir(PathBuf::from("./".to_owned()))
}

#[test]
fn lazy_awi() -> Option<()> {
    let epoch0 = Epoch::new();

    let mut x = LazyAwi::opaque(bw(1));
    let mut a = awi!(x);
    a.not_();

    {
        use awi::*;
        let mut y = EvalAwi::from(a.as_ref());

        x.retro_(&awi!(0)).unwrap();

        epoch0.ensemble().verify_integrity().unwrap();
        awi::assert_eq!(y.eval().unwrap(), awi!(1));
        epoch0.ensemble().verify_integrity().unwrap();

        x.retro_(&awi!(1)).unwrap();

        awi::assert_eq!(y.eval().unwrap(), awi!(0));
        epoch0.ensemble().verify_integrity().unwrap();
    }

    // cleans up everything not still used by `LazyAwi`s, `LazyAwi`s deregister
    // notes when dropped
    drop(epoch0);

    Some(())
}

#[test]
fn invert_twice() {
    let epoch0 = Epoch::new();
    let mut x = LazyAwi::opaque(bw(1));
    let mut a = awi!(x);
    a.not_();
    let a_copy = a.clone();
    a.lut_(&inlawi!(10), &a_copy).unwrap();
    a.not_();

    {
        use awi::{assert_eq, *};

        let mut y = EvalAwi::from(a.as_ref());
        x.retro_(&awi!(0)).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(0));
        epoch0.ensemble().verify_integrity().unwrap();
        x.retro_(&awi!(1)).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(1));
    }
    drop(epoch0);
}

// TODO should loop be a capability of LazyAwi or something? Have an enum on the
// inside?
/*
#[test]
fn invert_in_loop() {
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

        t_dag.eval_all().unwrap();
        assert_eq!(t_dag.get_noted_as_extawi(p_x).unwrap(), awi!(1));
        t_dag.drive_loops();
        t_dag.eval_all().unwrap();
        assert_eq!(t_dag.get_noted_as_extawi(p_x).unwrap(), awi!(0));
        t_dag.drive_loops();
        t_dag.eval_all().unwrap();
        assert_eq!(t_dag.get_noted_as_extawi(p_x).unwrap(), awi!(1));
    }
}

// tests an incrementing counter
#[test]
fn incrementer() {
    let epoch0 = StateEpoch::new();
    let looper = Loop::zero(bw(4));
    let val = Awi::from(looper.as_ref());
    let mut tmp = Awi::from(looper.as_ref());
    tmp.inc_(true);
    looper.drive(&tmp).unwrap();

    let (mut op_dag, res) = OpDag::from_epoch(&epoch0);
    res.unwrap();

    let p_val = op_dag.note_pstate(&epoch0, val.state()).unwrap();

    op_dag.lower_all().unwrap();

    let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
    res.unwrap();

    t_dag.verify_integrity().unwrap();

    t_dag.eval_all().unwrap();

    t_dag.optimize_basic();

    for i in 0..16 {
        std::assert_eq!(i, t_dag.get_noted_as_extawi(p_val).unwrap().to_usize());

        t_dag.drive_loops();
        t_dag.eval_all().unwrap();
    }
}

// tests getting and setting outputs
#[test]
fn multiplier() {
    let epoch0 = StateEpoch::new();
    let input_a = inlawi!(opaque: ..16);
    let input_b = inlawi!(opaque: ..16);
    let mut output = inlawi!(zero: ..32);
    output.arb_umul_add_(&input_a, &input_b);

    let (mut op_dag, res) = OpDag::from_epoch(&epoch0);
    res.unwrap();

    let output = op_dag.note_pstate(&epoch0, output.state()).unwrap();
    let input_a = op_dag.note_pstate(&epoch0, input_a.state()).unwrap();
    let input_b = op_dag.note_pstate(&epoch0, input_b.state()).unwrap();

    op_dag.lower_all().unwrap();

    let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
    res.unwrap();

    t_dag.verify_integrity().unwrap();

    t_dag.eval_all().unwrap();

    t_dag.optimize_basic();

    {
        use awi::*;
        t_dag.set_noted(input_a, inlawi!(123u16).as_ref());
        t_dag.set_noted(input_b, inlawi!(77u16).as_ref());
        t_dag.eval_all().unwrap();
        std::assert_eq!(t_dag.get_noted_as_extawi(output).unwrap(), awi!(9471u32));

        t_dag.set_noted(input_a, inlawi!(10u16).as_ref());
        t_dag.eval_all().unwrap();
        std::assert_eq!(t_dag.get_noted_as_extawi(output).unwrap(), awi!(770u32));
    }
}
*/

// test LUT simplifications
#[test]
fn luts() {
    let mut rng = StarRng::new(0);
    let mut inp_bits = 0;
    for input_w in 1usize..=8 {
        let lut_w = 1 << input_w;
        for _ in 0..100 {
            let epoch0 = Epoch::new();
            let mut test_input = awi::Awi::zero(bw(input_w));
            rng.next_bits(&mut test_input);
            let original_input = test_input.clone();
            let mut input = LazyAwi::opaque(bw(input_w));
            let mut lut_input = dag::Awi::from(input.as_ref());
            let mut opaque_set = awi::Awi::umax(bw(input_w));
            for i in 0..input_w {
                // randomly set some bits to a constant and leave some as opaque
                if rng.next_bool() {
                    lut_input.set(i, test_input.get(i).unwrap()).unwrap();
                    opaque_set.set(i, false).unwrap();
                }
            }
            for _ in 0..input_w {
                if (rng.next_u8() % 8) == 0 {
                    let inx0 = (rng.next_u8() % (input_w as awi::u8)) as awi::usize;
                    let inx1 = (rng.next_u8() % (input_w as awi::u8)) as awi::usize;
                    if opaque_set.get(inx0).unwrap() && opaque_set.get(inx1).unwrap() {
                        // randomly make some inputs duplicates from the same source
                        let tmp = lut_input.get(inx0).unwrap();
                        lut_input.set(inx1, tmp).unwrap();
                        let tmp = test_input.get(inx0).unwrap();
                        test_input.set(inx1, tmp).unwrap();
                    }
                }
            }
            let mut lut = awi::Awi::zero(bw(lut_w));
            rng.next_bits(&mut lut);
            let mut x = awi!(0);
            x.lut_(&Awi::from(&lut), &lut_input).unwrap();

            {
                use awi::{assert, assert_eq, *};

                let mut opt_res = EvalAwi::from(&x);

                epoch0.optimize().unwrap();

                input.retro_(&original_input).unwrap();

                // check that the value is correct
                let opt_res = opt_res.eval().unwrap();
                let res = lut.get(test_input.to_usize()).unwrap();
                let res = Awi::from_bool(res);
                if opt_res != res {
                    /*
                    println!("{:0b}", &opaque_set);
                    println!("{:0b}", &test_input);
                    println!("{:0b}", &lut);
                    t_dag.render_to_svg_file(PathBuf::from("./rendered0.svg".to_owned())).unwrap();
                    */
                }
                assert_eq!(opt_res, res);

                let ensemble = epoch0.ensemble();

                // assert that there is at most one TNode with constant inputs optimized away
                let mut tnodes = ensemble.tnodes.vals();
                if let Some(tnode) = tnodes.next() {
                    inp_bits += tnode.inp.len();
                    assert!(tnode.inp.len() <= opaque_set.count_ones());
                    assert!(tnodes.next().is_none());
                }
                assert!(tnodes.next().is_none());
            }
        }
    }
    {
        use awi::assert_eq;
        assert_eq!(inp_bits, 1386);
    }
}
