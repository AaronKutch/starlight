use std::{collections::HashMap, path::PathBuf};

use awint::awint_dag::common::EvalError;
use triple_arena::{ptr_struct, Arena, Ptr};
use triple_arena_render::{DebugNode, DebugNodeTrait};

use crate::{
    chain_arena::{ChainArena, Link},
    BitState, Lut, PermDag,
};

#[derive(Debug)]
enum BitOrLut<P: Ptr> {
    // the Option is for direct bit connections when a bit does not have a LUT
    Bit(Option<P>, String, BitState<P>),
    // the LUT has most connections to preserve ordering in both inputs and outputs
    Lut(Vec<Option<P>>, Vec<Option<P>>, Lut<P>),
    // this is for preserving the ordering of the inputs and outputs of the LUTs
    Dummy,
}

impl<P: Ptr> DebugNodeTrait<P> for BitOrLut<P> {
    fn debug_node(this: &Self) -> DebugNode<P> {
        match this {
            BitOrLut::Bit(prev, s, t) => DebugNode {
                sources: if let Some(prev) = prev {
                    vec![(*prev, String::new())]
                } else {
                    vec![]
                },
                center: if s.is_empty() {
                    vec![format!("{:?}", t)]
                } else {
                    vec![format!("{:?}", t), s.clone()]
                },
                sinks: vec![],
            },
            BitOrLut::Lut(prevs, nexts, lut) => DebugNode {
                sources: prevs
                    .iter()
                    .map(|p| {
                        if let Some(p) = p {
                            (*p, String::new())
                        } else {
                            (Ptr::invalid(), String::new())
                        }
                    })
                    .collect(),
                center: lut
                    .perm
                    .to_string_table()
                    .lines()
                    .map(|s| s.to_owned())
                    .collect(),
                sinks: nexts
                    .iter()
                    .map(|p| {
                        if let Some(p) = p {
                            (*p, String::new())
                        } else {
                            (Ptr::invalid(), String::new())
                        }
                    })
                    .collect(),
            },
            BitOrLut::Dummy => DebugNode {
                sources: vec![],
                center: vec![],
                sinks: vec![],
            },
        }
    }
}

impl<PBitState: Ptr, PLut: Ptr> PermDag<PBitState, PLut> {
    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        ptr_struct!(Q);
        ChainArena::_check_invariants(&self.bits).unwrap();
        let mut a = Arena::<Q, BitOrLut<Q>>::new();
        let mut lut_map = HashMap::<PLut, Q>::new();
        for (p_lut, lut) in &self.luts {
            lut_map.insert(
                p_lut,
                a.insert(BitOrLut::Lut(vec![], vec![], Lut {
                    bits: vec![],
                    perm: lut.perm.clone(),
                    visit_num: lut.visit_num,
                })),
            );
        }
        let mut bit_map = HashMap::<PBitState, Q>::new();
        for (p_bit, bit) in self.bits.get_arena() {
            bit_map.insert(
                p_bit,
                a.insert(BitOrLut::Bit(None, format!("{:?}", p_bit), BitState {
                    lut: bit.t.lut.map(|lut| *lut_map.get(&lut).unwrap()),
                    state: bit.t.state,
                })),
            );
        }
        // register luts to their bits
        for (p_lut, lut) in &self.luts {
            let p_lut = lut_map[&p_lut];
            if let BitOrLut::Lut(ref mut prevs, ..) = &mut a[p_lut] {
                // push in reverse order
                for bit in lut.bits.iter().rev() {
                    prevs.push(bit_map.get(bit).copied());
                }
            }
            for bit in lut.bits.iter().rev() {
                if let Some(next) = Link::next(&self.bits[bit]) {
                    if let BitOrLut::Lut(_, ref mut nexts, _) = a[p_lut] {
                        nexts.push(bit_map.get(&next).copied());
                    }
                } else {
                    // need to preserve spots
                    let dummy = a.insert(BitOrLut::Dummy);
                    if let BitOrLut::Lut(_, ref mut nexts, _) = a[p_lut] {
                        nexts.push(Some(dummy));
                    }
                }
            }
        }
        for p_bit in self.bits.get_arena().ptrs() {
            if let Some(prev) = Link::prev(&self.bits[p_bit]) {
                if self.bits[prev].t.lut.is_none() {
                    // direct connect
                    if let BitOrLut::Bit(ref mut p, ..) = a[bit_map[&p_bit]] {
                        *p = Some(bit_map[&prev]);
                    }
                }
            }
        }
        let res = self.verify_integrity();
        triple_arena_render::render_to_svg_file(&a, false, out_file).unwrap();
        res
    }
}
