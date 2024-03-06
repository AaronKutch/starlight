use awint::awint_dag::triple_arena::{Advancer, Ptr};

use super::{Edge, EdgeKind, PCEdge, PCNode, PEmbedding, PMapping, Path, QCEdge, QCNode};
use crate::{
    route::{HyperPath, Router},
    Error,
};

#[derive(Debug, Clone)]
pub struct NodeSpread<PCNode: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    pub program_node: PCNode,
    pub target_hyperpath: HyperPath<QCNode, QCEdge>,
}

#[derive(Debug, Clone, Copy)]
pub enum NodeOrEdge<PCNode: Ptr, PCEdge: Ptr> {
    Node(PCNode),
    Edge(PCEdge),
}

#[derive(Debug, Clone)]
pub struct EdgeEmbed<PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    pub program_edge: PCEdge,
    pub target: NodeOrEdge<QCNode, QCEdge>,
}

#[derive(Debug, Clone)]
pub enum EmbeddingKind<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    /// A `CNode` needs to have its value spread across multiple target nodes
    NodeSpread(NodeSpread<PCNode, QCNode, QCEdge>),
    EdgeEmbed(EdgeEmbed<PCEdge, QCNode, QCEdge>),
}

#[derive(Debug, Clone)]
pub struct Embedding<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    pub kind: EmbeddingKind<PCNode, PCEdge, QCNode, QCEdge>,
}

impl<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> Embedding<PCNode, PCEdge, QCNode, QCEdge> {
    pub fn node_spread(program_node: PCNode, target_hyperpath: HyperPath<QCNode, QCEdge>) -> Self {
        Self {
            kind: EmbeddingKind::NodeSpread(NodeSpread {
                program_node,
                target_hyperpath,
            }),
        }
    }

    /// Returns a hyperpath if one is associated with `self`
    pub fn target_hyperpath(&self) -> Option<&HyperPath<QCNode, QCEdge>> {
        match &self.kind {
            EmbeddingKind::NodeSpread(node_spread) => Some(&node_spread.target_hyperpath),
            EmbeddingKind::EdgeEmbed(_) => todo!(),
        }
    }

    pub fn target_hyperpath_mut(&mut self) -> Option<&mut HyperPath<QCNode, QCEdge>> {
        match &mut self.kind {
            EmbeddingKind::NodeSpread(node_spread) => Some(&mut node_spread.target_hyperpath),
            EmbeddingKind::EdgeEmbed(_) => todo!(),
        }
    }
}

impl Router {
    /// Given the completed `Embedding`, sets up the embedding edges
    /// automatically
    fn make_embedding0(
        &mut self,
        embedding: Embedding<PCNode, PCEdge, QCNode, QCEdge>,
    ) -> Result<PEmbedding, Error> {
        // TODO: for now, we only put in a reference for an embedding into the program
        // channeler side and only allow at most one embedding per program `CNode`. If
        // we keep it this way then it should use an option, I suspect we may want to
        // register on both sides which will require a set for the target side.
        Ok(match embedding.kind {
            EmbeddingKind::NodeSpread(ref node_spread) => {
                let embeddings = &mut self
                    .program_channeler
                    .cnodes
                    .get_val_mut(node_spread.program_node)
                    .unwrap()
                    .embeddings;
                if !embeddings.is_empty() {
                    return Err(Error::OtherString(format!(
                        "program node {:?} is already associated with an embedding",
                        node_spread.program_node
                    )));
                }
                let p_embedding = self.embeddings.insert(embedding);
                embeddings.insert(p_embedding);
                p_embedding
            }
            EmbeddingKind::EdgeEmbed(_) => {
                todo!()
            }
        })
    }

