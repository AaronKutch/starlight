use awint::awint_dag::triple_arena::Ptr;

use crate::triple_arena::ptr_struct;

ptr_struct!(PHyperPath);

#[derive(Debug, Clone)]
pub enum Edge<QCNode: Ptr, QCEdge: Ptr> {
    /// Points to a `CEdge`
    Transverse(QCEdge),
    /// Points to a `Referent::SuperNode`
    Concentrate(QCNode),
    /// Points to a `Referent::SubNode`
    Dilute(QCNode),
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

    pub fn push(&mut self, edge: Edge<QCNode, QCEdge>) {
        self.edges.push(edge)
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
}
