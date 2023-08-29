use starlight::{
    awi,
    awint_dag::{Lineage, OpDag, StateEpoch},
    dag::*,
    TDag,
};

// tests an incrementing counter
#[test]
fn incrementer() {
    let epoch0 = StateEpoch::new();
    let looper = Loop::zero(bw(4));
    let val = ExtAwi::from(looper.as_ref());
    let mut tmp = ExtAwi::from(looper.as_ref());
    tmp.inc_(true);
    looper.drive(&tmp).unwrap();

    // TODO also have a single function for taking `Lineage` capable structs
    // straight to `TDag`s

    let (mut op_dag, res) = OpDag::from_epoch(&epoch0);
    res.unwrap();

    let p_val = op_dag.note_pstate(val.state()).unwrap();

    op_dag.lower_all().unwrap();

    let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
    res.unwrap();

    t_dag.verify_integrity().unwrap();

    // TODO
    t_dag.eval();
    //t_dag.basic_simplify();

    for i in 0..16 {
        std::assert_eq!(i, t_dag.get_noted_as_extawi(p_val).unwrap().to_usize());

        t_dag.drive_loops();
        t_dag.eval();
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

    let output = op_dag.note_pstate(output.state()).unwrap();
    let input_a = op_dag.note_pstate(input_a.state()).unwrap();
    let input_b = op_dag.note_pstate(input_b.state()).unwrap();

    op_dag.lower_all().unwrap();

    let (mut t_dag, res) = TDag::from_op_dag(&mut op_dag);
    res.unwrap();

    t_dag.verify_integrity().unwrap();

    // TODO
    t_dag.eval();
    //t_dag.basic_simplify();

    {
        use awi::*;
        t_dag.set_noted(input_a, inlawi!(123u16).as_ref());
        t_dag.set_noted(input_b, inlawi!(77u16).as_ref());
        t_dag.eval();
        std::assert_eq!(t_dag.get_noted_as_extawi(output).unwrap(), extawi!(9471u32));

        t_dag.set_noted(input_a, inlawi!(10u16).as_ref());
        t_dag.eval();
        std::assert_eq!(t_dag.get_noted_as_extawi(output).unwrap(), extawi!(770u32));
    }
}
