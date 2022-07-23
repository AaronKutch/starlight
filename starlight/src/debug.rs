use std::{collections::HashMap, path::PathBuf};

use awint::awint_dag::common::EvalError;
use triple_arena::{Arena, Ptr, PtrTrait};
use triple_arena_render::{DebugNode, DebugNodeTrait};

use crate::{BitState, Lut, PermDag};

enum BitOrLut<P: PtrTrait> {
    Bit(Option<Ptr<P>>, BitState<P>),
    Lut(Vec<Option<Ptr<P>>>, Lut<P>),
}

impl<P: PtrTrait> DebugNodeTrait<P> for BitOrLut<P> {
    fn debug_node(this: &Self) -> DebugNode<P> {
        match this {
            BitOrLut::Bit(prev, t) => DebugNode {
                sources: if let Some(prev) = prev {
                    vec![(*prev, String::new())]
                } else {
                    vec![]
                },
                center: vec![format!("{:?}", t)],
                sinks: vec![],
            },
            BitOrLut::Lut(v, lut) => DebugNode {
                sources: v
                    .iter()
                    .map(|p| {
                        if let Some(p) = p {
                            (*p, String::new())
                        } else {
                            (Ptr::invalid(), String::new())
                        }
                    })
                    .collect(),
                center: vec![format!("{:?}", lut)],
                sinks: vec![],
            },
        }
    }
}

impl<PBitState: PtrTrait, PLut: PtrTrait> PermDag<PBitState, PLut> {
    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let mut a = Arena::<PBitState, BitOrLut<PBitState>>::new();
        let mut lut_map = HashMap::<Ptr<PLut>, Ptr<PBitState>>::new();
        for (p, lut) in &self.luts {
            lut_map.insert(
                p,
                a.insert(BitOrLut::Lut(vec![], Lut {
                    bits: vec![],
                    perm: lut.perm.clone(),
                    visit_num: lut.visit_num,
                })),
            );
        }
        let mut bit_map = HashMap::<Ptr<PBitState>, Ptr<PBitState>>::new();
        for (p, bit) in self.bits.get_arena() {
            let lut = if let Some(lut) = bit.t.lut {
                lut_map.get(&lut).copied()
            } else {
                None
            };
            bit_map.insert(
                p,
                a.insert(BitOrLut::Bit(bit.prev, BitState {
                    lut,
                    state: bit.t.state,
                })),
            );
        }
        // second pass to register lut connections
        for (p, lut) in &self.luts {
            match &mut a[lut_map[&p]] {
                BitOrLut::Lut(ref mut inxs, _) => {
                    for bit in &lut.bits {
                        inxs.push(bit_map.get(bit).copied());
                    }
                }
                _ => unreachable!(),
            }
        }
        let res = self.verify_integrity();
        triple_arena_render::render_to_svg_file(&a, false, out_file).unwrap();
        res
    }
}
