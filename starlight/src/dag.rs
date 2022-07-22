use triple_arena::{Arena, Ptr, PtrTrait};

use crate::{linked_list::ChainArena, Perm};

#[derive(Debug, Clone)]
pub struct BitState<PLut: PtrTrait> {
    /// Lookup table permutation that results in this bit
    pub lut: Option<Ptr<PLut>>,
    pub state: Option<bool>,
}

/// Lookup table permutation with extra information
#[derive(Debug, Clone)]
pub struct Lut<PBitState: PtrTrait> {
    /// This is in order of the index bits of the lookup table
    pub bits: Vec<Ptr<PBitState>>,
    pub perm: Perm,
    /// Used in algorithms to check for visitation
    pub visit_num: u64,
}

/// A DAG made of only permutations
#[derive(Debug, Clone)]
pub struct PermDag<PBitState: PtrTrait, PLut: PtrTrait> {
    /// In a permutation DAG, bits are never created or destroyed so there will
    /// be a single linear chain of `BitState`s for each bit.
    pub bits: ChainArena<PBitState, BitState<PLut>>,
    pub luts: Arena<PLut, Lut<PBitState>>,
    /// A kind of generation counter tracking the highest `visit_num` number
    pub visit_gen: u64,
}
