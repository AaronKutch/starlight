use std::path::PathBuf;

use starlight::{
    awi,
    awint_dag::{basic_state_epoch::StateEpoch, EvalError, Lineage, OpDag},
    dag::*,
    StarRng, TDag,
};

// keep imports imported
fn _dbg(t_dag: &mut TDag) -> awi::Result<(), EvalError> {
    t_dag.render_to_svg_file(PathBuf::from("./t_dag.svg".to_owned()))
}

#[test]
fn invert_twice() {
    let epoch0 = StateEpoch::new();
    let x = awi!(opaque: ..1);
    let mut y = x.clone();
    y.not_();
    let y_copy = y.clone();
    y.lut_(&inlawi!(10), &y_copy).unwrap();
    y.not_();

    // TODO also have a single function for taking `Lineage` capable structs
    // straight to `TDag`s

    let (mut op_dag, res) = OpDag::from_epoch(&epoch0);
    res.unwrap();

    let p_x = op_dag.note_pstate(&epoch0, x.state()).unwrap();
    let p_y = op_dag.note_pstate(&epoch0, y.state()).unwrap();

    op_dag.lower_all().unwrap();

    let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
    res.unwrap();

    t_dag.verify_integrity().unwrap();

    t_dag.optimize_basic();

    t_dag.verify_integrity().unwrap();

    {
        use awi::{assert_eq, *};

        t_dag.set_noted(p_x, &inlawi!(1)).unwrap();
        t_dag.eval_all().unwrap();
        assert_eq!(t_dag.get_noted_as_extawi(p_y).unwrap(), awi!(1));
        t_dag.set_noted(p_x, &inlawi!(0)).unwrap();
        t_dag.eval_all().unwrap();
        assert_eq!(t_dag.get_noted_as_extawi(p_y).unwrap(), awi!(0));
    }
}

#[test]
fn invert_in_loop() {
    let epoch0 = StateEpoch::new();
    let looper = Loop::zero(bw(1));
    let mut x = awi!(looper);
    let x_copy = x.clone();
    x.lut_(&inlawi!(10), &x_copy).unwrap();
    x.not_();
    let x_copy = x.clone();
    x.lut_(&inlawi!(10), &x_copy).unwrap();
    looper.drive(&x).unwrap();

    let (mut op_dag, res) = OpDag::from_epoch(&epoch0);
    res.unwrap();

    let p_x = op_dag.note_pstate(&epoch0, x.state()).unwrap();

    op_dag.lower_all().unwrap();
    op_dag.delete_unused_nodes();

    let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
    res.unwrap();

    t_dag.verify_integrity().unwrap();

    t_dag.optimize_basic();

    t_dag.verify_integrity().unwrap();

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

// test LUT simplifications
#[test]
fn luts() {
    let mut rng = StarRng::new(0);
    let mut inp_bits = 0;
    for input_w in 1usize..=8 {
        let lut_w = 1 << input_w;
        for _ in 0..100 {
            let epoch0 = StateEpoch::new();
            let mut test_input = awi::Awi::zero(bw(input_w));
            rng.next_bits(&mut test_input);
            let original_input = test_input.clone();
            let mut input = Awi::opaque(bw(input_w));
            let input_state = input.state();
            let mut opaque_set = awi::Awi::umax(bw(input_w));
            for i in 0..input_w {
                // randomly set some bits to a constant and leave some as opaque
                if rng.next_bool() {
                    input.set(i, test_input.get(i).unwrap()).unwrap();
                    opaque_set.set(i, false).unwrap();
                }
            }
            for _ in 0..input_w {
                if (rng.next_u8() % 8) == 0 {
                    let inx0 = (rng.next_u8() % (input_w as awi::u8)) as awi::usize;
                    let inx1 = (rng.next_u8() % (input_w as awi::u8)) as awi::usize;
                    if opaque_set.get(inx0).unwrap() && opaque_set.get(inx1).unwrap() {
                        // randomly make some inputs duplicates from the same source
                        let tmp = input.get(inx0).unwrap();
                        input.set(inx1, tmp).unwrap();
                        let tmp = test_input.get(inx0).unwrap();
                        test_input.set(inx1, tmp).unwrap();
                    }
                }
            }
            let mut lut = awi::Awi::zero(bw(lut_w));
            rng.next_bits(&mut lut);
            let mut x = Awi::zero(bw(1));
            x.lut_(&Awi::from(&lut), &input).unwrap();

            let (mut op_dag, res) = OpDag::from_epoch(&epoch0);
            res.unwrap();

            let p_x = op_dag.note_pstate(&epoch0, x.state()).unwrap();
            let p_input = op_dag.note_pstate(&epoch0, input_state).unwrap();

            op_dag.lower_all().unwrap();

            let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
            res.unwrap();

            t_dag.optimize_basic();

            {
                use awi::{assert, assert_eq, *};
                // assert that there is at most one TNode with constant inputs optimized away
                let mut tnodes = t_dag.tnodes.vals();
                if let Some(tnode) = tnodes.next() {
                    inp_bits += tnode.inp.len();
                    assert!(tnode.inp.len() <= opaque_set.count_ones());
                    assert!(tnodes.next().is_none());
                }

                t_dag.set_noted(p_input, &original_input).unwrap();

                t_dag.eval_all().unwrap();

                // check that the value is correct
                let opt_res = t_dag.get_noted_as_extawi(p_x).unwrap();
                assert_eq!(opt_res.bw(), 1);
                let opt_res = opt_res.to_bool();
                let res = lut.get(test_input.to_usize()).unwrap();
                if opt_res != res {
                    /*
                    //dbg!(&t_dag);
                    println!("{:0b}", &opaque_set);
                    println!("{:0b}", &test_input);
                    println!("{:0b}", &lut);
                    t_dag.render_to_svg_file(PathBuf::from("./rendered0.svg".to_owned())).unwrap();
                    */
                }
                assert_eq!(opt_res, res);
            }
        }
    }
    {
        use awi::assert_eq;
        assert_eq!(inp_bits, 1386);
    }
}
