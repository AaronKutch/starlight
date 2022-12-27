use starlight::{
    awi,
    awint_dag::{Lineage, OpDag},
    dag::*,
    PTNode, TDag,
};

// tests an incrementing counter
#[test]
fn incrementer() {
    let looper = Loop::zero(bw(4));
    let val = ExtAwi::from(looper.as_ref());
    let mut tmp = ExtAwi::from(looper.as_ref());
    tmp.inc_(true);
    let handle = looper.drive(&tmp).unwrap();

    let leaves = vec![handle.state(), val.state()];

    let noted = leaves.clone();

    // TODO how we handle noted things in the future, is that we have an external
    // arena type that states can be put into (and the user can get a unified
    // pointer from for use past the TDag stage), and that gets passed to
    // `OpDag::new`

    // TODO also have a single function for taking `Lineage` capable structs
    // straight to `TDag`s

    let (mut op_dag, res) = OpDag::new(&leaves, &noted);
    op_dag.lower_all_noted().unwrap();
    res.unwrap();

    let (mut t_dag, res) = TDag::<PTNode>::from_op_dag_using_noted(&mut op_dag);

    let notes = res.unwrap();
    let p_val = notes[1];

    t_dag.basic_simplify();

    for i in 0..16 {
        std::assert_eq!(i, t_dag.get_noted_as_extawi(p_val).to_usize());

        t_dag.drive_loops();
        t_dag.eval();
    }
}

// tests getting and setting outputs
#[test]
fn multiplier() {
    let input_a = inlawi!(opaque: ..16);
    let input_b = inlawi!(opaque: ..16);
    let mut output = inlawi!(zero: ..32);
    output.arb_umul_add_(&input_a, &input_b);

    let leaves = vec![output.state()];

    let mut noted = leaves.clone();
    noted.push(input_a.state());
    noted.push(input_b.state());

    let (mut op_dag, res) = OpDag::new(&leaves, &noted);
    op_dag.lower_all_noted().unwrap();
    res.unwrap();

    let (mut t_dag, res) = TDag::<PTNode>::from_op_dag_using_noted(&mut op_dag);

    let notes = res.unwrap();
    t_dag.basic_simplify();
    let output = notes[0];
    let input_a = notes[1];
    let input_b = notes[2];

    {
        use awi::*;
        t_dag.set_noted(input_a, inlawi!(123u16).as_ref());
        t_dag.set_noted(input_b, inlawi!(77u16).as_ref());
        t_dag.eval();
        std::assert_eq!(t_dag.get_noted_as_extawi(output), extawi!(9471u32));

        t_dag.set_noted(input_a, inlawi!(10u16).as_ref());
        t_dag.eval();
        std::assert_eq!(t_dag.get_noted_as_extawi(output), extawi!(770u32));
    }
}
