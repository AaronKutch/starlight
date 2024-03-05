use awint::awint_dag::triple_arena::{Advancer, Ptr};

use super::{Edge, EdgeKind, PCEdge, PCNode, PEmbedding, PMapping, Path, QCEdge, QCNode};
use crate::{
    route::{HyperPath, Router},
    Error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EmbeddingKind<PCNode: Ptr, PCEdge: Ptr> {
    Edge(PCEdge),
    Node(PCNode),
}

#[derive(Debug, Clone)]
pub struct Embedding<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    pub program: EmbeddingKind<PCNode, PCEdge>,
    pub target_hyperpath: HyperPath<QCNode, QCEdge>,
}

impl Router {
    /// Given the completed `Embedding`, sets up the embedding edges
    /// automatically
    fn make_embedding0(
        &mut self,
        embedding: Embedding<PCNode, PCEdge, QCNode, QCEdge>,
    ) -> Result<PEmbedding, Error> {
        let program = embedding.program;
        let p_embedding = self.embeddings.insert(embedding);

        // NOTE: for now, we only put in a reference for an embedding into the program
        // channeler side and only allow at most one embedding per program `CNode`. If
        // we keep it this way then it should use an option, I suspect we may want to
        // register on both sides which will require a set for the target side.
        match program {
            EmbeddingKind::Edge(p_cedge) => {
                let embeddings = &mut self
                    .program_channeler
                    .cedges
                    .get_mut(p_cedge)
                    .unwrap()
                    .embeddings;
                if !embeddings.is_empty() {
                    return Err(Error::OtherString(format!(
                        "program edge {p_cedge:?} is already associated with an embedding"
                    )));
                }
                embeddings.insert(p_embedding);
            }
            EmbeddingKind::Node(p_cnode) => {
                let embeddings = &mut self
                    .program_channeler
                    .cnodes
                    .get_val_mut(p_cnode)
                    .unwrap()
                    .embeddings;
                if !embeddings.is_empty() {
                    return Err(Error::OtherString(format!(
                        "program node {p_cnode:?} is already associated with an embedding"
                    )));
                }
                embeddings.insert(p_embedding);
            }
        }
        Ok(p_embedding)
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
            // begin constructing hyperpath for the embedding
            let mut hyperpath = HyperPath::<QCNode, QCEdge>::new(target_source_q_cnode);

            if !mapping.target_sinks.is_empty() {
                // If a mapping has both a source and sinks, then we need an embedding of the
                // program cnode that embeds in a target cnode that can cover all the sources
                // and the sinks. The embedding then has a hyperpath that connects the sources
                // and sinks.

                // we are dealing with the single program node copying mapping case, which does
                // not interact with anything else directly so we only deal with the common
                // supernode of our source and sinks

                // begin finding the common target cnode
                let mut root_common_target_q_cnode = target_source_q_cnode;

                // do the same for the sinks
                for (i, mapping_target) in mapping.target_sinks.iter().enumerate() {
                    let target_sink_p_equiv = mapping_target.target_p_equiv;
                    let target_sink_q_cnode = self
                        .target_channeler()
                        .find_channeler_cnode(target_sink_p_equiv)
                        .unwrap();
                    let path = Path::new(target_sink_q_cnode);
                    hyperpath.push(path);
                    root_common_target_q_cnode = if let Some(q_cnode) = self
                        .target_channeler()
                        .find_common_supernode(root_common_target_q_cnode, target_sink_q_cnode)
                    {
                        q_cnode
                    } else {
                        let s = self.debug_mapping(p_mapping);
                        return Err(Error::OtherString(format!(
                            "When trying to find an initial embedding for a program bit that is \
                             mapped to both a target source and one or more target sinks (which \
                             occurs when mapping a trivial copy operation in the program directly \
                             onto a target), could not find a common supernode between the source \
                             and sink {i} (meaning that the target is like a disconnected graph \
                             and two parts of the mapping are on different parts that are \
                             impossible to route between). The mapping is:\n{s}\nThe `CNodes` are \
                             {root_common_target_q_cnode}, {target_sink_q_cnode}"
                        )));
                    };
                }

                // the endpoints of the hyperedge are initialized, initialize the paths by
                // connecting them all through the common supernode

                // get the common edges to the common root
                let mut q = hyperpath.source();
                let mut path_to_root = vec![];
                while q != root_common_target_q_cnode {
                    q = self.target_channeler().get_supernode(q).unwrap();
                    path_to_root.push(Edge::new(EdgeKind::Concentrate, q));
                }
                // push on the path to root and a path back down to the sink
                for path in hyperpath.paths_mut() {
                    let mut path_to_sink = vec![];
                    // note the order of operations because we reverse the `Vec` to avoid
                    // insertions, we needed to use `get_supernode`
                    let mut q = path.sink();
                    while q != root_common_target_q_cnode {
                        path_to_sink.push(Edge::new(EdgeKind::Dilute, q));
                        q = self.target_channeler().get_supernode(q).unwrap();
                    }
                    path_to_sink.reverse();
                    path.extend(path_to_root.iter().copied());
                    path.extend(path_to_sink);
                }

                self.make_embedding0(Embedding {
                    program: EmbeddingKind::Node(program_cnode),
                    target_hyperpath: hyperpath,
                })
                .unwrap();
            } else {
                // If the mapping has just a source, then a hyperpath needs to
                // go concentrating to a root node. If anything depending on the source does not
                // also have the root node in common, then there is a disconnection which we
                // easily detect later. There might not be a universal common root node in case
                // of disconnected targets.

                let mut q = hyperpath.source();
                let mut path = Path::new(Ptr::invalid());
                while let Some(tmp) = self.target_channeler().get_supernode(q) {
                    q = tmp;
                    path.push(Edge::new(EdgeKind::Concentrate, q));
                }
                let root_node = q;
                path.sink = root_node;
                hyperpath.push(path);

                self.make_embedding0(Embedding {
                    program: EmbeddingKind::Node(program_cnode),
                    target_hyperpath: hyperpath,
                })
                .unwrap();
            }
        } else {
            // The mapping just has sinks, then a hyper path
            // needs to go from the root node diluting to the sinks, and we also do the root
            // comparison from above

            let mut hyperpath = HyperPath::<QCNode, QCEdge>::new(Ptr::invalid());

            for (i, mapping_target) in mapping.target_sinks.iter().enumerate() {
                let target_sink_p_equiv = mapping_target.target_p_equiv;
                let target_sink_q_cnode = self
                    .target_channeler()
                    .find_channeler_cnode(target_sink_p_equiv)
                    .unwrap();
                let mut path = Path::new(target_sink_q_cnode);

                let mut q = path.sink();
                let mut path_to_sink = vec![Edge::new(EdgeKind::Dilute, q)];
                while let Some(tmp) = self.target_channeler().get_supernode(q) {
                    q = tmp;
                    path_to_sink.push(Edge::new(EdgeKind::Dilute, q));
                }
                let root_node = path_to_sink.pop().unwrap().to;
                if i == 0 {
                    hyperpath.source = root_node;
                } else if hyperpath.source() != root_node {
                    let s = self.debug_mapping(p_mapping);
                    return Err(Error::OtherString(format!(
                        "When trying to find an initial embedding for a program bit that is \
                         mapped to more than one target sink, could not find a common supernode \
                         between the sinks (meaning that the target is like a disconnected graph \
                         and two parts of the mapping are on different parts that are impossible \
                         to route between). The mapping is:\n{s}"
                    )));
                }
                path_to_sink.reverse();
                path.extend(path_to_sink);

                hyperpath.push(path);
            }

            self.make_embedding0(Embedding {
                program: EmbeddingKind::Node(program_cnode),
                target_hyperpath: hyperpath,
            })
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
