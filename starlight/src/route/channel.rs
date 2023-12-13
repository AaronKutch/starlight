use std::cmp::Ordering;

use awint::awint_dag::{
    smallvec::smallvec,
    triple_arena::{Arena, ArenaTrait, OrdArena},
};

use crate::{awint_dag::smallvec::SmallVec, ensemble::PBack, triple_arena::ptr_struct};

ptr_struct!(PCNode; PCEdge; PBackToCNode);

/// A channel node
#[derive(Debug, Clone)]
pub struct CNode {
    /// hierarchical capability
    /// and descends
    subnodes: SmallVec<[PCNode; 2]>,
    /// The hierarchy is like a dual overlapping binary tree one
    supernodes: SmallVec<[PCNode; 2]>,
}

/// A description of bits to set in order to achieve some desired edge behavior.
/// For now we unconditionally specify bits, in the future it should be more
/// detailed to allow for more close by programs to coexist
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Program {
    pub bits: SmallVec<[(PBack, bool); 4]>,
}

/// An edge between channels
#[derive(Debug, Clone)]
pub struct CEdge {
    /// The source `CNode`, this is readonly but bidirectional `Net`s can be
    /// represented with two `CEdge`s going both ways
    source: PCNode,
    /// The sink `CNode`
    sink: PCNode,

    // the variables above should uniquely determine a `CEdge`, we define `Eq` and `Ord` to only
    // respect the above and any insertion needs to check for duplicates
    /// Describes the required program to route a value (could be the `p_equiv`
    /// in a unit `CNode` or bulk routing through higher level `CNode`s) from
    /// the source to the sink.
    program: Program,
    // Ideally when `CNode`s are merged, they keep approximately the same weight distribution for
    // wide edges delay_weight: u64,
    //lagrangian_weight: u64,
}

impl PartialEq for CEdge {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.sink == other.sink
    }
}

impl Eq for CEdge {}

impl PartialOrd for CEdge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.source.partial_cmp(&other.source) {
            Some(Ordering::Equal) => {}
            ord => return ord,
        }
        self.sink.partial_cmp(&other.sink)
    }
}

impl Ord for CEdge {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.source.cmp(&other.source) {
            Ordering::Equal => {}
            ord => return ord,
        }
        self.sink.cmp(&other.sink)
    }
}

impl CEdge {
    pub fn program(&self) -> &Program {
        &self.program
    }
}

/// Management struct for channel nodes and edges
#[derive(Debug, Clone)]
pub struct Channeler {
    cnodes: Arena<PCNode, CNode>,
    cedges: OrdArena<PCEdge, CEdge, ()>,
    /// On hard dependencies where a path needs to connect to a particular
    /// `PBack`, valid descencions in the `CNode` hierarchy are determined by
    /// `find_with` to first get to the desired `PBack` section, then linear
    /// iterating to figure out which `CNode`s contain the `PBack`. The space is
    /// limited to a `n*log(n)`, there is probably some inevitable `n*log(n)`
    /// cost somewhere.
    backref_to_cnode: OrdArena<PBackToCNode, (PBack, PCNode), ()>,
}

impl Channeler {
    pub fn new() -> Self {
        Self {
            cnodes: Arena::new(),
            cedges: OrdArena::new(),
            backref_to_cnode: OrdArena::new(),
        }
    }

    pub fn make_cnode(&mut self, p_equiv: PBack) -> PCNode {
        self.cnodes.insert(CNode {
            subnodes: smallvec![],
            supernodes: smallvec![],
        })
    }

    pub fn make_cedge(&mut self, source: PCNode, sink: PCNode, program: Program) -> PCEdge {
        let (p_new, duplicate) = self.cedges.insert(
            CEdge {
                source,
                sink,
                program,
            },
            (),
        );
        // there may be future circumstances where we allow this and combine
        // appropriately, but disallow for now
        duplicate.unwrap();
        p_new
    }

    /// Starting from unit `CNode`s and `CEdge`s describing all known low level
    /// progam methods, this generates a logarithmic tree of higher level
    /// `CNode`s and `CEdge`s that results in top level `CNode`s that have no
    /// `CEdges` to any other (and unless the graph was disconnected there will
    /// be only one top level `CNode`).
    pub fn generate_hierarchy(&mut self) {}

    pub fn get_cnode(&self, p_cnode: PCNode) -> Option<&CNode> {
        self.cnodes.get(p_cnode)
    }

    pub fn get_cedge(&self, p_cedge: PCEdge) -> Option<&CEdge> {
        self.cedges.get(p_cedge).map(|(cedge, _)| cedge)
    }

    /// Starting from `p_cnode` assumed to contain `p_back`, this returns valid
    /// subnodes that still contain `PBack`
    pub fn valid_cnode_descensions(&self, p_cnode: PCNode, p_back: PBack) -> SmallVec<[PCNode; 4]> {
        if let Some(p) = self
            .backref_to_cnode
            .find_with(|_, (p_back1, p_cnode1), ()| p_back1.cmp(&p_back))
        {
            //
            let mut res = smallvec![];
            res
        } else {
            unreachable!()
        }
    }
}
