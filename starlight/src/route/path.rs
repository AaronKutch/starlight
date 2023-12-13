use crate::{
    route::channel::{PCEdge, PCNode},
    triple_arena::ptr_struct,
};

ptr_struct!(PHyperPath);

/// A single path from a source to sink across multiple `CEdge`s
#[derive(Debug, Clone)]
pub struct Path {
    sink: PCNode,
    edges: Vec<PCEdge>,
    //critical_multiplier: u64,
}

/// Represents the "hyperpath" that a logical bit will take from a `source` node
/// to one ore more `sink` nodes. Sinks can have different priorities.
#[derive(Debug, Clone)]
pub struct HyperPath {
    source: PCNode,
    paths: Vec<Path>,
}
