use awint::awint_dag::triple_arena::Ptr;

use crate::triple_arena::ptr_struct;

ptr_struct!(PHyperPath);

#[derive(Debug, Clone)]
pub enum Edge<QBack: Ptr, QCEdge: Ptr> {
    /// Points to a `CEdge`
    Transverse(QCEdge),
    /// Points to a `Referent::SuperNode`
    Concentrate(QBack),
    /// Points to a `Referent::SubNode`
    Dilute(QBack),
}

/// A single path from a source to sink across multiple `CEdge`s
#[derive(Debug, Clone)]
pub struct Path<QBack: Ptr, QCEdge: Ptr> {
    sink: QBack,
    edges: Vec<Edge<QBack, QCEdge>>,
    //critical_multiplier: u64,
}

impl<QBack: Ptr, QCEdge: Ptr> Path<QBack, QCEdge> {
    pub fn new(sink: QBack) -> Self {
        Self {
            sink,
            edges: vec![],
        }
    }

    pub fn push(&mut self, edge: Edge<QBack, QCEdge>) {
        self.edges.push(edge)
    }
}

/// Represents the "hyperpath" that a logical bit will take from a `source` node
/// to one ore more `sink` nodes. Sinks can have different priorities.
#[derive(Debug, Clone)]
pub struct HyperPath<QBack: Ptr, QCEdge: Ptr> {
    source: QBack,
    paths: Vec<Path<QBack, QCEdge>>,
}

impl<QBack: Ptr, QCEdge: Ptr> HyperPath<QBack, QCEdge> {
    pub fn new(source: QBack) -> Self {
        Self {
            source,
            paths: vec![],
        }
    }
}
