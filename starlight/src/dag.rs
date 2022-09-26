use std::fmt;

use awint::awint_dag::EvalError;
use triple_arena::{Arena, ChainArena, Link};

use crate::{PBit, PLut, PNote, Perm};

#[derive(Clone)]
pub struct Bit {
    /// Lookup table permutation that results in this bit
    pub lut: Option<PLut>,
    pub state: Option<bool>,
    /// Reference count for keeping this `Bit`
    pub rc: u64,
    pub visit: u64,
    /// Used in algorithms
    pub tmp: Option<bool>,
}

impl fmt::Debug for Bit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}",
            if let Some(b) = self.state {
                if b {
                    "1"
                } else {
                    "0"
                }
            } else {
                "*"
            },
        )
    }
}

impl Bit {
    pub fn new() -> Self {
        Self {
            lut: None,
            state: None,
            rc: 0,
            visit: 0,
            tmp: None,
        }
    }
}

impl Default for Bit {
    fn default() -> Self {
        Self::new()
    }
}

/// Lookup table permutation with extra information
#[derive(Debug, Clone)]
pub struct Lut {
    /// This is in order of the index bits of the lookup table
    pub bits: Vec<PBit>,
    pub perm: Perm,
    /// Used in algorithms to check for visitation
    pub visit: u64,
    /// Used in algorithms to track how many bits have been handled
    pub bit_rc: usize,
}

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<PBit>,
}

/// A DAG made of only permutations
#[derive(Debug, Clone)]
pub struct PermDag {
    /// In a permutation DAG, bits are never created or destroyed so there will
    /// be a single linear chain of `Bit`s for each bit.
    pub bits: ChainArena<PBit, Bit>,
    /// The lookup tables
    pub luts: Arena<PLut, Lut>,
    /// A kind of generation counter tracking the highest `visit` number
    pub visit_gen: u64,
    pub notes: Arena<PNote, Note>,
}

impl PermDag {
    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        for bit in self.bits.vals() {
            if let Some(lut) = bit.t.lut {
                if !self.luts.contains(lut) {
                    return Err(EvalError::OtherStr("broken `Ptr` from `Bit` to `Lut`"))
                }
            }
        }
        for (p_lut, lut) in &self.luts {
            for bit in &lut.bits {
                if let Some(bit) = self.bits.get(*bit) {
                    if bit.t.lut != Some(p_lut) {
                        // we just checked for containment before
                        return Err(EvalError::OtherStr(
                            "broken `Ptr` correspondance between `Lut` and `Bit`",
                        ))
                    }
                } else {
                    return Err(EvalError::OtherStr("broken `Ptr` from `Lut` to `Bit`"))
                }
            }
        }
        for note in self.notes.vals() {
            for bit in &note.bits {
                if let Some(bit) = self.bits.get(*bit) {
                    if bit.rc == 0 {
                        return Err(EvalError::OtherStr("reference count for noted bit is zero"))
                    }
                } else {
                    return Err(EvalError::OtherStr("broken `Ptr` in the noted bits"))
                }
            }
        }
        Ok(())
    }

    /// Evaluates `self` as much as possible
    pub fn eval(&mut self) {
        // acquire all evaluatable root bits
        let mut front = vec![];
        for (p_bit, bit) in &self.bits {
            if Link::prev(bit).is_none() && bit.state.is_some() {
                front.push(p_bit);
            }
        }

        let this_visit = self.visit_gen;
        self.visit_gen += 1;

        while let Some(p_bit) = front.pop() {
            if let Some(p_lut) = self.bits[p_bit].lut {
                let lut = &mut self.luts[p_lut];
                let len = lut.bits.len();
                if lut.visit < this_visit {
                    // reset temporaries
                    lut.bit_rc = len;
                    lut.visit = this_visit;
                }
                if self.bits[p_bit].tmp.is_some() {
                    lut.bit_rc -= 1;
                    if lut.bit_rc == 0 {
                        // acquire LUT input
                        let mut inx = 0;
                        for i in 0..len {
                            inx |= (self.bits[lut.bits[i]].tmp.unwrap() as usize) << i;
                        }
                        // evaluate
                        let out = lut.perm.get(inx).unwrap();
                        for i in 0..len {
                            let state = Some(((out >> i) & 1) != 0);
                            self.bits[lut.bits[i]].state = state;
                            // propogate
                            if let Some(p_next) = Link::next(&self.bits[lut.bits[i]]) {
                                self.bits[p_next].tmp = state;
                            }
                        }
                    }
                }
            } else if let Some(p_next) = Link::next(&self.bits[p_bit]) {
                // propogate state
                self.bits[p_next].tmp = self.bits[p_bit].state;
                front.push(p_next);
            }
        }
    }
}
