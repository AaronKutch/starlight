use std::{cmp::max, collections::HashMap, num::NonZeroUsize};

use awint::{
    awint_dag::{
        lowering::{Dag, PNode},
        EvalError,
        Op::*,
    },
    bw, extawi, inlawi, Bits, ExtAwi, InlAwi,
};
use triple_arena::{Arena, ChainArena};

use crate::{Bit, Lut, Note, PBit, PNote, Perm, PermDag};

impl PermDag {
    /// Constructs a directed acyclic graph of permutations from an
    /// `awint_dag::Dag`. `op_dag.noted` are translated as bits in lsb to msb
    /// order.
    ///
    /// If an error occurs, the DAG (which may be in an unfinished or completely
    /// broken state) is still returned along with the error enum, so that debug
    /// tools like `render_to_svg_file` can be used.
    pub fn from_op_dag(op_dag: &mut Dag) -> (Self, Result<Vec<PNote>, EvalError>) {
        let mut res = Self {
            bits: ChainArena::new(),
            luts: Arena::new(),
            visit_gen: 0,
            notes: Arena::new(),
        };
        let err = res.add_group(op_dag);
        (res, err)
    }

    pub fn add_group(&mut self, op_dag: &mut Dag) -> Result<Vec<PNote>, EvalError> {
        op_dag.visit_gen += 1;
        let gen = op_dag.visit_gen;
        let mut map = HashMap::<PNode, Vec<PBit>>::new();
        // DFS
        let noted_len = op_dag.noted.len();
        for j in 0..noted_len {
            if let Some(leaf) = op_dag.noted[j] {
                if op_dag[leaf].visit == gen {
                    continue
                }
                let mut path: Vec<(usize, PNode)> = vec![(0, leaf)];
                loop {
                    let (i, p) = path[path.len() - 1];
                    let ops = op_dag[p].op.operands();
                    if ops.is_empty() {
                        // reached a root
                        match op_dag[p].op {
                            Literal(ref lit) => {
                                let mut v = vec![];
                                for i in 0..lit.bw() {
                                    v.push(self.bits.insert_new(Bit {
                                        lut: None,
                                        state: Some(lit.get(i).unwrap()),
                                        ..Default::default()
                                    }));
                                }
                                map.insert(p, v);
                            }
                            Opaque(_) => {
                                let bw = op_dag.get_bw(p).get();
                                let mut v = vec![];
                                for _ in 0..bw {
                                    v.push(self.bits.insert_new(Bit {
                                        lut: None,
                                        state: None,
                                        ..Default::default()
                                    }));
                                }
                                map.insert(p, v);
                            }
                            ref op => {
                                return Err(EvalError::OtherString(format!("cannot lower {:?}", op)))
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
                            Copy([x]) => {
                                let source_bits = &map[&x];
                                let mut v = vec![];
                                for bit in source_bits {
                                    v.push(self.copy_bit(*bit, gen).unwrap());
                                }
                                map.insert(p, v);
                            }
                            StaticGet([bits], inx) => {
                                let bit = map[&bits][inx];
                                map.insert(p, vec![self.copy_bit(bit, gen).unwrap()]);
                            }
                            StaticSet([bits, bit], inx) => {
                                let bit = &map[&bit];
                                assert_eq!(bit.len(), 1);
                                let bit = bit[0];
                                let bits = &map[&bits];
                                // TODO this is inefficient
                                let mut v = bits.clone();
                                v[inx] = bit;
                                map.insert(p, v);
                            }
                            StaticLut([inx], ref table) => {
                                let inxs = &map[&inx];
                                let v = self.permutize_lut(inxs, table, gen).unwrap();
                                map.insert(p, v);
                            }
                            ref op => {
                                return Err(EvalError::OtherString(format!("cannot lower {:?}", op)))
                            }
                        }
                        path.pop().unwrap();
                        if path.is_empty() {
                            break
                        }
                    } else {
                        let p_next = ops[i];
                        if op_dag[p_next].visit == gen {
                            // do not visit
                            path.last_mut().unwrap().0 += 1;
                        } else {
                            op_dag[p_next].visit = gen;
                            path.push((0, p_next));
                        }
                    }
                }
            }
        }
        let mut note_map = vec![];
        // handle the noted
        for noted in op_dag.noted.iter().flatten() {
            let mut note = vec![];
            // TODO what guarantees do we give?
            //if op_dag[note].op.is_opaque() {}
            for bit in &map[noted] {
                note.push(*bit);
            }
            note_map.push(self.notes.insert(Note { bits: note }));
        }
        Ok(note_map)
    }

    /// Copies the bit at `p` with a reversible permutation if needed
    pub fn copy_bit(&mut self, p: PBit, gen: u64) -> Option<PBit> {
        if !self.bits.contains(p) {
            return None
        }

        if let Some(new) = self.bits.insert_end(p, Bit::default()) {
            // this is the first copy, use the end of the chain directly
            Some(new)
        } else {
            // need to do a reversible copy
            /*
            zc cc 'z' for zero, `c` for the copied bit
            00|00
            01|11
            10|10
            11|01
            'c' is copied if 'z' is zero, and the lsb 'c' is always correct
            */
            let perm = Perm::from_raw(bw(2), extawi!(01_10_11_00));
            let mut res = None;
            self.luts.insert_with(|lut| {
                // insert a handle for the bit preserving LUT to latch on to
                let copy0 = self
                    .bits
                    .insert((Some(p), None), Bit {
                        lut: Some(lut),
                        state: None,
                        ..Default::default()
                    })
                    .unwrap();

                let zero = self.bits.insert_new(Bit {
                    lut: Some(lut),
                    state: Some(false),
                    ..Default::default()
                });
                res = Some(zero);
                Lut {
                    bits: vec![copy0, zero],
                    perm,
                    visit: gen,
                    bit_rc: 0,
                }
            });

            res
        }
    }

    #[allow(clippy::needless_range_loop)]
    pub fn permutize_lut(&mut self, inxs: &[PBit], table: &Bits, gen: u64) -> Option<Vec<PBit>> {
        // TODO have some kind of upstream protection for this
        assert!(inxs.len() <= 4);
        let num_entries = 1 << inxs.len();
        assert_eq!(table.bw() % num_entries, 0);
        let original_out_bw = table.bw() / num_entries;
        assert!(original_out_bw <= 4);
        // if all entries are the same value then 2^8 is needed
        let mut set = inlawi!(0u256);
        /*
        consider a case like:
        ab|y
        00|0
        01|0
        10|0
        11|1
        There are 3 entries of '0', which means we need at least ceil(lb(3)) = 2 zero bits to turn
        this into a permutation:
        zzab|  y
        0000|000 // concatenate with an incrementing value unique to the existing bit patterns
        0001|010
        0010|100
        0011|001
        ... then after the original table is preserved iterate over remaining needed entries in
        order, which tends to give a more ideal table
        */
        let mut entries = vec![0; num_entries];
        // counts the number of occurances of an entry value
        let mut integer_counts = vec![0; num_entries];
        let mut inx = extawi!(zero: ..(inxs.len())).unwrap();
        let mut tmp = extawi!(zero: ..(original_out_bw)).unwrap();
        let mut max_count = 0;
        for i in 0..num_entries {
            inx.usize_assign(i);
            tmp.lut_assign(table, &inx).unwrap();
            let original_entry = tmp.to_usize();
            let count = integer_counts[original_entry];
            max_count = max(count, max_count);
            let new_entry = original_entry | (count << original_out_bw);
            set.set(new_entry, true).unwrap();
            entries[i] = new_entry;
            integer_counts[original_entry] = count + 1;
        }
        let extra_bits = (64 - max_count.leading_zeros()) as usize;
        let new_w = extra_bits + original_out_bw;
        let mut perm = Perm::ident(NonZeroUsize::new(new_w).unwrap()).unwrap();
        let mut j = entries.len();
        for (i, entry) in entries.into_iter().enumerate() {
            perm.unstable_set(i, entry).unwrap();
        }
        // all the remaining garbage entries
        for i in 0..(1 << new_w) {
            if !set.get(i).unwrap() {
                perm.unstable_set(j, i).unwrap();
                j += 1;
            }
        }

        let mut extended_v = vec![];
        // get copies of all index bits
        for inx in inxs {
            extended_v.push(self.copy_bit(*inx, gen).unwrap());
        }
        // get the zero bits
        for _ in inxs.len()..new_w {
            extended_v.push(self.bits.insert_new(Bit {
                lut: None,
                state: Some(false),
                ..Default::default()
            }));
        }
        // because this is the actual point where LUTs are inserted, we need an extra
        // layer to make room for the lut specification
        let mut lut_layer = vec![];
        self.luts.insert_with(|lut| {
            for bit in extended_v {
                lut_layer.push(
                    self.bits
                        .insert((Some(bit), None), Bit {
                            lut: Some(lut),
                            state: None,
                            ..Default::default()
                        })
                        .unwrap(),
                );
            }
            Lut {
                bits: lut_layer.clone(),
                perm,
                visit: gen,
                bit_rc: 0,
            }
        });
        // only return the part of the layer for the original LUT output
        for _ in original_out_bw..new_w {
            lut_layer.pop();
        }
        Some(lut_layer)
    }
}
