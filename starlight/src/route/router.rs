use std::fmt::Write;

use awint::awint_dag::triple_arena::{Advancer, OrdArena};

use crate::{
    ensemble::{Ensemble, PEquiv, PExternal, Referent},
    route::{
        route, Channeler, Configurator, EdgeEmbed, EdgeKind, NodeEmbed, NodeOrEdge, PEdgeEmbed,
        PMapping, PNodeEmbed,
    },
    triple_arena::Arena,
    Corresponder, Error, SuspendedEpoch,
};

#[derive(Debug, Clone)]
pub struct MappingTarget {
    pub target_p_external: PExternal,
    pub target_bit_i: usize,
    pub target_p_equiv: PEquiv,
}

/// The corresponding program `PBack` is in the key that this `Mapping` should
/// be uniquely associated with.
#[derive(Debug, Clone)]
pub struct Mapping {
    pub program_p_external: PExternal,
    pub program_bit_i: usize,
    // Usually, only one of the following has a single `MappingTarget`, but there are cases like
    // copying a bit that all happens in a single program `CNode`, but needs to be mapped to
    // differing target `CNode`s, so in general it can map to a single target source and multiple
    // target sinks.
    pub target_source: Option<MappingTarget>,
    pub target_sinks: Vec<MappingTarget>,
}

#[derive(Debug, Clone)]
pub struct Router {
    pub(crate) target_ensemble: Ensemble,
    pub(crate) target_channeler: Channeler,
    pub(crate) configurator: Configurator,
    pub(crate) program_ensemble: Ensemble,
    // `PEquiv` mapping from program to target
    pub(crate) mappings: OrdArena<PMapping, PEquiv, Mapping>,
    // routing embedding of part of the program in the target
    pub(crate) node_embeddings: Arena<PNodeEmbed, NodeEmbed>,
    pub(crate) edge_embeddings: Arena<PEdgeEmbed, EdgeEmbed>,
    // this should only be set after a successful routing, and be unset the moment any mappings,
    // embeddings, or configurations are changed.
    pub(crate) is_valid_routing: bool,
}

impl Router {
    /// Given the `SuspendedEpoch` of the target, the `Configurator` for the
    /// target, and the `SuspendedEpoch` of the program, this creates a
    /// `Router`.
    ///
    /// # Note
    ///
    /// Currently, the only supported way of using a `Router` is to do these
    /// steps in order:
    ///
    /// 1. The target and program are independently created each by starting an
    ///    `Epoch`, performing the mimicking descriptions, then suspending the
    ///    epoch before starting another one. The target additionally needs to
    ///    specify all of its configurable bits with the `Configurator` so that
    ///    the router can understand what it is allowed to configure.
    ///
    /// 2. A `Configurator` is created to correspond input/output pins on the
    ///    program with input/output pins on the target. One program `EvalAwi`
    ///    can be corresponded with multiple `EvalAwi`s on the target if it
    ///    should be copied, but in every other case the correspondences should
    ///    be one-to-one.
    ///
    /// 3. The router is created from these components. Note that it clones the
    ///    internal `Ensemble`s of the `SuspendedEpoch`s and assumes their
    ///    structure does not change. If you do more mimicking operations to
    ///    them afterwards or do any special modifications beyond `retro_`
    ///    assigning and `eval`uating, the router will not know about their new
    ///    structure and later configures may be wrong.
    ///
    /// 4. `route` is called. If an error is returned then there may be an issue
    ///    with the setup above, a bug with the router itself, or the target may
    ///    simply not have the necessary routability to support the program.
    ///
    /// 5. `get_config` can be used to get the configuration corresponding to a
    ///    target config bit. If you want to simulate the configured target
    ///    however, proceed to the next step.
    ///
    /// 6. The target epoch can be resumed, and when `config_target` is called
    ///    it will set the `LazyAwi`s specified in the configurator. Note that
    ///    if it found a that a bit did not need to be specified, it may set it
    ///    to `Unknown`.
    ///
    /// 7. Now `transpose*` functions can be used with the configurator to
    ///    transpose any desired program operations onto the target.
    pub fn new(
        target_epoch: &SuspendedEpoch,
        configurator: &Configurator,
        program_epoch: &SuspendedEpoch,
    ) -> Result<Self, Error> {
        let target_channeler = Channeler::from_target(target_epoch, configurator)?;
        Ok(Self::new_from_channelers(
            target_epoch,
            target_channeler,
            configurator,
            program_epoch,
        ))
    }

