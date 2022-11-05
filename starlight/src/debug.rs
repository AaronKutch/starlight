use std::path::PathBuf;

use awint::awint_dag::EvalError;
use triple_arena::Ptr;
use triple_arena_render::{DebugNode, DebugNodeTrait};

use crate::{TDag, TNode};

impl<P: Ptr> DebugNodeTrait<P> for TNode<P> {
    fn debug_node(this: &Self) -> DebugNode<P> {
        DebugNode {
            sources: this.inp.iter().map(|p| (*p, String::new())).collect(),
            center: vec![format!("{:?}", this)],
            sinks: this.inp.iter().map(|p| (*p, String::new())).collect(),
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