    /// Makes a necessary embedding to express the given mapping.
    fn make_embedding1(&mut self, p_mapping: PMapping) -> Result<(), Error> {
        let (program_p_equiv, mapping) = self.mappings.get(p_mapping).unwrap();
        let program_p_equiv = *program_p_equiv;
        let program_cnode = self
            .program_channeler()
            .find_channeler_cnode(program_p_equiv)
            .unwrap();

        if let Some(ref source_mapping_target) = mapping.target_source {
            // find the corresponding `QCNode` for the source
            let target_source_p_equiv = source_mapping_target.target_p_equiv;
            let target_source_q_cnode = self
                .target_channeler()
                .find_channeler_cnode(target_source_p_equiv)
                .unwrap();

            // create path from source to root
            let mut q = target_source_q_cnode;
            let mut path_to_root = vec![];
            while let Some(tmp) = self.target_channeler().get_supernode(q) {
                q = tmp;
                path_to_root.push(Edge::new(EdgeKind::Concentrate, q));
            }
            let common_root_target_q_cnode = q;
            let mut paths = vec![];

            if !mapping.target_sinks.is_empty() {
                // If a mapping has both a source and sinks, then it is a trivial copy program
                // node. The embedding then has a hyperpath that connects the sources
                // to the sinks.

                // TODO instead of going all the way to the root node like in other cases, we
                // may just go to the common supernode of the source and sinks.

                // create paths from root to sinks, which will be concatenated on top of
                // `path_to_root`
                for (i, mapping_target) in mapping.target_sinks.iter().enumerate() {
                    let target_sink_p_equiv = mapping_target.target_p_equiv;
                    let target_sink_q_cnode = self
                        .target_channeler()
                        .find_channeler_cnode(target_sink_p_equiv)
                        .unwrap();

                    let mut q = target_sink_q_cnode;
                    let mut path_to_sink = vec![Edge::new(EdgeKind::Dilute, q)];
                    while let Some(tmp) = self.target_channeler().get_supernode(q) {
                        q = tmp;
                        path_to_sink.push(Edge::new(EdgeKind::Dilute, q));
                    }
                    if q != common_root_target_q_cnode {
                        let s = self.debug_mapping(p_mapping);
                        return Err(Error::OtherString(format!(
                            "When trying to find an initial embedding for a program bit that is \
                             mapped to both a target source and one or more target sinks (which \
                             occurs when mapping a trivial copy operation in the program directly \
                             onto a target), could not find a common supernode between the source \
                             and sink {i} (meaning that the target is like a disconnected graph \
                             and two parts of the mapping are on different parts that are \
                             impossible to route between). The mapping is:\n{s}\nThe roots are \
                             {common_root_target_q_cnode}, {q}"
                        )));
                    }
                    // remove extra dilution to root
                    path_to_sink.pop();
                    // better than repeated insertion, TODO any reduction improvements to paths
                    // should handle stuff like this, maybe just have `VecDeque` partials
                    path_to_sink.reverse();
                    let mut combined_path = vec![];
                    // first the common part from the source to root
                    combined_path.extend(path_to_root.iter().copied());
                    combined_path.extend(path_to_sink);

                    paths.push(Path::new(combined_path));
                }

                self.make_embedding0(Embedding::node_spread(
                    program_cnode,
                    HyperPath::new(target_source_q_cnode, paths),
                ))
                .unwrap();

                // this case is not bound to any other part of the program
                // graph, so we do not touch root embeddings
            } else {
                // If the mapping has just a source, then a hyperpath needs to
                // go concentrating to a root node. If anything depending on the source does not
                // also have the root node in common, then there is a disconnection which we
                // easily detect later. There might not be a universal common root node in case
                // of disconnected targets.

                self.make_embedding0(Embedding::node_spread(
                    program_cnode,
                    HyperPath::new(target_source_q_cnode, vec![Path::new(path_to_root)]),
                ))
                .unwrap();
            }
        } else {
            // The mapping just has sinks, then a hyper path
            // needs to go from the root node diluting to the sinks, and we also do the root
            // comparison from above

            let mut common_root_target_q_cnode = None;
            let mut paths = vec![];
            for (i, mapping_target) in mapping.target_sinks.iter().enumerate() {
                let target_sink_p_equiv = mapping_target.target_p_equiv;
                let target_sink_q_cnode = self
                    .target_channeler()
                    .find_channeler_cnode(target_sink_p_equiv)
                    .unwrap();

                let mut q = target_sink_q_cnode;
                let mut path_to_sink = vec![Edge::new(EdgeKind::Dilute, q)];
                while let Some(tmp) = self.target_channeler().get_supernode(q) {
                    q = tmp;
                    path_to_sink.push(Edge::new(EdgeKind::Dilute, q));
                }
                let root_node = path_to_sink.pop().unwrap().to;
                if i == 0 {
                    common_root_target_q_cnode = Some(root_node);
                } else if common_root_target_q_cnode != Some(root_node) {
                    let s = self.debug_mapping(p_mapping);
                    return Err(Error::OtherString(format!(
                        "When trying to find an initial embedding for a program bit that is \
                         mapped to more than one target sink, could not find a common supernode \
                         between the sinks (meaning that the target is like a disconnected graph \
                         and two parts of the mapping are on different parts that are impossible \
                         to route between). The mapping is:\n{s}"
                    )));
                }
                // remove extra dilution to root
                path_to_sink.pop();
                path_to_sink.reverse();
                paths.push(Path::new(path_to_sink));
            }

            self.make_embedding0(Embedding::node_spread(
                program_cnode,
                HyperPath::new(common_root_target_q_cnode.unwrap(), paths),
            ))
            .unwrap();
        }

        // TODO support custom `CEdge` mappings

        Ok(())
    }

    /// Clears embeddings and uses mappings to make embeddings that are known to
    /// be neccessary for the routing to be possible.
    pub(crate) fn initialize_embeddings(&mut self) -> Result<(), Error> {
        // in case of rerouting we need to clear old embeddings
        self.embeddings.clear();
        for cnode in self.program_channeler.cnodes.vals_mut() {
            cnode.embeddings.clear_and_shrink();
        }
        for cedge in self.program_channeler.cedges.vals_mut() {
            cedge.embeddings.clear_and_shrink();
        }

        // Mappings will stay static because they are used for figuring out translating
        // program IO to target IO. Embeddings will represent bulk programmings of the
        // hierarchy. However, we know that the mappings correspond to some embeddings
        // that are absolutely necessary for the routing to be possible, so we can start
        // by making those embeddings.
        let mut adv = self.mappings.advancer();
        while let Some(p_mapping) = adv.advance(&self.mappings) {
            self.make_embedding1(p_mapping).unwrap()
        }
        Ok(())
    }
}
