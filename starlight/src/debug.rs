use std::path::PathBuf;

use awint::awint_dag::EvalError;
use triple_arena::Ptr;
use triple_arena_render::{DebugNode, DebugNodeTrait};

use crate::{TDag, TNode};

#[cfg(not(feature = "debug_min"))]
impl<P: Ptr> DebugNodeTrait<P> for TNode<P> {
    fn debug_node(p_this: P, this: &Self) -> DebugNode<P> {
        DebugNode {
            sources: this.inp.iter().map(|p| (*p, String::new())).collect(),
            center: vec![
                format!("{:?}", p_this),
                format!("{:?}", this.lut),
                format!(
                    "{} {} {} {}",
                    match this.val {
                        None => "*",
                        Some(false) => "0",
                        Some(true) => "1",
                    },
                    this.alg_rc,
                    this.rc,
                    this.visit,
                ),
                format!("{:?}", this.loopback),
            ],
            sinks: vec![],
        }
    }
}

#[cfg(feature = "debug_min")]
impl<P: Ptr> DebugNodeTrait<P> for TNode<P> {
    fn debug_node(_p_this: P, this: &Self) -> DebugNode<P> {
        DebugNode {
            sources: this.inp.iter().map(|p| (*p, String::new())).collect(),
            center: {
                let mut v = vec![];
                if let Some(ref lut) = this.lut {
                    v.push(format!("{:?}", lut));
                }
                if let Some(loopback) = this.loopback {
                    v.push(format!("->{:?}", loopback));
                }
                v
            },
            sinks: vec![],
        }
    }
}

impl<P: Ptr> TDag<P> {
    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let res = self.verify_integrity();
        triple_arena_render::render_to_svg_file(&self.a, false, out_file).unwrap();
        res
    }
}
