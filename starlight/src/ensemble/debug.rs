use std::path::PathBuf;

use awint::{
    awint_dag::{EvalError, PNote, PState},
    awint_macro_internals::triple_arena::Arena,
};

use crate::{
    ensemble::{Ensemble, Equiv, PBack, Referent, TNode},
    triple_arena::{Advancer, ChainArena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
    Epoch,
};

#[derive(Debug, Clone)]
pub struct StateBit {
    p_state: PState,
    i: usize,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    StateBit(StateBit),
    TNode(TNode),
    Equiv(Equiv, Vec<PBack>),
    Note(PBack, PNote, u64),
    Remove,
}

impl DebugNodeTrait<PBack> for NodeKind {
    fn debug_node(p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            NodeKind::StateBit(state_bit) => DebugNode {
                sources: vec![],
                center: {
                    let mut v = vec![format!("{:?}", p_this)];
                    v.push(format!("{} {}", state_bit.p_state, state_bit.i));
                    v
                },
                sinks: vec![],
            },
            NodeKind::TNode(tnode) => DebugNode {
                sources: tnode
                    .inp
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, format!("{i}")))
                    .collect(),
                center: {
                    let mut v = vec![format!("{:?}", p_this)];
                    if let Some(ref lut) = tnode.lut {
                        v.push(format!("{:?} ", lut));
                    }
                    if let Some(driver) = tnode.loop_driver {
                        v.push(format!("driver: {:?}", driver));
                    }
                    v
                },
                sinks: vec![],
            },
            NodeKind::Equiv(equiv, p_tnodes) => DebugNode {
                sources: p_tnodes.iter().map(|p| (*p, String::new())).collect(),
                center: {
                    vec![
                        format!("{:?}", equiv.p_self_equiv),
                        format!("{:?}", equiv.val),
                    ]
                },
                sinks: vec![],
            },
            NodeKind::Note(p_back, p_note, inx) => DebugNode {
                sources: vec![(*p_back, String::new())],
                center: { vec![format!("{p_note} [{inx}]")] },
                sinks: vec![],
            },
            NodeKind::Remove => panic!("should have been removed"),
        }
    }
}

impl Ensemble {
    pub fn backrefs_to_chain_arena(&self) -> ChainArena<PBack, Referent> {
        let mut chain_arena = ChainArena::new();
        self.backrefs
            .clone_keys_to_chain_arena(&mut chain_arena, |_, p_tnode| *p_tnode);
        chain_arena
    }

    pub fn to_debug(&self) -> Arena<PBack, NodeKind> {
        let mut arena = Arena::<PBack, NodeKind>::new();
        self.backrefs
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                match referent {
                    Referent::ThisEquiv => {
                        let mut v = vec![];
                        let mut adv = self.backrefs.advancer_surject(p_self);
                        while let Some(p) = adv.advance(&self.backrefs) {
                            if let Referent::ThisTNode(_) = self.backrefs.get_key(p).unwrap() {
                                // get every TNode that is in this equivalence
                                v.push(p);
                            }
                        }
                        NodeKind::Equiv(self.backrefs.get_val(p_self).unwrap().clone(), v)
                    }
                    Referent::ThisStateBit(p, i) => {
                        NodeKind::StateBit(StateBit { p_state: *p, i: *i })
                    }
                    Referent::ThisTNode(p_tnode) => {
                        let mut tnode = self.tnodes.get(*p_tnode).unwrap().clone();
                        // forward to the `PBack`s of TNodes
                        for inp in &mut tnode.inp {
                            if let Referent::Input(_) = self.backrefs.get_key(*inp).unwrap() {
                                let p_input = self.backrefs.get_val(*inp).unwrap().p_self_equiv;
                                *inp = p_input;
                            }
                        }
                        if let Some(loop_driver) = tnode.loop_driver.as_mut() {
                            if let Referent::LoopDriver(_) =
                                self.backrefs.get_key(*loop_driver).unwrap()
                            {
                                let p_driver =
                                    self.backrefs.get_val(*loop_driver).unwrap().p_self_equiv;
                                *loop_driver = p_driver;
                            }
                        }
                        NodeKind::TNode(tnode)
                    }
                    Referent::Note(p_note) => {
                        let note = self.notes.get(*p_note).unwrap();
                        let mut inx = u64::MAX;
                        for (i, bit) in note.bits.iter().enumerate() {
                            if *bit == p_self {
                                inx = u64::try_from(i).unwrap();
                            }
                        }
                        let equiv = self.backrefs.get_val(p_self).unwrap();
                        NodeKind::Note(equiv.p_self_equiv, *p_note, inx)
                    }
                    _ => NodeKind::Remove,
                }
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let NodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_debug(), false, out_file).unwrap();
        res
    }
}

impl Epoch {
    pub fn eprint_debug_summary(&self) {
        let ensemble = self.clone_ensemble();
        let chain_arena = ensemble.backrefs_to_chain_arena();
        let debug = ensemble.to_debug();
        eprintln!(
            "ensemble: {:#?}\nchain_arena: {:#?}\ndebug: {:#?}",
            ensemble, chain_arena, debug
        );
    }

    pub fn render_to_svg_file(&self, out_file: PathBuf) -> Result<(), EvalError> {
        self.clone_ensemble().render_to_svg_file(out_file)
    }
}
