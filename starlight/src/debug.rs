use std::{collections::HashMap, path::PathBuf};

use awint::awint_dag::common::EvalError;
use triple_arena::{ptr_trait_struct_with_gen, Arena, Ptr, PtrTrait};
use triple_arena_render::{DebugNode, DebugNodeTrait};

use crate::{chain_arena::Link, BitState, Lut, PermDag};

#[derive(Debug)]
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
        ptr_trait_struct_with_gen!(Q);
        let mut a = Arena::<Q, BitOrLut<Q>>::new();
        let mut lut_map = HashMap::<Ptr<PLut>, Ptr<Q>>::new();
        for (p_lut, lut) in &self.luts {
            lut_map.insert(
                p_lut,
                a.insert(BitOrLut::Lut(vec![], Lut {
                    bits: vec![],
                    perm: lut.perm.clone(),
                    visit_num: lut.visit_num,
                })),
            );
        }
        let mut bit_map = HashMap::<Ptr<PBitState>, Ptr<Q>>::new();
        for (p_bit, bit) in self.bits.get_arena() {
            if let Some(lut) = bit.t.lut {
                // point to a LUT node
                let lut = lut_map[&lut];
                bit_map.insert(
                    p_bit,
                    a.insert(BitOrLut::Bit(Some(lut), BitState {
                        lut: Some(lut),
                        state: bit.t.state,
                    })),
                );
            } else {
                // point to another bit, register later
                bit_map.insert(
                    p_bit,
                    a.insert(BitOrLut::Bit(None, BitState {
                        lut: None,
                        state: bit.t.state,
                    })),
                );
            };
        }
        // second pass on bits to register direct bit connections
        for (p_bit, bit) in self.bits.get_arena() {
            if let BitOrLut::Bit(ref mut p, _) = &mut a[bit_map[&p_bit]] {
                if p.is_none() {
                    if let Some(prev) = Link::prev(bit) {
                        *p = Some(bit_map[&prev])
                    }
                }
            }
        }
        // second pass on luts to register bit connections
        for (p_lut, lut) in &self.luts {
            match &mut a[lut_map[&p_lut]] {
                BitOrLut::Lut(ref mut inxs, _) => {
                    for bit in &lut.bits {
                        // only if the bit
                        if Link::prev(&self.bits[bit]).is_none() {}
                        inxs.push(bit_map.get(bit).copied());
                    }
                }
                _ => unreachable!(),
            }
        }
        dbg!(&a);
        let res = self.verify_integrity();
        triple_arena_render::render_to_svg_file(&a, false, out_file).unwrap();
        res
    }
}
