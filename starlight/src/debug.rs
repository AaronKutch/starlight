use std::path::PathBuf;

use awint::{awint_dag::EvalError, awint_macro_internals::triple_arena::Arena};

use crate::{
    triple_arena::{Advancer, ChainArena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
    Equiv, PBack, Referent, TDag, TNode,
};

#[derive(Debug, Clone)]
pub enum DebugTDag {
    TNode(TNode),
    Equiv(Equiv, Vec<PBack>),
    Remove,
}

impl DebugNodeTrait<PBack> for DebugTDag {
    fn debug_node(p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            DebugTDag::TNode(tnode) => DebugNode {
                sources: tnode
                    .inp
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, format!("{i}")))
                    .collect(),
                center: {
                    let mut v = vec![format!("{:?}", p_this)];
                    if let Some(ref lut) = tnode.lut {
                        v.push(format!("{:?}", lut));
                    }
                    v.push(format!("alg_rc:{} vis:{}", tnode.alg_rc, tnode.visit,));
                    if let Some(driver) = tnode.loop_driver {
                        v.push(format!("driver: {:?}", driver));
                    }
                    v
                },
                sinks: vec![],
            },
            DebugTDag::Equiv(equiv, p_tnodes) => DebugNode {
                sources: p_tnodes.iter().map(|p| (*p, String::new())).collect(),
                center: {
                    vec![
                        format!("{:?} {}", equiv.p_self_equiv, equiv.equiv_alg_rc),
                        format!("{:?}", equiv.val),
                    ]
                },
                sinks: vec![],
            },
            DebugTDag::Remove => panic!("should have been removed"),
        }
    }
}

impl TDag {
    pub fn backrefs_to_chain_arena(&self) -> ChainArena<PBack, Referent> {
        let mut chain_arena = ChainArena::new();
        self.backrefs
            .clone_keys_to_chain_arena(&mut chain_arena, |_, p_tnode| *p_tnode);
        chain_arena
    }

    pub fn to_debug_tdag(&self) -> Arena<PBack, DebugTDag> {
        let mut arena = Arena::<PBack, DebugTDag>::new();
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
                        DebugTDag::Equiv(self.backrefs.get_val(p_self).unwrap().clone(), v)
                    }
                    Referent::ThisTNode(p_tnode) => {
                        let mut tnode = self.tnodes.get(*p_tnode).unwrap().clone();
                        // forward to the `PBack`s of TNodes
                        for inp in &mut tnode.inp {
                            if let Referent::Input(p_input) = self.backrefs.get_key(*inp).unwrap() {
                                *inp = self.tnodes.get(*p_input).unwrap().p_self;
                            }
                        }
                        if let Some(loop_driver) = tnode.loop_driver.as_mut() {
                            if let Referent::LoopDriver(p_driver) =
                                self.backrefs.get_key(*loop_driver).unwrap()
                            {
                                *loop_driver = self.tnodes.get(*p_driver).unwrap().p_self;
                            }
                        }
                        DebugTDag::TNode(tnode)
                    }
                    _ => DebugTDag::Remove,
                }
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let DebugTDag::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_debug_tdag(), false, out_file).unwrap();
        res
    }
}
