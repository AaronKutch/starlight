use std::collections::HashMap;

use awint::{
    awint_dag::{
        common::{EvalError, Op::*},
        lowering::Dag,
    },
    bw, extawi, Bits, ExtAwi, InlAwi,
};
use triple_arena::{Arena, Ptr, PtrTrait};

use crate::{
    linked_list::{ChainArena, Link},
    BitState, Lut, Perm, PermDag,
};

impl<PBitState: PtrTrait, PLut: PtrTrait> PermDag<PBitState, PLut> {
    pub fn new<P: PtrTrait>(mut op_dag: Dag<P>) -> Result<Self, EvalError> {
        let mut res = Self {
            bits: ChainArena::new(),
            luts: Arena::new(),
            visit_gen: 0,
        };
        op_dag.visit_gen += 1;
        let gen = op_dag.visit_gen;
        // map between `Ptr<P>` and vectors of `Ptr<PBitState>`
        let mut map = HashMap::<Ptr<P>, Vec<Ptr<PBitState>>>::new();
        // DFS
        let noted_len = op_dag.noted.len();
        for j in 0..noted_len {
            let leaf = op_dag.noted[j];
            if op_dag[leaf].visit_num == gen {
                continue
            }
            let mut path: Vec<(usize, Ptr<P>)> = vec![(0, leaf)];
            loop {
                let (i, p) = path[path.len() - 1];
                let ops = op_dag[p].op.operands();
                if ops.is_empty() {
                    // reached a root
                    if op_dag[p].visit_num != gen {
                        op_dag[p].visit_num = gen;
                        match op_dag[p].op {
                            Literal(ref lit) => {
                                let mut v = vec![];
                                for i in 0..lit.bw() {
                                    v.push(res.bits.insert_new(BitState {
                                        lut: None,
                                        state: Some(lit.get(i).unwrap()),
                                    }));
                                }
                                map.insert(p, v);
                            }
                            Opaque(_) => {
                                let bw = op_dag.get_bw(p).unwrap().get();
                                let mut v = vec![];
                                for _ in 0..bw {
                                    v.push(res.bits.insert_new(BitState {
                                        lut: None,
                                        state: None,
                                    }));
                                }
                                map.insert(p, v);
                            }
                            ref op => {
                                return Err(EvalError::OtherString(format!("cannot lower {:?}", op)))
                            }
                        }
                    }
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                    path.last_mut().unwrap().0 += 1;
                } else if i >= ops.len() {
                    // checked all sources
                    match op_dag[p].op {
                        Copy([a]) => {
                            let source_bits = &map[&a];
                            let mut v = vec![];
                            for bit in source_bits {
                                v.push(res.copy_bit(*bit, gen).unwrap());
                            }
                            map.insert(p, v);
                        }
                        //Get([a, inx]) => {}
                        ref op => {
                            return Err(EvalError::OtherString(format!("cannot lower {:?}", op)))
                        }
                    }
                    path.pop().unwrap();
                    if path.is_empty() {
                        break
                    }
                } else {
                    let next_p = ops[i];
                    if op_dag[next_p].visit_num == gen {
                        // do not visit
                        path.last_mut().unwrap().0 += 1;
                    } else {
                        op_dag[next_p].visit_num = gen;
                        path.push((0, next_p));
                    }
                }
            }
        }
        Ok(res)
    }

    pub fn copy_bit(&mut self, p: Ptr<PBitState>, gen: u64) -> Option<Ptr<PBitState>> {
        if !self.bits.get_arena().contains(p) {
            return None
        }
        if let Some(new) = self.bits.insert_last(p, BitState {
            lut: None,
            state: None,
        }) {
            // this is the first copy, use the end of the chain directly
            Some(new)
        } else {
            // need to do a reversible copy
            /*
            azc acc 'z' for zero, 'a' for any, `c` for the copied bit
            000|000 <-
            001|011 <-
            010|010
            011|001
            100|100 <-
            101|111 <-
            110|110
            111|101
            The 'a' bit is preserved in all cases, 'c' is copied if 'z' is zero, and the lsb 'c'
            is always correct
            */
            let perm = Perm::from_raw(bw(3), extawi!(101_110_111_100_001_010_011_000));
            let mut res = None;
            self.luts.insert_with(|lut| {
                // insert a handle for the bit preserving LUT to latch on to
                let copy0 = self
                    .bits
                    .insert(Link {
                        t: BitState {
                            lut: Some(lut),
                            state: None,
                        },
                        prev: Some(p),
                        next: None,
                    })
                    .unwrap();
                let zero = self.bits.insert_new(BitState {
                    lut: None,
                    state: Some(false),
                });
                let copy1 = self
                    .bits
                    .insert_last(zero, BitState {
                        lut: Some(lut),
                        state: None,
                    })
                    .unwrap();
                res = Some(copy1);
                // implicit "don't care" state by having a LUT start the chain
                let any = self.bits.insert_new(BitState {
                    lut: Some(lut),
                    state: None,
                });
                Lut {
                    bits: vec![copy0, copy1, any],
                    perm,
                    visit_num: gen,
                }
            });
            res
        }
    }
}
