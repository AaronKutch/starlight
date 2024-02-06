use awint::awint_dag::triple_arena::{ptr_struct, Advancer, Ptr};

use super::{
    router::{PCEdge, PCNode, PMapping, QCEdge, QCNode},
    Edge, EdgeKind, Path,
};
use crate::{
    route::{HyperPath, Router},
    Error,
};

ptr_struct!(PEmbedding);

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

    /// Makes a minimal embedding to express the given mapping.
    fn make_embedding1(&mut self, p_mapping: PMapping) -> Result<(), Error> {
        let (program_p_equiv, mapping) = self.mappings.get(p_mapping).unwrap();
        let program_p_equiv = *program_p_equiv;
        let program_cnode = self
            .program_channeler()
            .find_channeler_cnode(program_p_equiv)
            .unwrap();

        if mapping.target_source.is_some() && (!mapping.target_sinks.is_empty()) {
            // If a mapping has both a source and sinks, then we need an embedding of the
            // program cnode that embeds in a target cnode that can cover all the sources
            // and the sinks. The embedding then has a hyperpath that connects the sources
            // and sinks.

            // we are dealing with the single program node copying mapping case, which does
            // not interact with anything else directly so we only deal with the common
            // supernode of our source and sinks

            // find the corresponding `QCNode` for the source
            let target_source_p_equiv = mapping.target_source.as_ref().unwrap().target_p_equiv;
            let target_source_q_cnode = self
                .target_channeler()
                .find_channeler_cnode(target_source_p_equiv)
                .unwrap();

            // begin constructing hyperpath for the embedding
            let mut hyperpath = HyperPath::<QCNode, QCEdge>::new(target_source_q_cnode);

            // begin finding the common target cnode
            let mut root_common_target_q_cnode = target_source_q_cnode;

            // do the same for the sinks
            for (i, mapping_target) in mapping.target_sinks.iter().enumerate() {
                let target_sink_p_equiv = mapping_target.target_p_equiv;
                let target_sink_q_cnode = self
                    .target_channeler()
                    .find_channeler_cnode(target_sink_p_equiv)
                    .unwrap();
                let path = Path::<QCNode, QCEdge>::new(target_sink_q_cnode);
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
                         onto a target), could not find a common supernode between the source and \
                         sink {i} (meaning that the target is like a disconnected graph and two \
                         parts of the mapping are on different parts that are impossible to route \
                         between). The mapping is:\n{s}\nThe `CNodes` are \
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
                path.extend(path_to_sink.iter().copied());
            }

            self.make_embedding0(Embedding {
                program: EmbeddingKind::Node(program_cnode),
                target_hyperpath: hyperpath,
            })
            .unwrap();
        } else {
            // If the mapping has just a source, then a hyper path needs to go concentrating
            // to a root node. If the mapping just has sinks, then a hyper path
            // needs to go from the root node diluting to the sinks.

            // just find the root from the current location, embed the root, but if it is
            // already embedded check it corresponds with the same program root, otherwise
            // there must be a disconnection
            todo!()
        }

        // TODO support custom `CEdge` mappings

        Ok(())
    }

    pub(crate) fn initialize_embeddings(&mut self) -> Result<(), Error> {
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
