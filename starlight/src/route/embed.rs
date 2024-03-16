use awint::awint_dag::triple_arena::Advancer;

use crate::{
    ensemble::{PEquiv, PLNode, Referent},
    route::{Edge, EdgeKind, HyperPath, NodeOrEdge, PMapping, Path, Router},
    Error,
};

#[derive(Debug, Clone)]
pub struct NodeEmbed {
    pub program_node: PEquiv,
    pub hyperpath: HyperPath,
}

impl NodeEmbed {
    pub fn new(program_node: PEquiv, hyperpath: HyperPath) -> Self {
        Self {
            program_node,
            hyperpath,
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
    /// This will create a base hyperpath embedding, returning an error if a
    /// program node has already been associated with an embedding. If
    /// `embed_program_root_into_target_root.is_some()`, then the root supernode
    /// of `program_cnode` on the _program_ side is embedded in the target root
    /// supplied, if not already embedded. If already embedded, it will check
    /// that root nodes agree, otherwise it returns an error.
    fn make_hyperpath_embedding(
        &mut self,
        embed_from: PEquiv,
        hyperpath: HyperPath,
    ) -> Result<(), Error> {
        //FIXME or should this be a thing checked in validation?
        /*let embedding_ref = &mut self
            .program_channeler
            .cnodes
            .get_val_mut(embed_from)
            .unwrap()
            .embedding;
        if embedding_ref.is_some() {
            return Err(Error::OtherString(format!(
                "program node {embed_from:?} is already associated with an embedding, there is \
                 probably a bug in the router",
            )));
        }*/
        self.node_embeddings
            .insert(NodeEmbed::new(embed_from, hyperpath));
        Ok(())
    }

    /// Makes a necessary embedding to express the given mapping.
    fn make_embedding_for_mapping(&mut self, p_mapping: PMapping) -> Result<(), Error> {
        let (program_p_equiv, mapping) = self.mappings.get(p_mapping).unwrap();
        let program_p_equiv = *program_p_equiv;

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
                // other part of the program graph, so we do not need to embed any root
                self.make_hyperpath_embedding(
                    program_p_equiv,
                    HyperPath::new(None, target_source_p_cnode, paths),
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

            // for the program_source, there should be exactly one source from an edge

            // TODO can there be zero? What kind of constant related cases are there?

            let mut program_source = None;
            let mut adv = self
                .program_ensemble
                .backrefs
                .advancer_surject(program_p_equiv.into());
            while let Some(p_ref) = adv.advance(&self.program_ensemble.backrefs) {
                if let Referent::Input(p_lnode) =
                    *self.program_ensemble.backrefs.get_key(p_ref).unwrap()
                {
                    assert!(program_source.is_none());
                    program_source = Some(p_lnode);
                }
            }

            assert!(program_source.is_some());

            self.make_hyperpath_embedding(
                program_p_equiv,
                HyperPath::new(program_source, target_root, paths),
            )
            .unwrap();
        }

        // TODO support custom `CEdge` mappings

        // After much thought, I have come to the conclusion that we should embed all
        // the remaining base nodes at once. The original idea was that after necessary
        // embeddings were made, new embeddings would progress by embedding program
        // nodes higher than the base level, and eventually diluting the program side
        // embeddings at the same time as the target side. This should improve
        // performance as long as we dilute the program side at the right times.
        // However, upon closer inspection there are _many_ things that would be
        // required to make this happen; interlevel embedding management on both sides
        // would have to happen, and we simply don't have known heuristics at this time
        // to do the program side dilution. There would have to be lazy embeddings, etc.
        // I realize that program side dilution can only be delayed by a small constant
        // number of levels and have comparatively little advantage overall. Instead, we
        // are embedding all the base level program nodes at once and giving fine level
        // details more levels to shift around. This means that we don't need a
        // channeling graph for the program side.
        // If we want the same speedup, then there is probably some heuristic algorithm
        // that would allow placement of groups of program nodes at a level lower than
        // the root

        Ok(())
    }

    /// Clears embeddings and uses mappings to make embeddings that are known to
    /// be neccessary for the routing to be possible.
    pub(crate) fn initialize_embeddings(&mut self) -> Result<(), Error> {
        // in case of rerouting we need to clear old embeddings
        self.node_embeddings.clear();
        self.edge_embeddings.clear();

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
}
