use crate::{awint_dag::smallvec::SmallVec, ensemble::PBack, triple_arena::ptr_struct};

ptr_struct!(PCNode; PCEdge);

/// A channel node
#[derive(Debug, Clone)]
pub struct CNode {
    /// hierarchical capability, routing starts at some single top level node
    /// and descends
    subnodes: SmallVec<[PCNode; 2]>,
    /// The hierarchy is like a dual overlapping binary tree one
    supernodes: SmallVec<[PCNode; 2]>,
    /// at the bottom of the hierarchy are always unit `CNode`s marking a single
    /// value point.
    p_equiv: Option<PBack>,
}

/// A description of bits to set in order to achieve some desired edge behavior.
/// For now we unconditionally specify bits, in the future it should be more
/// detailed to allow for more close by programs to coexist
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Program {
    bits: SmallVec<[(PBack, bool); 4]>,
}

/// An edge between channels
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CEdge {
    /// The source `CNode`, this is readonly but bidirectional `Net`s can be
    /// represented with two `CEdge`s going both ways
    source: PCNode,
    /// The sink `CNode`
    sink: PCNode,
    /// Describes the required program to route a value (could be the `p_equiv`
    /// in a unit `CNode` or bulk routing through higher level `CNode`s) from
    /// the source to the sink.
    program: Program,
    // Ideally when `CNode`s are merged, they keep approximately the same weight distribution for
    // wide edges delay_weight: u64,
    //lagrangian_weight: u64,
}
