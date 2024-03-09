use awint::awint_dag::triple_arena::{Advancer, Ptr};

use crate::{
    route::{
        Edge, EdgeKind, HyperPath, NodeOrEdge, PCEdge, PCNode, PMapping, Path, QCEdge, QCNode,
        Router,
    },
    Error,
};

#[derive(Debug, Clone)]
pub struct NodeEmbed<PCNode: Ptr, QCNode: Ptr> {
    pub program_cnode: PCNode,
    pub target_cnode: QCNode,
}

#[derive(Debug, Clone)]
pub struct EdgeEmbed<PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    pub program_edge: PCEdge,
    pub target: NodeOrEdge<QCNode, QCEdge>,
}

#[derive(Debug, Clone)]
pub enum EmbeddingKind<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    /// A `CNode` needs to have its value spread across multiple target nodes
    HyperPath(HyperPath<PCNode, PCEdge, QCNode, QCEdge>),
    /// A one-to-one mapping of nodes, used for keeping track of connections in
    /// supernodes
    NodeEmbed(NodeEmbed<PCNode, QCNode>),
    /// A one-to-one mapping of a program edge onto a programmable target edge,
    /// which can happen at the base level or higher up
    EdgeEmbed(EdgeEmbed<PCEdge, QCNode, QCEdge>),
}

#[derive(Debug, Clone)]
pub struct Embedding<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> {
    pub kind: EmbeddingKind<PCNode, PCEdge, QCNode, QCEdge>,
}

impl<PCNode: Ptr, PCEdge: Ptr, QCNode: Ptr, QCEdge: Ptr> Embedding<PCNode, PCEdge, QCNode, QCEdge> {
    pub fn new_hyperpath(hyperpath: HyperPath<PCNode, PCEdge, QCNode, QCEdge>) -> Self {
        Self {
            kind: EmbeddingKind::HyperPath(hyperpath),
        }
    }

    pub fn new_node_embed(program_cnode: PCNode, target_cnode: QCNode) -> Self {
        Self {
            kind: EmbeddingKind::NodeEmbed(NodeEmbed {
                program_cnode,
                target_cnode,
            }),
        }
    }

    // TODO after the routing algorithm settles, the places where these are called
    // should mostly be replaced by passing around the path and maybe using
    // `Option::take` internally.

    pub fn hyperpath(&self) -> Option<&HyperPath<PCNode, PCEdge, QCNode, QCEdge>> {
        if let EmbeddingKind::HyperPath(ref hyperpath) = self.kind {
            Some(hyperpath)
        } else {
            None
        }
    }

    pub fn hyperpath_mut(&mut self) -> Option<&mut HyperPath<PCNode, PCEdge, QCNode, QCEdge>> {
        if let EmbeddingKind::HyperPath(ref mut hyperpath) = self.kind {
            Some(hyperpath)
        } else {
            None
        }
    }
}

impl Router {
    /// This will create a base hyperpath embedding, returning an error if a
    /// program node has already been associated with an embedding. If
    /// `embed_program_root_into_target_root.is_some()`, then the root supernode
    /// of `program_cnode` on the _program_ side is embedded in the target root
    /// supplied, if not already embedded. If already embedded, it will check
    /// that root nodes agree, otherwise it returns an error.
    fn make_hyperpath_embedding(
        &mut self,
        hyperpath: HyperPath<PCNode, PCEdge, QCNode, QCEdge>,
    ) -> Result<(), Error> {
        let embedding_ref = &mut self
            .program_channeler
            .cnodes
            .get_val_mut(hyperpath.program_source)
            .unwrap()
            .embedding;
        if embedding_ref.is_some() {
            return Err(Error::OtherString(format!(
                "program node {:?} is already associated with an embedding, there is probably a \
                 bug in the router",
                hyperpath.program_source
            )));
        }
        let p_embedding = self.embeddings.insert(Embedding::new_hyperpath(hyperpath));
        *embedding_ref = Some(p_embedding);
        Ok(())
    }

    /// This will do nothing without error if the root is already embedded and
    /// that the embedding is a node embedding with target equal to
    /// `target_root`.
    fn make_root_embedding(
        &mut self,
        program_root: PCNode,
        target_root: QCNode,
    ) -> Result<(), Error> {
        let embedding_ref = &mut self
            .program_channeler
            .cnodes
            .get_val_mut(program_root)
            .unwrap()
            .embedding;
        if let Some(p_embedding) = embedding_ref {
            if let EmbeddingKind::NodeEmbed(ref node_embed) =
                self.embeddings.get(*p_embedding).unwrap().kind
            {
                if target_root != node_embed.target_cnode {
                    // TODO need to test that the error works and that we have a dedicated error
                    // showing the two mappings that contradict, may need to store a `PMapping`
                    // in the embedding
                    return Err(Error::OtherString(format!(
                        "when checking that a common root channel node is equal to a previously \
                         set embedding root, found that {:?} and {:?} are unequal. This probably \
                         means that the target is not fully connected, and two parts of the \
                         program have been corresponded on two parts of the target that cannot \
                         reach one another, meaning that the routing is impossible.",
                        target_root, node_embed.target_cnode
                    )));
                }
            } else {
                return Err(Error::OtherStr(
                    "when checking an already embedded root, found it to be embedded with an \
                     unexpected kind of embedding, there is probably a bug in the router",
                ));
            }
        } else {
            let p_embedding = self
                .embeddings
                .insert(Embedding::new_node_embed(program_root, target_root));
            *embedding_ref = Some(p_embedding);
        }
        Ok(())
    }

