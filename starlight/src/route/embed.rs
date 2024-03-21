use std::fmt::Write;

use awint::awint_dag::triple_arena::Advancer;

use crate::{
    ensemble::{PBack, PEquiv, PLNode, Referent},
    route::{
        Edge, EdgeKind, HyperPath, NodeOrEdge, PCNode, PEdgeEmbed, PMapping, PNodeEmbed, Path,
        Router,
    },
    Error,
};

#[derive(Debug, Clone)]
pub struct NodeEmbed {
    pub program_node: PEquiv,
    pub hyperpath: HyperPath,
    pub first_embedded_by: PMapping,
}

impl NodeEmbed {
    pub fn new(program_node: PEquiv, hyperpath: HyperPath, first_embedded_by: PMapping) -> Self {
        Self {
            program_node,
            hyperpath,
            first_embedded_by,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EdgeEmbed {
    pub program_edge: PLNode,
    pub target: NodeOrEdge,
}

impl EdgeEmbed {
    pub fn new(program_edge: PLNode, target: NodeOrEdge) -> Self {
        Self {
            program_edge,
            target,
        }
    }
}

impl Router {
    /// Explore all connected nodes and embed them in the root, on most targets
    /// this will explore the entire target so this should have its own
    /// optimized function
    fn embed_all_connected(
        &mut self,
        common_root: PCNode,
        p_init: PBack,
        embedding_from: PMapping,
    ) -> Result<(), Error> {
        // advance based on the visit number and not on whether the node is already
        // embedded, since `p_init` has already been embedded
        let visit = self.program_ensemble.next_alg_visit();
        let mut front = vec![p_init];
        while let Some(p_start) = front.pop() {
            let node = self.program_ensemble.backrefs.get_val_mut(p_start).unwrap();
            if node.alg_visit == visit {
                continue
            }
            node.alg_visit = visit;
            let mut program_source = None;
            let mut paths = vec![];

            // there are no `TNode`s to worry about for the program ensemble
            let mut adv = self.program_ensemble.backrefs.advancer_surject(p_start);
            while let Some(p_ref) = adv.advance(&self.program_ensemble.backrefs) {
                match self.program_ensemble.backrefs.get_key(p_ref) {
                    Some(Referent::ThisLNode(p_lnode)) => {
                        assert!(program_source.is_none());
                        program_source = Some(*p_lnode);
                        let lnode = self.program_ensemble.lnodes.get_mut(*p_lnode).unwrap();
                        if lnode.p_edge_embed.is_none() {
                            lnode.p_edge_embed =
                                Some(self.edge_embeddings.insert(EdgeEmbed::new(
                                    *p_lnode,
                                    NodeOrEdge::Node(common_root),
                                )));
                        }
                        lnode.inputs(|p| {
                            front.push(p);
                        });
                    }
                    Some(Referent::Input(p_lnode)) => {
                        let lnode = self.program_ensemble.lnodes.get_mut(*p_lnode).unwrap();
                        if lnode.p_edge_embed.is_none() {
                            lnode.p_edge_embed =
                                Some(self.edge_embeddings.insert(EdgeEmbed::new(
                                    *p_lnode,
                                    NodeOrEdge::Node(common_root),
                                )));
                        }
                        lnode.incidents(|p| {
                            front.push(p);
                        });
                        paths.push(Path::new(Some(p_ref), vec![]));
                    }
                    _ => (),
                }
            }

            let node = self.program_ensemble.backrefs.get_val_mut(p_start).unwrap();
            if node.p_node_embed.is_none() {
                node.p_node_embed = Some(self.node_embeddings.insert(NodeEmbed::new(
                    node.p_self_equiv,
                    HyperPath::new(program_source, common_root, paths),
                    embedding_from,
                )));
            } else {
                // an embedding should fully explore its region, we shouldn't encounter this
                unreachable!()
            }
        }
        Ok(())
    }

    /// This assumes we are in the single initial embedding pass that embeds
    /// every thing in target root nodes. This will create a base hyperpath
    /// embedding, returning an error if `program_node` has been already
    /// embedded with an incompatible embedding. `common_root` should be `None`
    /// only in the special program copy case. `hyperpath` should not have a
    /// `program_source`.
    fn make_hyperpath_embedding(
        &mut self,
        program_node: PEquiv,
        hyperpath: HyperPath,
        common_root: Option<PCNode>,
        embedding_from: PMapping,
    ) -> Result<(), Error> {
        let node = self
            .program_ensemble
            .backrefs
            .get_val(program_node.into())
            .unwrap();
        if let Some(p_node_embed) = node.p_node_embed {
            if let Some(common_root) = common_root {
                // because of the all-connected-nodes exploration that always runs after any
                // initial embedding call, if the new embedding would have inconsistent roots we
                // can detect immediately it here
                let embedding = self.node_embeddings.get(p_node_embed).unwrap();

                // If this was from an exploration, then all should share a common root.
                let mut all_match = true;
                if embedding.hyperpath.target_source != common_root {
                    all_match = false;
                } else {
                    for path in embedding.hyperpath.paths() {
                        if path.target_sink() != common_root {
                            all_match = false;
                        }
                    }
                }
                if all_match {
                    let embedding = self.node_embeddings.get_mut(p_node_embed).unwrap();
                    // new `LNode` drivers not expected to be handled
                    assert!(hyperpath.program_source.is_none());
                    if embedding.hyperpath.program_source.is_some() {
                        // the new `hyperpath` source shouldn't be coming up from the base level
                        // since it is being driven
                        assert_eq!(common_root, hyperpath.target_source);
                    }
                    if hyperpath.target_source != common_root {
                        // preexisting paths being driven from the root need to have the
                        // concentration path prepended on
                        todo!()
                    }
                    // add the `path.program_sink() == None` necessary embeddings onto the
                    // compatible embedding which may be also driving `LNode`s in addition to being
                    // read by a mapping
                    for path in hyperpath.paths() {
                        embedding.hyperpath.push(path.clone());
                    }
                    // the connected region of the program connected to this embedding was already
                    // explored
                    Ok(())
                } else {
                    // If there is a bad mapping, it is more likely to be a singular mistake that
                    // may end up being the first setter of a region of embeddings. This is why the
                    // node embeddings have `first_embedded_by` so that we can display it and the
                    // embedding we were trying to make

                    let s0 = self.debug_mapping(embedding.first_embedded_by);
                    let s1 = self.debug_mapping(embedding_from);
                    Err(Error::OtherString(format!(
                        "When trying to find initial embeddings for program bits, found two bits \
                         that are connected in the program that cannot be connected between their \
                         mapped locations on the target, meaning that routing between then is \
                         impossible regardless of other constraints and that a complete routing \
                         is therefore impossible. One mapping involved is:\n{s0}\nand the other \
                         mapping involved is:\n{s1}"
                    )))
                }
            } else {
                Err(Error::OtherStr(
                    "the same plain copy program node is being embedded a second time, which \
                     shouldn't be possible if hereditary mapping is enforced, this may be a bug \
                     with the router",
                ))
            }
        } else {
            // initial embedding of this connected part of the program
            self.node_embeddings
                .insert(NodeEmbed::new(program_node, hyperpath, embedding_from));

            if let Some(common_root) = common_root {
                self.embed_all_connected(common_root, program_node.into(), embedding_from)
            } else {
                // else is simple copy case that can't trigger other embeddings
                Ok(())
            }
        }
    }

    /// Makes a necessary embedding to express the given mapping.
    fn make_embedding_for_mapping(&mut self, p_mapping: PMapping) -> Result<(), Error> {
        let (program_p_equiv, mapping) = self.mappings.get(p_mapping).unwrap();
        let program_p_equiv = *program_p_equiv;

        // TODO support custom `CEdge` mappings

        // remember that `*_root` does not necessarily mean a global root, just a common
        // root

        if let Some(ref source_mapping_target) = mapping.target_source {
            // find the corresponding `QCNode` for the source
            let target_source_p_equiv = source_mapping_target.target_p_equiv;
            let target_source_p_cnode = self
                .target_channeler
                .translate_equiv(target_source_p_equiv)
                .unwrap();

            // create path from source to root
            let mut q = target_source_p_cnode;
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
                    let target_sink_p_cnode = self
                        .target_channeler
                        .translate_equiv(target_sink_p_equiv)
                        .unwrap();

                    let mut q = target_sink_p_cnode;
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
                    paths.push(Path::new(None, combined_path));
                }

                // this case is just a single program node not bound to any
                // other part of the program graph and doesn't trigger other embeddings
                self.make_hyperpath_embedding(
                    program_p_equiv,
                    HyperPath::new(None, target_source_p_cnode, paths),
                    None,
                    p_mapping,
                )
                .unwrap();
            } else {
                // If the mapping has just a source, then for every program sink there needs to
                // be a hyperpath concentrating to the root node on the target side.

                let mut paths = vec![];

                let mut adv = self
                    .program_ensemble
                    .backrefs
                    .advancer_surject(program_p_equiv.into());
                while let Some(p_ref) = adv.advance(&self.program_ensemble.backrefs) {
                    if let Referent::Input(_) =
                        *self.program_ensemble.backrefs.get_key(p_ref).unwrap()
                    {
                        paths.push(Path::new(Some(p_ref), path_to_root.clone()));
                    }
                }
                self.make_hyperpath_embedding(
                    program_p_equiv,
                    HyperPath::new(None, target_source_p_cnode, paths),
                    Some(target_root),
                    p_mapping,
                )
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
                    .target_channeler
                    .translate_equiv(target_sink_p_equiv)
                    .unwrap();

                let mut q = target_sink_q_cnode;
                while let Some(tmp) = self.target_channeler().get_supernode(q) {
                    q = tmp;
                }
                q
            };

            let mut paths = vec![];
            for mapping_target in &mapping.target_sinks {
                let target_sink_p_equiv = mapping_target.target_p_equiv;
                let target_sink_q_cnode = self
                    .target_channeler()
                    .translate_equiv(target_sink_p_equiv)
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
                paths.push(Path::new(None, path_to_sink));
            }

            // `make_hyperpath_embedding` will handle the `LNode`s

            self.make_hyperpath_embedding(
                program_p_equiv,
                HyperPath::new(None, target_root, paths),
                Some(target_root),
                p_mapping,
            )
            .unwrap();
        }

        Ok(())
    }

    /// This is public for debugging. Clears embeddings and uses mappings to
    /// make embeddings that are known to be neccessary for the routing to
    /// be possible.
    pub fn initialize_embeddings(&mut self) -> Result<(), Error> {
        // in case of rerouting we need to clear old embeddings
        self.node_embeddings.clear();
        self.edge_embeddings.clear();
        for node in self.program_ensemble.backrefs.vals_mut() {
            node.p_node_embed = None;
        }
        for node in self.program_ensemble.lnodes.vals_mut() {
            node.p_edge_embed = None;
        }

        // After much thought, I have come to the conclusion that we should embed all
        // the base nodes (excluding unused program connected regions) and not just the
        // necessary nodes from the mappings. The original idea was that after
        // necessary embeddings were made, new embeddings would progress by
        // embedding program nodes higher than the base level, and eventually
        // diluting the program side embeddings at the same time as the target
        // side. This should improve performance as long as we dilute the
        // program side at the right times. However, upon closer inspection
        // there are _many_ things that would be required to make this happen;
        // interlevel embedding management on both sides would have to happen,
        // and we simply don't have known heuristics at this time to do the
        // program side dilution. There would have to be lazy embeddings, etc. I
        // realize that program side dilution can only be delayed by a small
        // constant number of levels and have comparatively little advantage
        // overall. Instead, we are embedding all the base level program nodes
        // at once and giving fine level details more levels to shift around.
        // This means that we don't need a channeling graph for the program
        // side. If we want the same speedup, then there is probably some
        // heuristic algorithm that would allow placement of groups of program
        // nodes at a level lower than the root

        // Mappings will stay static because they are used for figuring out translating
        // program IO to target IO. Embeddings will represent bulk programmings of the
        // hierarchy. However, we know that the mappings correspond to some embeddings
        // that are absolutely necessary for the routing to be possible, so we can start
        // by making those embeddings.
        let mut adv = self.mappings.advancer();
        while let Some(p_mapping) = adv.advance(&self.mappings) {
            self.make_embedding_for_mapping(p_mapping)?;
        }

        Ok(())
    }

    pub fn debug_node_embedding(&self, p_node_embed: PNodeEmbed) -> String {
        let node_embed = self.node_embeddings().get(p_node_embed).unwrap();
        format!("{node_embed:#?}")
    }

    pub fn debug_edge_embedding(&self, p_edge_embed: PEdgeEmbed) -> String {
        let edge_embed = self.edge_embeddings().get(p_edge_embed).unwrap();
        format!("{edge_embed:#?}")
    }

    pub fn debug_all_embeddings(&self) -> String {
        let mut s = String::new();
        for p_node_embed in self.node_embeddings().ptrs() {
            writeln!(s, "{}\n", self.debug_node_embedding(p_node_embed)).unwrap();
        }
        for p_edge_embed in self.edge_embeddings().ptrs() {
            writeln!(s, "{}\n", self.debug_edge_embedding(p_edge_embed)).unwrap();
        }
        s
    }
}
