use crate::{
    ensemble::{PBack, PLNode},
    route::{PCEdge, PCNode},
};

#[derive(Debug, Clone, Copy)]
pub enum NodeOrEdge {
    Node(PCNode),
    Edge(PCEdge),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeKind {
    /// Edge through a `CEdge` between `CNode`s on the same level. The `usize`
    /// indicates which source is used.
    Transverse(PCEdge, usize),
    /// Edge to a higher level `CNode`
    Concentrate,
    /// Edge to a lower level `CNode`
    Dilute,
}

#[derive(Debug, Clone, Copy)]
pub struct Edge {
    /// The method of traversal
    pub kind: EdgeKind,
    /// The incident the edge reaches, the concentration and
    /// dilution edges can easily be derived from this.
    pub to: PCNode,
}

impl Edge {
    pub fn new(kind: EdgeKind, to: PCNode) -> Self {
        Self { kind, to }
    }
}

/// A single path from a source to sink across multiple `CEdge`s
#[derive(Debug, Clone)]
pub struct Path {
    /// If `None`, then this is a necessary copy-to-output embedding, otherwise
    /// this is a `PBack` to a `Referent::Input`
    pub program_sink: Option<PBack>,
    // the target sink is on the last edge
    pub edges: Vec<Edge>,
    //critical_multiplier: u64,
}

impl Path {
    pub fn new(program_sink: Option<PBack>, edges: Vec<Edge>) -> Self {
        Self {
            program_sink,
            edges,
        }
    }

    // Returns `None` only if the path is empty which shouldn't be the case
    pub fn target_sink(&self) -> Option<PCNode> {
        Some(self.edges().last()?.to)
    }

    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    pub fn push(&mut self, edge: Edge) {
        self.edges.push(edge)
    }
}

/// Represents the "hyperpath" that a logical bit will take from a `source` node
/// to one ore more `sink` nodes. Sinks can have different priorities.
#[derive(Debug, Clone)]
pub struct HyperPath {
    /// If `None`, then this is a necessary input embedding, otherwise this is
    /// driven by the output of the `PLNode`
    pub program_source: Option<PLNode>,
    pub target_source: PCNode,
    paths: Vec<Path>,
}

impl HyperPath {
    pub fn new(program_source: Option<PLNode>, target_source: PCNode, paths: Vec<Path>) -> Self {
        Self {
            program_source,
            target_source,
            paths,
        }
    }

    pub fn push(&mut self, path: Path) {
        self.paths.push(path)
    }

    pub fn paths(&self) -> &[Path] {
        &self.paths
    }

    pub fn paths_mut(&mut self) -> &mut [Path] {
        &mut self.paths
    }
}
