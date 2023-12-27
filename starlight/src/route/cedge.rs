use awint::awint_dag::smallvec::smallvec;

use super::{channel::Referent, Channeler};
use crate::{
    awint_dag::smallvec::SmallVec, ensemble, route::PBack, triple_arena::ptr_struct, Epoch,
};

ptr_struct!(PCEdge);

/// Used by higher order edges to tell what it is capable of overall
#[derive(Debug, Clone)]
pub struct BulkBehavior {
    /// The number of bits that can enter this channel
    channel_entry_width: usize,
    /// The number of bits that can exit this channel
    channel_exit_width: usize,
    /// For now, we just add up the number of LUT bits in the channel
    lut_bits: usize,
}

#[derive(Debug, Clone)]
pub enum Behavior {
    /// Routes the bit from `source` to `sink`
    RouteBit,
    /// Can behave as an arbitrary lookup table outputting a bit and taking the
    /// input bits.
    ArbitraryLut(PBack, SmallVec<[PBack; 4]>),
    /// Bulk behavior
    Bulk(BulkBehavior),
    /// Nothing can happen between nodes, used for connecting top level nodes
    /// that have no connection to each other
    Noop,
}

/// A description of bits to set in order to achieve some desired edge behavior.
/// For now we unconditionally specify bits, in the future it should be more
/// detailed to allow for more close by programs to coexist
#[derive(Debug, Clone)]
pub struct Instruction {
    pub set_bits: SmallVec<[(ensemble::PBack, bool); 4]>,
}

#[derive(Debug, Clone)]
pub struct Programmability {
    /// The behavior that can be programmed into this edge
    behavior: Behavior,
    /// The instruction required to get the desired behavior
    instruction: Instruction,
}

/// An edge between channels
#[derive(Debug, Clone)]
pub struct CEdge {
    /// The sources and sinks
    pub incidences: SmallVec<[PBack; 4]>,

    // the variables above should uniquely determine a `CEdge`, we define `Eq` and `Ord` to only
    // respect the above and any insertion needs to check for duplicates
    /// Describes the required program to route a value (could be the `p_equiv`
    /// in a unit `CNode` or bulk routing through higher level `CNode`s) from
    /// the source to the sink.
    programmability: Programmability,
    // Ideally when `CNode`s are merged, they keep approximately the same weight distribution for
    // wide edges delay_weight: u64,
    //lagrangian_weight: u64,
}

impl CEdge {
    pub fn programmability(&self) -> &Programmability {
        &self.programmability
    }
}

impl Channeler {
    /// Given the `incidences` (which should point to unique `ThisCNode`s), this
    /// will manage the backrefs
    pub fn make_cedge(&mut self, incidences: &[PBack], programmability: Programmability) -> PCEdge {
        self.cedges.insert_with(|p_self| {
            let mut actual_incidences = smallvec![];
            for (i, incidence) in incidences.iter().enumerate() {
                actual_incidences.push(
                    self.cnodes
                        .insert_key(*incidence, Referent::CEdgeIncidence(p_self, i))
                        .unwrap(),
                );
            }
            CEdge {
                incidences: actual_incidences,
                programmability,
            }
        })
    }

    /// Assumes that `epoch` has been optimized
    pub fn from_epoch(epoch: &Epoch) -> Self {
        let mut channeler = Self::new();

        epoch.ensemble(|ensemble| {
            // for each equivalence make a `CNode` with associated `EnsembleBackref`
            for equiv in ensemble.backrefs.vals() {
                let p_cnode = channeler.make_top_level_cnode(vec![]);
                channeler
                    .cnodes
                    .insert_key(p_cnode, Referent::EnsembleBackRef(equiv.p_self_equiv))
                    .unwrap();
            }

            // add `CEdge`s according to `LNode`s
            for lnode in ensemble.lnodes.vals() {
                //if let Some(lnode.lut
            }
        });

        channeler
    }
}
