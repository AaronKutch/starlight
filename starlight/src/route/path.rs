use std::iter::IntoIterator;

use awint::awint_dag::triple_arena::Ptr;

#[derive(Debug, Clone, Copy)]
pub enum EdgeKind<QCEdge: Ptr> {
    /// Edge through a `CEdge` between `CNode`s on the same level. The
    Transverse(QCEdge, Option<usize>),
    /// Edge to a higher level `CNode`
    Concentrate,
    /// Edge to a lower level `CNode`
    Dilute,
}

#[derive(Debug, Clone, Copy)]
pub struct Edge<QCNode: Ptr, QCEdge: Ptr> {
    /// The method of traversal
    pub kind: EdgeKind<QCEdge>,
    /// The `ThisCNode` incident the edge reaches, the concentration and
    /// dilution edges can easily be derived from this.
    pub to: QCNode,
}

impl<QCNode: Ptr, QCEdge: Ptr> Edge<QCNode, QCEdge> {
    pub fn new(kind: EdgeKind<QCEdge>, to: QCNode) -> Self {
        Self { kind, to }
    }
}

/// A single path from a source to sink across multiple `CEdge`s
#[derive(Debug, Clone)]
pub struct Path<QCNode: Ptr, QCEdge: Ptr> {
    sink: QCNode,
    edges: Vec<Edge<QCNode, QCEdge>>,
    //critical_multiplier: u64,
}

impl<QCNode: Ptr, QCEdge: Ptr> Path<QCNode, QCEdge> {
    pub fn new(sink: QCNode) -> Self {
        Self {
            sink,
            edges: vec![],
        }
    }

    pub fn sink(&self) -> QCNode {
        self.sink
    }

    pub fn edges(&self) -> &[Edge<QCNode, QCEdge>] {
        &self.edges
    }

    pub fn push(&mut self, edge: Edge<QCNode, QCEdge>) {
        self.edges.push(edge)
    }

    pub fn extend<I: IntoIterator<Item = Edge<QCNode, QCEdge>>>(&mut self, edges: I) {
        self.edges.extend(edges)
    }
}

/// Represents the "hyperpath" that a logical bit will take from a `source` node
/// to one ore more `sink` nodes. Sinks can have different priorities.
#[derive(Debug, Clone)]
pub struct HyperPath<QCNode: Ptr, QCEdge: Ptr> {
    source: QCNode,
    paths: Vec<Path<QCNode, QCEdge>>,
}

impl<QCNode: Ptr, QCEdge: Ptr> HyperPath<QCNode, QCEdge> {
    pub fn new(source: QCNode) -> Self {
        Self {
            source,
            paths: vec![],
        }
    }

    pub fn source(&self) -> QCNode {
        self.source
    }

    pub fn push(&mut self, path: Path<QCNode, QCEdge>) {
        self.paths.push(path)
    }

    pub fn paths(&self) -> &[Path<QCNode, QCEdge>] {
        &self.paths
    }

    pub fn paths_mut(&mut self) -> &mut [Path<QCNode, QCEdge>] {
        &mut self.paths
    }
}
