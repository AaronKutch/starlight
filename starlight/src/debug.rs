use std::path::PathBuf;

use awint::{awint_dag::EvalError, awint_macro_internals::triple_arena::Arena};

use crate::{
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
    PTNode, TDag, TNode,
};

#[cfg(not(feature = "debug_min"))]
impl DebugNodeTrait<PTNode> for TNode {
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
                v.push(format!("{}", match this.val {
                    None => "*",
                    Some(false) => "0",
                    Some(true) => "1",
                },));
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
impl DebugNodeTrait<PTNode> for TNode {
    fn debug_node(_p_this: PTNode, this: &Self) -> DebugNode<PTNode> {
        DebugNode {
            sources: this.inp.iter().map(|p| (*p, String::new())).collect(),
            center: {
                let mut v = vec![];
                if let Some(ref lut) = this.lut {
                    v.push(format!("{lut:?}"));
                }
                match this.val {
                    None => (),
                    Some(false) => v.push("0".to_string()),
                    Some(true) => v.push("1".to_string()),
                }
                if let Some(driver) = this.loop_driver {
                    v.push(format!("->{driver:?}"));
                }
                v
            },
            sinks: vec![],
        }
    }
}

enum BackRefOrTNode {
    BackRef(PTNode, PTNode),
    ExtraRef(PTNode, PTNode),
    TNode(TNode),
}

impl DebugNodeTrait<PTNode> for BackRefOrTNode {
    fn debug_node(_p_this: PTNode, this: &Self) -> DebugNode<PTNode> {
        match this {
            BackRefOrTNode::BackRef(p_this, p_val) => DebugNode {
                sources: vec![(*p_val, "p_val".to_owned())],
                center: vec![format!("{p_this}")],
                sinks: vec![],
            },
            BackRefOrTNode::TNode(tnode) => DebugNodeTrait::debug_node(_p_this, tnode),
            BackRefOrTNode::ExtraRef(p_this, p_val) => DebugNode {
                sources: vec![(*p_val, "p_val".to_owned())],
                center: vec![format!("{p_this}"), "extra".to_owned()],
                sinks: vec![],
            },
        }
    }
}

impl TDag {
    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let res = self.verify_integrity();
        let mut arena = Arena::<PTNode, BackRefOrTNode>::new();
        self.a.clone_keys_to_arena(&mut arena, |p_this, k| {
            if p_this == *k {
                let p_node = self.a.get_val(p_this).unwrap().p_self;
                if p_this == p_node {
                    BackRefOrTNode::TNode(self.a.get_val(p_this).unwrap().clone())
                } else {
                    BackRefOrTNode::ExtraRef(p_this, p_node)
                }
            } else {
                BackRefOrTNode::BackRef(p_this, self.a.get_val(p_this).unwrap().p_self)
            }
        });
        render_to_svg_file(&arena, false, out_file).unwrap();
        res
    }
}
