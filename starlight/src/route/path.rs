use crate::{
    route::{PBack, PCEdge},
    triple_arena::ptr_struct,
};

ptr_struct!(PHyperPath);

pub enum Edge {
    /// Points to a `CEdge`
    Transverse(PCEdge),
    /// Points to a `Referent::SuperNode`
    Concentrate(PBack),
    /// Points to a `Referent::SubNode`
    Dilute(PBack),
}

/// A single path from a source to sink across multiple `CEdge`s
#[derive(Debug, Clone)]
pub struct Path {
    sink: PBack,
    edges: Vec<PCEdge>,
    //critical_multiplier: u64,
}

/// Represents the "hyperpath" that a logical bit will take from a `source` node
/// to one ore more `sink` nodes. Sinks can have different priorities.
#[derive(Debug, Clone)]
pub struct HyperPath {
    source: PBack,
    paths: Vec<Path>,
}
