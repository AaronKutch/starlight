#![allow(clippy::redundant_closure)]

use std::{collections::HashMap, path::PathBuf};

use awint::awint_dag::EvalError;
use triple_arena::{ptr_struct, Arena, ChainArena, Link, Ptr};
use triple_arena_render::{DebugNode, DebugNodeTrait};

use crate::{Bit, Lut, PBit, PLut, TDag};

#[derive(Debug)]
enum BitOrLut<P: Ptr> {
    // the Option is for direct bit connections when a bit does not have a LUT
    Bit(Option<P>, String, Bit),
    // the LUT has most connections to preserve ordering in both inputs and outputs
    Lut(Vec<P>, Vec<P>, Lut, String),
    // when a bit with a `Lut` has a state but no previous bit
    Dummy(Bit),
}

impl<P: Ptr> DebugNodeTrait<P> for BitOrLut<P> {
    fn debug_node(this: &Self) -> DebugNode<P> {
        match this {
            BitOrLut::Bit(next, s, t) => DebugNode {
                sources: vec![],
                center: if s.is_empty() {
                    vec![format!("{:?}", t)]
                } else {
                    vec![format!("{:?}", t), s.clone()]
                },
                sinks: if let Some(next) = next {
                    vec![(*next, String::new())]
                } else {
                    vec![]
                },
            },
            BitOrLut::Lut(prevs, nexts, lut, s) => DebugNode {
                sources: prevs.iter().map(|p| (*p, String::new())).collect(),
                center: {
                    let mut v: Vec<String> = lut
                        .perm
                        .to_string_table()
                        .lines()
                        .map(|s| s.to_owned())
                        .collect();
                    v.push(s.clone());
                    v
                },
                sinks: nexts.iter().map(|p| (*p, String::new())).collect(),
            },
            BitOrLut::Dummy(bit) => DebugNode {
                sources: vec![],
                center: vec![format!("{:?}", bit)],
                sinks: vec![],
            },
        }
    }
}

impl TDag {
    pub fn render_to_svg_file(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        ptr_struct!(Q);
        ChainArena::_check_invariants(&self.bits).unwrap();
        let mut a = Arena::<Q, BitOrLut<Q>>::new();
        let mut lut_map = HashMap::<PLut, Q>::new();
        for (p_lut, lut) in &self.luts {
            lut_map.insert(
                p_lut,
                a.insert(BitOrLut::Lut(
                    vec![],
                    vec![],
                    Lut {
                        bits: vec![],
                        perm: lut.perm.clone(),
                        visit: lut.visit,
                        bit_rc: lut.bit_rc,
                    },
                    format!("{:?}", p_lut),
                )),
            );
        }
        let mut bit_map = HashMap::<PBit, Q>::new();
        for (p_bit, bit) in &self.bits {
            bit_map.insert(
                p_bit,
                a.insert(BitOrLut::Bit(None, format!("{:?}", p_bit), Bit {
                    lut: bit.t.lut,
                    state: bit.t.state,
                    ..Default::default()
                })),
            );
        }
        // register luts to their bits
        for (p_lut, lut) in &self.luts {
            let p_lut = lut_map[&p_lut];
            for bit in lut.bits.iter().rev() {
                if let Some(prev) = Link::prev(&self.bits[bit]) {
                    if let BitOrLut::Lut(ref mut prevs, ..) = a[p_lut] {
                        prevs.push(
                            bit_map
                                .get(&prev)
                                .copied()
                                .unwrap_or_else(|| Ptr::invalid()),
                        );
                    }
                } else {
                    // need to preserve spots
                    let dummy = a.insert(BitOrLut::Dummy(Bit {
                        lut: None,
                        state: self.bits[bit].state,
                        ..Default::default()
                    }));
                    if let BitOrLut::Lut(ref mut prevs, ..) = a[p_lut] {
                        prevs.push(dummy);
                    }
                }
            }
            if let BitOrLut::Lut(_, ref mut nexts, ..) = &mut a[p_lut] {
                // push in reverse order
                for bit in lut.bits.iter().rev() {
                    nexts.push(bit_map.get(bit).copied().unwrap_or_else(|| Ptr::invalid()));
                }
            }
        }
        for p_bit in self.bits.ptrs() {
            if let Some(next) = Link::next(&self.bits[p_bit]) {
                if self.bits[next].t.lut.is_none() {
                    // direct connect
                    if let BitOrLut::Bit(ref mut p, ..) = a[bit_map[&p_bit]] {
                        *p = Some(bit_map[&next]);
                    }
                }
            }
        }
        let res = self.verify_integrity();
        triple_arena_render::render_to_svg_file(&a, false, out_file).unwrap();
        res
    }
}