    /// Create the router from externally created `Channeler`s and no automatic
    /// mappings
    pub fn new_from_channelers(
        target_epoch: &SuspendedEpoch,
        target_channeler: Channeler,
        configurator: &Configurator,
        program_epoch: &SuspendedEpoch,
    ) -> Self {
        Self {
            target_ensemble: target_epoch.ensemble(|ensemble| ensemble.clone()),
            target_channeler,
            configurator: configurator.clone(),
            program_ensemble: program_epoch.ensemble(|ensemble| ensemble.clone()),
            mappings: OrdArena::new(),
            node_embeddings: Arena::new(),
            edge_embeddings: Arena::new(),
            is_valid_routing: false,
        }
    }

    pub fn target_ensemble(&self) -> &Ensemble {
        &self.target_ensemble
    }

    pub fn program_ensemble(&self) -> &Ensemble {
        &self.program_ensemble
    }

    pub fn target_channeler(&self) -> &Channeler {
        &self.target_channeler
    }

    pub fn mappings(&self) -> &OrdArena<PMapping, PEquiv, Mapping> {
        &self.mappings
    }

    pub fn node_embeddings(&self) -> &Arena<PNodeEmbed, NodeEmbed> {
        &self.node_embeddings
    }

    pub fn edge_embeddings(&self) -> &Arena<PEdgeEmbed, EdgeEmbed> {
        &self.edge_embeddings
    }

