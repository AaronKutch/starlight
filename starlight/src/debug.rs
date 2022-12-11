use std::path::PathBuf;

use awint::awint_dag::EvalError;

use crate::{
    triple_arena::Ptr,
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
    TDag, TNode,
};

#[cfg(not(feature = "debug_min"))]
impl<P: Ptr> DebugNodeTrait<P> for TNode<P> {
    fn debug_node(p_this: P, this: &Self) -> DebugNode<P> {
        DebugNode {
            sources: this.inp.iter().map(|p| (*p, String::new())).collect(),
            center: {
                let mut v = vec![format!("{:?}", p_this)];
                if let Some(ref lut) = this.lut {
                    v.push(format!("{:?}", lut));
                }
                v.push(format!(
                    "a_rc:{} rc:{} vis:{}",
                    this.alg_rc, this.rc, this.visit,
                ));
                v.push(format!(
                    "{} {}",
                    match this.val {
                        None => "*",
                        Some(false) => "0",
                        Some(true) => "1",
                    },
                    if this.is_permanent { "(perm)" } else { "" }
                ));
                if let Some(driver) = this.loop_driver {
                    v.push(format!("driver:{:?}", driver));
                }
                v
            },
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
                match this.val {
                    None => (),
                    Some(false) => v.push("0".to_string()),
                    Some(true) => v.push("1".to_string()),
                }
                if let Some(driver) = this.loop_driver {
                    v.push(format!("->{:?}", driver));
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
        render_to_svg_file(&self.a, false, out_file).unwrap();
        res
    }
}
