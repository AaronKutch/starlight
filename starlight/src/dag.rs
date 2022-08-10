use std::fmt;

use awint::awint_dag::EvalError;
use triple_arena::{Arena, ChainArena, Ptr};

use crate::Perm;

#[derive(Clone)]
pub struct BitState<PLut: Ptr> {
    /// Lookup table permutation that results in this bit
    pub lut: Option<PLut>,
    pub state: Option<bool>,
}

/// Lookup table permutation with extra information
#[derive(Debug, Clone)]
pub struct Lut<PBitState: Ptr> {
    /// This is in order of the index bits of the lookup table
    pub bits: Vec<PBitState>,
    pub perm: Perm,
    /// Used in algorithms to check for visitation
    pub visit_num: u64,
}

/// A DAG made of only permutations
#[derive(Debug, Clone)]
pub struct PermDag<PBitState: Ptr, PLut: Ptr> {
    /// In a permutation DAG, bits are never created or destroyed so there will
    /// be a single linear chain of `BitState`s for each bit.
    pub bits: ChainArena<PBitState, BitState<PLut>>,
    pub luts: Arena<PLut, Lut<PBitState>>,
    /// A kind of generation counter tracking the highest `visit_num` number
    pub visit_gen: u64,
    pub noted: Vec<PBitState>,
}

impl<PLut: Ptr> fmt::Debug for BitState<PLut> {
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

impl<PBitState: Ptr, PLut: Ptr> PermDag<PBitState, PLut> {
    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        for bit in self.bits.vals() {
            if let Some(lut) = bit.t.lut {
                if !self.luts.contains(lut) {
                    return Err(EvalError::OtherStr("broken `Ptr` from `BitState` to `Lut`"))
                }
            }
        }
        for (p_lut, lut) in &self.luts {
            for bit in &lut.bits {
                if let Some(bit) = self.bits.get(*bit) {
                    if bit.t.lut != Some(p_lut) {
                        // we just checked for containment before
                        return Err(EvalError::OtherStr(
                            "broken `Ptr` correspondance between `Lut` and `BitState`",
                        ))
                    }
                } else {
                    return Err(EvalError::OtherStr("broken `Ptr` from `Lut` to `BitState`"))
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
}
