use std::path::PathBuf;

use awint::awint_dag::{
    triple_arena::{Advancer, Arena, ChainArena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
};

use super::{channel::Referent, Channeler};
use crate::{
    ensemble,
    route::{CNode, PBack, PCEdge},
    Error,
};

#[derive(Debug, Clone)]
pub enum NodeKind {
    CNode(CNode, Vec<PBack>, Vec<PBack>),
    SubNode(PBack),
    SuperNode(PBack),
    CEdgeIncidence(PCEdge, usize, bool),
    EnsembleBackRef(ensemble::PBack),
    Remove,
}

impl DebugNodeTrait<PBack> for NodeKind {
    fn debug_node(p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            NodeKind::CNode(..) => DebugNode {
                sources: vec![],
                center: { todo!() },
                sinks: vec![],
            },
            NodeKind::SubNode(_) => todo!(),
            NodeKind::SuperNode(_) => todo!(),
            NodeKind::CEdgeIncidence(..) => todo!(),
            NodeKind::EnsembleBackRef(_) => todo!(),
            NodeKind::Remove => panic!("should have been removed"),
        }
    }
}

impl Channeler {
    pub fn to_debug(&self) -> Arena<PBack, NodeKind> {
        let mut arena = Arena::<PBack, NodeKind>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| match referent {
                Referent::ThisCNode => todo!(),
                Referent::SubNode(_) => todo!(),
                Referent::SuperNode(_) => todo!(),
                Referent::CEdgeIncidence(..) => todo!(),
                Referent::EnsembleBackRef(_) => todo!(),
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let NodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn render_to_svgs_in_dir(&self, out_file: PathBuf) -> Result<(), Error> {
        let dir = match out_file.canonicalize() {
            Ok(o) => {
                if !o.is_dir() {
                    return Err(Error::OtherStr("need a directory not a file"));
                }
                o
            }
            Err(e) => {
                return Err(Error::OtherString(format!("{e:?}")));
            }
        };
        let mut ensemble_file = dir.clone();
        ensemble_file.push("ensemble.svg");
        let mut state_file = dir;
        state_file.push("states.svg");
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_debug(), false, ensemble_file).unwrap();
        res
    }

    pub fn backrefs_to_chain_arena(&self) -> ChainArena<PBack, Referent> {
        let mut chain_arena = ChainArena::new();
        self.cnodes
            .clone_keys_to_chain_arena(&mut chain_arena, |_, p_lnode| *p_lnode);
        chain_arena
    }

    pub fn eprint_debug_summary(&self) {
        let chain_arena = self.backrefs_to_chain_arena();
        eprintln!("chain_arena: {:#?}", chain_arena);
    }
}
