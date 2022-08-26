use std::fmt;

use awint::awint_dag::EvalError;
use triple_arena::{Arena, ChainArena, Link};

use crate::{PBit, PLut, Perm};

#[derive(Clone)]
pub struct Bit {
    /// Lookup table permutation that results in this bit
    pub lut: Option<PLut>,
    pub state: Option<bool>,
}

/// Lookup table permutation with extra information
#[derive(Debug, Clone)]
pub struct Lut {
    /// This is in order of the index bits of the lookup table
    pub bits: Vec<PBit>,
    pub perm: Perm,
    /// Used in algorithms to check for visitation
    pub visit: u64,
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
    pub noted: Vec<PBit>,
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
            }
        )
    }
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
        for note in &self.noted {
            if !self.bits.contains(*note) {
                return Err(EvalError::OtherStr("broken `Ptr` in the noted bits"))
            }
        }
        Ok(())
    }

    /// Evaluates `self` as much as possible
    pub fn eval(&mut self) -> Result<(), EvalError> {
        // acquire all evaluatable root bits
        let mut roots = vec![];
        for (p_bit, bit) in &self.bits {
            if Link::prev(bit).is_none() && bit.state.is_some() {
                roots.push(p_bit);
            }
        }

        Ok(())
    }
}
