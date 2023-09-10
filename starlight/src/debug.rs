use std::{num::NonZeroU64, path::PathBuf};

use awint::{
    awint_dag::{smallvec::SmallVec, EvalError},
    awint_macro_internals::triple_arena::Arena,
    ExtAwi,
};

use crate::{
    triple_arena::{ChainArena, Ptr},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
    PBack, PTNode, Referent, TDag, TNode, Value,
};

/// This is a separate struct so that all `PBack`s can be replaced with
/// `PTNode`s
#[derive(Debug, Clone)]
pub struct DebugTNode {
    pub p_back_self: PTNode,
    pub inp: SmallVec<[PTNode; 4]>,
    pub lut: Option<ExtAwi>,
    pub val: Option<Value>,
    pub loop_driver: Option<PTNode>,
    pub alg_rc: u64,
    pub visit: NonZeroU64,
}

impl DebugTNode {
    pub fn from_tnode(tnode: &TNode, tdag: &TDag) -> Self {
        Self {
            p_back_self: tdag.get_p_tnode(tnode.p_self).unwrap_or(Ptr::invalid()),
            inp: tnode
                .inp
                .iter()
                .map(|p| tdag.get_p_tnode(*p).unwrap_or(Ptr::invalid()))
                .collect(),
            lut: tnode.lut.clone(),
            val: tdag.get_val(tnode.p_self),
            loop_driver: tnode
                .loop_driver
                .map(|p| tdag.get_p_tnode(p).unwrap_or(Ptr::invalid())),
            alg_rc: tnode.alg_rc,
            visit: tnode.visit,
        }
    }
}

#[cfg(not(feature = "debug_min"))]
impl DebugNodeTrait<PTNode> for DebugTNode {
    fn debug_node(p_this: PTNode, this: &Self) -> DebugNode<PTNode> {
        DebugNode {
            sources: this
                .inp
                .iter()
                .enumerate()
                .map(|(i, p)| (*p, format!("{i}")))
                .collect(),
            center: {
                let mut v = vec![format!("{:?}", p_this)];
                if let Some(ref lut) = this.lut {
                    v.push(format!("{:?}", lut));
                }
                v.push(format!("alg_rc:{} vis:{}", this.alg_rc, this.visit,));
                v.push(match this.val {
                    None => "invalid p_self".to_owned(),
                    Some(val) => format!("{val:?}"),
                });
                if let Some(driver) = this.loop_driver {
                    v.push(format!("driver: {:?}", driver));
                }
                v
            },
            sinks: vec![],
        }
    }
}

#[cfg(feature = "debug_min")]
impl DebugNodeTrait<PTNode> for DebugTNode {
    fn debug_node(_p_this: PTNode, this: &Self) -> DebugNode<PTNode> {
        DebugNode {
            sources: this.inp.iter().map(|p| (*p, String::new())).collect(),
            center: {
                let mut v = vec![];
                if let Some(ref lut) = this.lut {
                    v.push(format!("{lut:?}"));
                }
                v.push(match this.val {
                    None => "invalid p_self".to_owned(),
                    Some(val) => format!("{val:?}"),
                });
                if let Some(driver) = this.loop_driver {
                    v.push(format!("->{driver:?}"));
                }
                v
            },
            sinks: vec![],
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

    pub fn to_debug_tdag(&self) -> Arena<PTNode, DebugTNode> {
        let mut arena = Arena::<PTNode, DebugTNode>::new();
        arena.clone_from_with(&self.tnodes, |_, tnode| DebugTNode::from_tnode(tnode, self));
        arena
    }

    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_debug_tdag(), false, out_file).unwrap();
        res
    }
}
