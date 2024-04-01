use std::{cmp::Ordering, fmt::Write, num::NonZeroUsize};

use awint::awint_dag::triple_arena::{Advancer, OrdArena};

use super::{Embedding, PCNode};
use crate::{
    ensemble::{Ensemble, PEquiv, PExternal, Referent},
    route::{route, Channeler, Configurator, PEmbed, PMapping},
    triple_arena::Arena,
    Corresponder, Error, OptimizerOptions, SuspendedEpoch,
};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MappingTarget {
    pub p_external: PExternal,
    pub p_cnodes: Vec<Option<PCNode>>,
}

/// The corresponding program `PExternal` is in the key that this `Mapping`
/// should be uniquely associated with.
#[derive(Debug, Clone)]
pub struct Mapping {
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
    pub(crate) mappings: OrdArena<PMapping, PExternal, Mapping>,
    // routing embedding of part of the program in the target
    pub(crate) embeddings: Arena<PEmbed, Embedding>,
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
    ///
    /// Note that the program is optimized internally with
    /// `union_remove_all_tnodes` (so that external post-simulation will not run
    /// into instant infinite loop problems), however it should be optimized
    /// before use with a higher optimization level.
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
    /// mappings, automatically runs basic optimization with
    /// `union_remove_all_tnodes`
    pub fn new_from_channelers(
        target_epoch: &SuspendedEpoch,
        target_channeler: Channeler,
        configurator: &Configurator,
        program_epoch: &SuspendedEpoch,
    ) -> Self {
        let mut program_ensemble = program_epoch.ensemble(|ensemble| ensemble.clone());
        if !program_ensemble.tnodes.is_empty() {
            // remove all `TNode`s
            program_ensemble
                .optimize(OptimizerOptions::new().union_remove_all_tnodes(true))
                .unwrap();
        }
        Self {
            target_ensemble: target_epoch.ensemble(|ensemble| ensemble.clone()),
            target_channeler,
            configurator: configurator.clone(),
            program_ensemble,
            mappings: OrdArena::new(),
            embeddings: Arena::new(),
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

    pub fn mappings(&self) -> &OrdArena<PMapping, PExternal, Mapping> {
        &self.mappings
    }

    pub fn embeddings(&self) -> &Arena<PEmbed, Embedding> {
        &self.embeddings
    }

    /// Also returns the bitwidth
    fn verify_integrity_of_mapping_target(
        &self,
        mapping_target: &MappingTarget,
    ) -> Result<NonZeroUsize, Error> {
        if let Ok((_, rnode)) = self
            .target_ensemble
            .notary
            .get_rnode(mapping_target.p_external)
        {
            if let Some(bits) = rnode.bits() {
                if bits.len() != mapping_target.p_cnodes.len() {
                    return Err(Error::OtherString(format!(
                        "{mapping_target:#?} bitwidth mismatch"
                    )));
                }
                for i in 0..bits.len() {
                    if bits[i].is_some() != mapping_target.p_cnodes[i].is_some() {
                        return Err(Error::OtherString(format!(
                            "{mapping_target:#?} prune state mismatch"
                        )));
                    }
                }
                Ok(rnode.nzbw())
            } else {
                Err(Error::OtherString(format!(
                    "{mapping_target:#?} rnode is unlowered"
                )))
            }
        } else {
            Err(Error::OtherString(format!(
                "{mapping_target:#?}.target_p_external is invalid"
            )))
        }
    }

    pub fn verify_integrity(&self) -> Result<(), Error> {
        // check substituent validities first
        self.target_ensemble.verify_integrity()?;
        self.target_channeler.verify_integrity()?;
        self.program_ensemble.verify_integrity()?;
        if !self.program_ensemble().tnodes.is_empty() {
            return Err(Error::OtherStr(
                "there are tnodes in the program ensemble after `Router` creation",
            ))
        }
        // mapping validities
        for (p_mapping, program_p_external, mapping) in self.mappings() {
            if let Ok((_, rnode)) = self.program_ensemble.notary.get_rnode(*program_p_external) {
                let common_width = rnode.nzbw();

                if let Some(ref mapping_target) = mapping.target_source {
                    let w = self.verify_integrity_of_mapping_target(mapping_target)?;
                    if w != common_width {
                        return Err(Error::OtherString(format!(
                            "{p_mapping} {mapping:#?} source width mismatch"
                        )))
                    }
                }
                for mapping_target in &mapping.target_sinks {
                    let w = self.verify_integrity_of_mapping_target(mapping_target)?;
                    if w != common_width {
                        return Err(Error::OtherString(format!(
                            "{p_mapping} {mapping:#?} sink width mismatch"
                        )))
                    }
                }
            } else {
                return Err(Error::OtherString(format!(
                    "{p_mapping} {mapping:#?}.program_p_external is invalid"
                )))
            }
        }
        // node embedding validities
        for (p_embedding, embedding) in self.embeddings() {
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
            if !self
                .target_channeler()
                .cnodes
                .contains(hyperpath.target_source)
            {
                return Err(Error::OtherString(format!(
                    "{p_embedding} {embedding:#?}.hyperpath.target_source is invalid"
                )))
            }
            if let Some(program_source) = hyperpath.program_source {
                if !self.program_ensemble().lnodes.contains(program_source) {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:#?}.hyperpath.program_source is invalid"
                    )))
                }
            } else {
                let p_source = hyperpath.target_source;
                if self.target_channeler().cnodes.get(p_source).unwrap().lvl != 0 {
                    return Err(Error::OtherString(format!(
                        "{p_embedding} {embedding:#?} with `program_source == None` has a target \
                         source that is not on the base level"
                    )))
                }
            }
            for path in hyperpath.paths() {
                if let Some(program_sink) = path.program_sink {
                    if let Some(referent) = self.program_ensemble().backrefs.get_key(program_sink) {
                        if !matches!(referent, Referent::Input(_)) {
                            return Err(Error::OtherString(format!(
                                "{p_embedding} {embedding:#?} path program sink does not point to \
                                 `Referent::Input`"
                            )))
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:#?} path program sink is invalid"
                        )))
                    }
                } else {
                    let p_sink = path.target_sink().unwrap_or(hyperpath.target_source);
                    if self.target_channeler().cnodes.get(p_sink).unwrap().lvl != 0 {
                        return Err(Error::OtherString(format!(
                            "{p_embedding} {embedding:#?} path with `program_sink == None` has a \
                             target sink that is not on the base level"
                        )))
                    }
                }
                if !self
                    .target_channeler()
                    .cnodes
                    .contains(path.target_sink().unwrap())
                {
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
                }
            }
            // check path continuity
            for (i, path) in hyperpath.paths().iter().enumerate() {
                let start = self
                    .target_channeler()
                    .cnodes
                    .get(hyperpath.target_source)
                    .unwrap();
                let mut prev = hyperpath.target_source;
                let mut lvl = start.lvl;
                for (j, edge) in path.edges().iter().enumerate() {
                    let other = self.target_channeler().cnodes.get(edge.to).unwrap();
                    match other.lvl.cmp(&lvl) {
                        Ordering::Less => {
                            if other.lvl.wrapping_add(1) != lvl {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i}[{j}] bad level \
                                     difference"
                                )))
                            }
                            lvl = other.lvl;
                            if self.target_channeler().get_supernode(edge.to) != Some(prev) {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i}[{j}] bad edge.to \
                                     supernode"
                                )))
                            }
                        }
                        Ordering::Equal => {
                            // traversal
                            if other.lvl != lvl {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i}[{j}] bad level \
                                     difference"
                                )))
                            }
                            if prev == edge.to {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i}[{j}] traversal in-place"
                                )))
                            }
                        }
                        Ordering::Greater => {
                            if other.lvl != lvl.wrapping_add(1) {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i}[{j}] bad level \
                                     difference"
                                )))
                            }
                            lvl = other.lvl;
                            if self.target_channeler().get_supernode(prev) != Some(edge.to) {
                                return Err(Error::OtherString(format!(
                                    "{p_embedding} {embedding:#?} path {i}[{j}] bad previous \
                                     supernode"
                                )))
                            }
                        }
                    }
                    prev = edge.to;
                }
            }
        }
        Ok(())
    }

    pub fn debug_mapping(&self, p_mapping: PMapping) -> String {
        let (program_p_external, mapping) = self.mappings().get(p_mapping).unwrap();
        let mut s = format!("{p_mapping:?} Mapping {{\nprogram: {program_p_external}\n");
        let rnode = self
            .program_ensemble()
            .notary
            .get_rnode(*program_p_external)
            .unwrap()
            .1;
        if let Some(ref debug_name) = rnode.debug_name {
            writeln!(s, "debug_name: {debug_name}").unwrap();
        }
        if let Some(location) = rnode.location {
            writeln!(s, "{location:#?}").unwrap();
        }
        if let Some(ref source) = mapping.target_source {
            let rnode = self
                .target_ensemble()
                .notary
                .get_rnode(source.p_external)
                .unwrap()
                .1;
            writeln!(
                s,
                "target source {} {:?}",
                source.p_external, source.p_cnodes
            )
            .unwrap();
            if let Some(ref debug_name) = rnode.debug_name {
                writeln!(s, "debug_name: {debug_name}").unwrap();
            }
            if let Some(location) = rnode.location {
                writeln!(s, "{location:#?}").unwrap();
            }
        }
        for (i, sink) in mapping.target_sinks.iter().enumerate() {
            let rnode = self
                .target_ensemble()
                .notary
                .get_rnode(sink.p_external)
                .unwrap()
                .1;
            writeln!(s, "target sink {i} {} {:?}", sink.p_external, sink.p_cnodes).unwrap();
            if let Some(ref debug_name) = rnode.debug_name {
                writeln!(s, "debug_name: {debug_name}").unwrap();
            }
            if let Some(location) = rnode.location {
                writeln!(s, "{location:#?}").unwrap();
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
                         {target:#?} ({len1}) mismatch"
                    )));
                }

                // first get the `p_cnode`s for the target
                let mut p_cnodes = vec![];
                for (bit_i, bit) in target_rnode_bits.iter().copied().enumerate() {
                    if let Some(bit) = bit {
                        let mut adv = self.target_ensemble.backrefs.advancer_surject(bit);
                        while let Some(p_ref) = adv.advance(&self.target_ensemble.backrefs) {
                            match *self.target_ensemble.backrefs.get_key(p_ref).unwrap() {
                                Referent::ThisEquiv
                                | Referent::ThisLNode(_)
                                | Referent::ThisTNode(_)
                                | Referent::ThisStateBit(..)
                                | Referent::Input(_)
                                | Referent::Driver(_) => (),
                                Referent::ThisRNode(p_rnode) => self.target_ensemble,
                            }
                        }
                    } else {
                        p_cnodes.push(None);
                    }
                }

                // insert new mapping target
                let mapping_target = MappingTarget {
                    p_external: target,
                    p_cnodes,
                };
                if let Some(p_map) = self.mappings.find_key(&program_p_equiv) {
                    let mapping = self.mappings.get_val_mut(p_map).unwrap();
                    if is_driver {
                        if mapping.target_source.is_some() {
                            return Err(Error::OtherString(format!(
                                "Tried to map multiple program drivers for the same program \
                                 `RNode` {:#?}, probably called `Router::map_*` twice on the same \
                                 program `LazyAwi`",
                                program
                            )));
                        }
                        mapping.target_source = Some(mapping_target);
                    } else {
                        for target_sink in &mapping.target_sinks {
                            if *target_sink == mapping_target {
                                return Err(Error::OtherString(format!(
                                    "Tried to map multiple program value sinks for the same \
                                     program `RNode` {:#?}, probably called `Router::map_*` twice \
                                     on the same program `EvalAwi`",
                                    program
                                )));
                            }
                        }
                        mapping.target_sinks.push(mapping_target);
                    }
                } else {
                    let mapping = if is_driver {
                        Mapping {
                            target_source: Some(mapping_target),
                            target_sinks: vec![],
                        }
                    } else {
                        Mapping {
                            target_source: None,
                            target_sinks: vec![mapping_target],
                        }
                    };
                    let _ = self.mappings.insert(program, mapping);
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
