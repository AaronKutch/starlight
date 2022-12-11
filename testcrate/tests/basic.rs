use starlight::{
    awint_dag::{Lineage, OpDag},
    dag_prelude::*,
    PTNode, TDag,
};

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
        assert_eq!(i, t_dag.get_noted_as_extawi(p_val).to_usize());

        t_dag.drive_loops();
        t_dag.eval();
    }
}