    fn verify_integrity_of_mapping_target(
        &self,
        mapping_target: &MappingTarget,
    ) -> Result<(), Error> {
        if let Ok((_, rnode)) = self
            .target_ensemble
            .notary
            .get_rnode(mapping_target.target_p_external)
        {
            if let Some(bits) = rnode.bits() {
                let mut ok = false;
                if let Some(Some(bit)) = bits.get(mapping_target.target_bit_i) {
                    if let Some(bit) = self.target_ensemble().backrefs.get_val(*bit) {
                        if bit.p_self_equiv == mapping_target.target_p_equiv {
                            ok = true;
                        }
                    }
                }
                if !ok {
                    return Err(Error::OtherString(format!(
                        "{mapping_target:#?} rnode validity issue"
                    )));
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{mapping_target:#?} rnode is unlowered"
                )));
            }
        } else {
            return Err(Error::OtherString(format!(
                "{mapping_target:#?}.target_p_external is invalid"
            )))
        }
        Ok(())
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
        // check substituent validities first
        self.target_ensemble.verify_integrity()?;
        self.target_channeler.verify_integrity()?;
        self.program_ensemble.verify_integrity()?;
        // mapping validities
        for (p_mapping, program_p_equiv, mapping) in self.mappings() {
            if let Ok((_, rnode)) = self
                .program_ensemble
                .notary
                .get_rnode(mapping.program_p_external)
            {
                if let Some(bits) = rnode.bits() {
                    let mut ok = false;
                    if let Some(Some(bit)) = bits.get(mapping.program_bit_i) {
                        if let Some(bit) = self.program_ensemble().backrefs.get_val(*bit) {
                            if bit.p_self_equiv == *program_p_equiv {
                                ok = true;
                            }
                        }
                    }
                    if !ok {
                        return Err(Error::OtherString(format!(
                            "{p_mapping} {mapping:#?} rnode validity issue"
                        )));
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "{p_mapping} {mapping:#?} rnode is unlowered"
                    )));
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{p_mapping} {mapping:#?}.program_p_external is invalid"
                )))
            }

            if let Some(ref mapping_target) = mapping.target_source {
                self.verify_integrity_of_mapping_target(mapping_target)?;
            }
            for mapping_target in &mapping.target_sinks {
                self.verify_integrity_of_mapping_target(mapping_target)?;
            }
        }
        // node embedding validities
        for (p_embedding, embedding) in self.node_embeddings() {
            if !self
                .program_ensemble()
                .backrefs
                .contains(embedding.program_node.into())
            {
                return Err(Error::OtherString(format!(
                    "{p_embedding} {embedding:#?}.program_node is invalid"
                )))
            }
            let hyperpath = &embedding.hyperpath;
            if let Some(program_source) = hyperpath.program_source {
                if !self.program_ensemble().lnodes.contains(program_source) {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:#?}.hyperpath.program_source is invalid"
                    )))
                }
            }
            if !self
                .target_channeler()
                .cnodes
                .contains(hyperpath.target_source)
            {
                return Err(Error::OtherString(format!(
                    "{p_embedding} {embedding:#?}.hyperpath.target_source is invalid"
                )))
            }
            for path in hyperpath.paths() {
                if let Some(program_sink) = path.program_sink {
                    if let Some(referent) = self.program_ensemble().backrefs.get_key(program_sink) {
                        if !matches!(referent, Referent::ThisLNode(_)) {
                            return Err(Error::OtherString(format!(
                                "{p_embedding} {embedding:#?} path program sink does not point to \
                                 `ThisLNode`"
                            )))
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:#?} path program sink is invalid"
                        )))
                    }
                }
                if !self.target_channeler().cnodes.contains(path.target_sink()) {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:#?} path target sink is invalid"
                    )))
                }
                for edge in path.edges() {
                    if !self.target_channeler().cnodes.contains(edge.to) {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:#?} path edge.to is invalid"
                        )))
                    }
                    match edge.kind {
                        EdgeKind::Transverse(q_cedge, source_i) => {
                            if let Some(cedge) = self.target_channeler().cedges.get(q_cedge) {
                                if cedge.sources().get(source_i).is_none() {
                                    return Err(Error::OtherString(format!(
                                        "{p_embedding} {embedding:#?} path sink source_i is out \
                                         of range"
                                    )))
                                }
                            } else {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path edge.kind is invalid"
                                )))
                            }
                        }
                        EdgeKind::Concentrate => (),
                        EdgeKind::Dilute => (),
                    }
                }
            }
            // check path continuity
            for (i, path) in hyperpath.paths().iter().enumerate() {
                let mut q = hyperpath.target_source;
                for (j, edge) in path.edges().iter().enumerate() {
                    match edge.kind {
                        EdgeKind::Transverse(q_cedge, source_i) => {
                            let cedge = self.target_channeler().cedges.get(q_cedge).unwrap();
                            let source = cedge.sources()[source_i].p_cnode;
                            if q != source {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i} source is broken at \
                                     traversal edge {j} {cedge:#?}"
                                )))
                            }
                            q = edge.to;
                        }
                        EdgeKind::Concentrate => {
                            q = self.target_channeler().get_supernode(q).unwrap();
                            if q != edge.to {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i} is broken at \
                                     concentration edge {j}"
                                )))
                            }
                        }
                        EdgeKind::Dilute => {
                            let supernode = self.target_channeler().get_supernode(edge.to).unwrap();
                            if q != supernode {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i} is broken at dilution \
                                     edge {j}"
                                )))
                            }
                            q = edge.to;
                        }
                    }
                }
                if q != path.target_sink() {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:#?} path {i} ending does not match sink"
                    )))
                }
            }
        }
        // edge embedding validities
        for (p_embedding, embedding) in self.edge_embeddings() {
            if !self
                .program_ensemble()
                .lnodes
                .contains(embedding.program_edge)
            {
                return Err(Error::OtherString(format!(
                    "{p_embedding} {embedding:#?}.program_edge is invalid"
                )))
            }
            match embedding.target {
                NodeOrEdge::Node(q_cnode) => {
                    if !self.target_channeler().cnodes.contains(q_cnode) {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:#?}.target is invalid"
                        )))
                    }
                }
                NodeOrEdge::Edge(q_cedge) => {
                    if !self.target_channeler().cedges.contains(q_cedge) {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:#?}.target is invalid"
                        )))
                    }
                }
            }
        }
        Ok(())
    }

    /// Looks through the target ensemble for potential mapping points and their
    /// corresponding channeling nodes
    pub fn debug_potential_map_points(&self, locations: bool, skip_invalid: bool) -> String {
        let mut s = String::new();
        for (p_rnode, p_external, rnode) in self.target_ensemble().notary.rnodes() {
            let mut init = false;
            if !skip_invalid {
                writeln!(
                    s,
                    "{p_rnode:?} {p_external:#?} debug_name: {:?}",
                    rnode.debug_name,
                )
                .unwrap();
                if locations {
                    writeln!(s, "{:#?}", rnode.location).unwrap()
                }
            }
            if let Some(bits) = rnode.bits() {
                for (i, bit) in bits.iter().copied().enumerate() {
                    if let Some(bit) = bit {
                        let bit = self
                            .target_ensemble()
                            .backrefs
                            .get_val(bit)
                            .unwrap()
                            .p_self_equiv;
                        if let Some(q_cnode) = self.target_channeler().translate_equiv(bit) {
                            if skip_invalid && !init {
                                writeln!(
                                    s,
                                    "{p_rnode:?} {p_external:#?} debug_name: {:?}",
                                    rnode.debug_name
                                )
                                .unwrap();
                                if locations {
                                    writeln!(s, "{:#?}", rnode.location).unwrap()
                                }
                                init = true;
                            }
                            writeln!(s, "bit {i} {q_cnode:?}").unwrap();
                        } else if !skip_invalid {
                            writeln!(s, "bit {i} (no corresponding channeling node)").unwrap();
                        }
                    } else if !skip_invalid {
                        writeln!(s, "bit {i} (was dropped or optimized away)").unwrap();
                    }
                }
            } else if !skip_invalid {
                writeln!(s, "(`RNode` never initialized)").unwrap();
            }
        }
        s
    }

    pub fn debug_mapping(&self, p_mapping: PMapping) -> String {
        let (p_equiv, mapping) = self.mappings().get(p_mapping).unwrap();
        let mut s = format!(
            "{p_mapping:?} {p_equiv:#?} Mapping {{\nprogram: {} bit {}\n",
            mapping.program_p_external, mapping.program_bit_i
        );
        let rnode = self
            .program_ensemble()
            .notary
            .get_rnode(mapping.program_p_external)
            .unwrap()
            .1;
        if let Some(ref debug_name) = rnode.debug_name {
            writeln!(s, "debug_name: {debug_name}").unwrap();
        }
        if let Some(location) = rnode.location {
            writeln!(s, "{location:#?}").unwrap();
        }
        if let Some(q_cnode) = self.target_channeler().translate_equiv(*p_equiv) {
            writeln!(s, "{q_cnode:?}").unwrap();
        }
        if let Some(ref source) = mapping.target_source {
            let rnode = self
                .target_ensemble()
                .notary
                .get_rnode(source.target_p_external)
                .unwrap()
                .1;
            writeln!(
                s,
                "target source {} bit {} {}",
                source.target_p_external, source.target_bit_i, source.target_p_equiv
            )
            .unwrap();
            if let Some(ref debug_name) = rnode.debug_name {
                writeln!(s, "debug_name: {debug_name}").unwrap();
            }
            if let Some(location) = rnode.location {
                writeln!(s, "{location:#?}").unwrap();
            }
            if let Some(q_cnode) = self
                .target_channeler()
                .translate_equiv(source.target_p_equiv)
            {
                writeln!(s, "{q_cnode:?}").unwrap();
            }
        }
        for (i, sink) in mapping.target_sinks.iter().enumerate() {
            let rnode = self
                .target_ensemble()
                .notary
                .get_rnode(sink.target_p_external)
                .unwrap()
                .1;
            writeln!(
                s,
                "target sink {i} {} bit {} {}",
                sink.target_p_external, sink.target_bit_i, sink.target_p_equiv
            )
            .unwrap();
            if let Some(ref debug_name) = rnode.debug_name {
                writeln!(s, "debug_name: {debug_name}").unwrap();
            }
            if let Some(location) = rnode.location {
                writeln!(s, "{location:#?}").unwrap();
            }
            if let Some(q_cnode) = self.target_channeler().translate_equiv(sink.target_p_equiv) {
                writeln!(s, "{q_cnode:?}").unwrap();
            }
        }
        writeln!(s, "}}").unwrap();
        s
    }

    pub fn debug_mappings(&self) -> String {
        let mut s = String::new();
        for p_mapping in self.mappings().ptrs() {
            writeln!(s, "{}", self.debug_mapping(p_mapping)).unwrap();
        }
        s
    }

    /// Tell the router what program input bits we want to map to what target
    /// input bits. This is automatically handled by `Router::route()`.
    pub fn map_rnodes(
        &mut self,
        program: PExternal,
        target: PExternal,
        is_driver: bool,
    ) -> Result<(), Error> {
        self.is_valid_routing = false;
        if let Ok((_, program_rnode)) = self.program_ensemble.notary.get_rnode(program) {
            let program_rnode_bits = if let Some(bits) = program_rnode.bits() {
                bits
            } else {
                return Err(Error::OtherString(
                    "when mapping bits, found that the program epoch has not been lowered or \
                     preferably optimized"
                        .to_owned(),
                ));
            };
            if let Ok((_, target_rnode)) = self.target_ensemble.notary.get_rnode(target) {
                let target_rnode_bits = if let Some(bits) = target_rnode.bits() {
                    bits
                } else {
                    return Err(Error::OtherString(
                        "when mapping bits, found that the target epoch has not been lowered or \
                         preferably optimized"
                            .to_owned(),
                    ));
                };
                let len0 = program_rnode_bits.len();
                let len1 = target_rnode_bits.len();
                if len0 != len1 {
                    return Err(Error::OtherString(format!(
                        "when mapping bits, found that the bitwidths of {program:#?} ({len0}) and \
                         {target:#?} ({len1}) differ"
                    )));
                }
                for (bit_i, the_two) in program_rnode_bits
                    .iter()
                    .copied()
                    .zip(target_rnode_bits.iter().copied())
                    .enumerate()
                {
                    match the_two {
                        (Some(program_bit), Some(target_bit)) => {
                            let program_p_equiv = self
                                .program_ensemble
                                .backrefs
                                .get_val(program_bit)
                                .unwrap()
                                .p_self_equiv;
                            let target_p_equiv = self
                                .target_ensemble
                                .backrefs
                                .get_val(target_bit)
                                .unwrap()
                                .p_self_equiv;

                            // insert new mapping target
                            let mapping_target = MappingTarget {
                                target_p_external: target,
                                target_bit_i: bit_i,
                                target_p_equiv,
                            };
                            if let Some(p_map) = self.mappings.find_key(&program_p_equiv) {
                                let mapping = self.mappings.get_val_mut(p_map).unwrap();
                                if is_driver {
                                    if mapping.target_source.is_some() {
                                        return Err(Error::OtherString(format!(
                                            "Tried to map multiple program drivers for the same \
                                             program `RNode` {:#?}, probably called \
                                             `Router::map_*` twice on the same program `LazyAwi`",
                                            program
                                        )));
                                    }
                                    mapping.target_source = Some(mapping_target);
                                } else {
                                    for target_sink in &mapping.target_sinks {
                                        if target_sink.target_p_equiv
                                            == mapping_target.target_p_equiv
                                        {
                                            return Err(Error::OtherString(format!(
                                                "Tried to map multiple program value sinks for \
                                                 the same program `RNode` {:#?}, probably called \
                                                 `Router::map_*` twice on the same program \
                                                 `EvalAwi`",
                                                program
                                            )));
                                        }
                                    }
                                    mapping.target_sinks.push(mapping_target);
                                }
                            } else {
                                let mapping = if is_driver {
                                    Mapping {
                                        program_p_external: program,
                                        program_bit_i: bit_i,
                                        target_source: Some(mapping_target),
                                        target_sinks: vec![],
                                    }
                                } else {
                                    Mapping {
                                        program_p_external: program,
                                        program_bit_i: bit_i,
                                        target_source: None,
                                        target_sinks: vec![mapping_target],
                                    }
                                };
                                let _ = self.mappings.insert(program_p_equiv, mapping);
                            }
                        }
                        (None, None) => (),
                        _ => {
                            // maybe it should just be a no-op? haven't encountered a case yet
                            return Err(Error::OtherString(format!(
                                "when mapping bits {program:#?} and {target:#?}, one or the other \
                                 bits were optimized away inconsistently"
                            )));
                        }
                    }
                }
                Ok(())
            } else {
                Err(Error::OtherString(format!(
                    "when mapping bits, could not find {target:#?} in the target `Ensemble`"
                )))
            }
        } else {
            Err(Error::OtherString(format!(
                "when mapping bits, could not find {program:#?} in the program `Ensemble`"
            )))
        }
    }

    /// Uses the corresponder to find `map_rnodes` points. This is automatically
    /// handled by `Router::route()`.
    pub fn map_rnodes_from_corresponder(
        &mut self,
        corresponder: &Corresponder,
    ) -> Result<(), Error> {
        for (_, p_external, p_correspond) in &corresponder.a {
            if let Ok((_, program_rnode)) = self.program_ensemble().notary.get_rnode(*p_external) {
                // we are oriented around the program side of the correspondence because there
                // should be at most one per correspondence
                let program_p_external = *p_external;
                let is_driver = !program_rnode.read_only();
                let mut target_count = 0;
                let mut adv = corresponder.c.advancer_surject(*p_correspond);
                // skip once
                adv.advance(&corresponder.c);
                while let Some(p_correspond) = adv.advance(&corresponder.c) {
                    let p_meta = *corresponder.c.get_key(p_correspond).unwrap();
                    let target_p_external = *corresponder.a.get_key(p_meta).unwrap();
                    if let Ok((_, target_rnode)) =
                        self.target_ensemble().notary.get_rnode(target_p_external)
                    {
                        if is_driver == target_rnode.read_only() {
                            return Err(Error::OtherString(format!(
                                "in `Router::map_rnodes_from_corresponder()`, it appears that a \
                                 correspondence is between a `LazyAwi` and a `EvalAwi` which \
                                 shouldn't be possible, the two sides were \
                                 {program_p_external:#?} and {target_p_external:#?}"
                            )));
                        }
                        self.map_rnodes(program_p_external, target_p_external, is_driver)?;
                        target_count += 1;
                    } else if self
                        .program_ensemble()
                        .notary
                        .rnodes()
                        .find_key(&target_p_external)
                        .is_some()
                    {
                        // probably a common mistake we should handle specially
                        return Err(Error::CorrespondenceDoubleProgram(
                            program_p_external,
                            target_p_external,
                        ));
                    } else {
                        return Err(Error::CorrespondenceNotFoundInEpoch(target_p_external));
                    }
                }
                if target_count == 0 {
                    return Err(Error::CorrespondenceWithoutTarget(program_p_external));
                }
            } else if self.target_ensemble().notary.get_rnode(*p_external).is_ok() {
                // check that there is at least one program corresponded with this, the other
                // branch will do the other kinds of checks
                let mut program_count = 0;
                let mut adv = corresponder.c.advancer_surject(*p_correspond);
                // skip once
                adv.advance(&corresponder.c);
                while let Some(p_correspond) = adv.advance(&corresponder.c) {
                    let p_meta = *corresponder.c.get_key(p_correspond).unwrap();
                    let p_tmp = *corresponder.a.get_key(p_meta).unwrap();
                    if self.program_ensemble().notary.get_rnode(p_tmp).is_ok() {
                        program_count += 1;
                    }
                }
                if program_count == 0 {
                    return Err(Error::CorrespondenceWithoutProgram(*p_external));
                }
            } else {
                return Err(Error::CorrespondenceNotFoundInEpoch(*p_external));
            }
        }
        Ok(())
    }

    /// Clears any mappings currently registered for this `Router`
    pub fn clear_mappings(&mut self) {
        self.is_valid_routing = false;
        self.mappings.clear();
    }

    /// The same as [Router::route] except that this uses any preexisting manual
    /// mappings.
    pub fn route_without_remapping(&mut self) -> Result<(), Error> {
        self.initialize_embeddings()?;
        for configuration in self.configurator.configurations.vals_mut() {
            configuration.value = None;
        }
        route(self)?;
        self.set_configurations()?;
        self.is_valid_routing = true;
        Ok(())
    }

    /// Routes the program on the target, finding the configuration needed to
    /// match the functionality of target to the program. This resets any
    /// mappings and configurations from previous calls and creates mappings
    /// from the program to the target based on the `corresponder`.
    ///
    /// This function should be called to perform the routing algorithms and
    /// determine how the target can be configured to match the
    /// functionality of the program.
    ///
    /// # Errors
    ///
    /// If the routing is infeasible an error is returned.
    pub fn route(&mut self, corresponder: &Corresponder) -> Result<(), Error> {
        self.clear_mappings();
        self.map_rnodes_from_corresponder(corresponder)?;
        self.route_without_remapping()
    }
}