    /// Makes a necessary embedding to express the given mapping.
    fn make_embedding_for_mapping(&mut self, p_mapping: PMapping) -> Result<(), Error> {
        let (program_p_equiv, mapping) = self.mappings.get(p_mapping).unwrap();
        let program_p_equiv = *program_p_equiv;
        let program_cnode = self
            .program_channeler()
            .find_channeler_cnode(program_p_equiv)
            .unwrap();

        // remember that `*_root` does not necessarily mean a global root, just a common
        // root

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
            let target_root = q;
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
                    if q != target_root {
                        let s = self.debug_mapping(p_mapping);
                        return Err(Error::OtherString(format!(
                            "When trying to find an initial embedding for a program bit that is \
                             mapped to both a target source and one or more target sinks (which \
                             occurs when mapping a trivial copy operation in the program directly \
                             onto a target), could not find a common supernode between the source \
                             and sink {i} (meaning that the target is like a disconnected graph \
                             and two parts of the mapping are on different parts that are \
                             impossible to route between). The mapping is:\n{s}\nThe roots are \
                             {target_root}, {q}"
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

                    // copy as itself
                    paths.push(Path::new(NodeOrEdge::Node(program_cnode), combined_path));
                }

                // this case is just a single program node not bound to any
                // other part of the program graph, so we do not need to embed any root
                self.make_hyperpath_embedding(HyperPath::new(
                    program_cnode,
                    target_source_q_cnode,
                    paths,
                ))
                .unwrap();
            } else {
                // If the mapping has just a source, then a hyperpath needs to
                // go concentrating to a root node. If anything depending on the source does not
                // also have the root node in common, then there is a disconnection which is
                // detected when embedding the root now or later call.

                let mut p = program_cnode;
                while let Some(tmp) = self.program_channeler().get_supernode(p) {
                    p = tmp;
                }
                let program_root = p;
                self.make_root_embedding(program_root, target_root)?;
                self.make_hyperpath_embedding(HyperPath::new(
                    program_cnode,
                    target_source_q_cnode,
                    vec![Path::new(NodeOrEdge::Node(program_root), path_to_root)],
                ))
                .unwrap();
            }
        } else {
            // The mapping just has sinks, then a hyper path
            // needs to go from the root node diluting to the sinks, and we also do the root
            // comparison from above

            let target_root = {
                let mapping_target = mapping.target_sinks.first().unwrap();
                let target_sink_p_equiv = mapping_target.target_p_equiv;
                let target_sink_q_cnode = self
                    .target_channeler()
                    .find_channeler_cnode(target_sink_p_equiv)
                    .unwrap();

                let mut q = target_sink_q_cnode;
                while let Some(tmp) = self.target_channeler().get_supernode(q) {
                    q = tmp;
                }
                q
            };

            let mut p = program_cnode;
            while let Some(tmp) = self.program_channeler().get_supernode(p) {
                p = tmp;
            }
            let program_root = p;

            let mut paths = vec![];
            for mapping_target in &mapping.target_sinks {
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
                let root_node = q;
                path_to_sink.pop().unwrap();
                if target_root != root_node {
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
                paths.push(Path::new(NodeOrEdge::Node(program_root), path_to_sink));
            }

            self.make_root_embedding(program_root, target_root)?;
            self.make_hyperpath_embedding(HyperPath::new(program_cnode, target_root, paths))
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
            let _ = cnode.embedding.take();
        }
        for cedge in self.program_channeler.cedges.vals_mut() {
            let _ = cedge.embedding.take();
        }

        // Mappings will stay static because they are used for figuring out translating
        // program IO to target IO. Embeddings will represent bulk programmings of the
        // hierarchy. However, we know that the mappings correspond to some embeddings
        // that are absolutely necessary for the routing to be possible, so we can start
        // by making those embeddings.
        let mut adv = self.mappings.advancer();
        while let Some(p_mapping) = adv.advance(&self.mappings) {
            self.make_embedding_for_mapping(p_mapping).unwrap()
        }
        Ok(())
    }
}
