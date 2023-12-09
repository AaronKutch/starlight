use starlight::{awi, dag::*, Epoch, EvalAwi, Loop};

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

        let eval_x = EvalAwi::from(&x);
        assert_eq!(eval_x.eval().unwrap(), awi!(1));
        epoch0.drive_loops().unwrap();
        assert_eq!(eval_x.eval().unwrap(), awi!(0));
        epoch0.drive_loops().unwrap();
        assert_eq!(eval_x.eval().unwrap(), awi!(1));
    }
    drop(epoch0);
}

/*
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
*/
