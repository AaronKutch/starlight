use std::{
    cmp::max,
    fmt::Write,
    num::{NonZeroU32, NonZeroU64},
};

use awint::{
    awint_dag::triple_arena::{Advancer, Arena, Recast, Recaster},
    Awi,
};

use super::PEmbed;
use crate::{
    awint_dag::smallvec::SmallVec,
    ensemble::{DynamicValue, Ensemble, LNodeKind, PBack, PEquiv, PExternal},
    route::{Channeler, Configurator, PCNode, PConfig},
    Error, OptimizerOptions, SuspendedEpoch,
};

#[derive(Debug, Clone, Copy)]
pub struct ConfigBit {
    p_config: PConfig,
    bit_i: usize,
}

/// The selector can use its configuration bits to arbitrarily select from any
/// of the `SelectorValues` in a power-of-two array.
#[derive(Debug, Clone)]
pub struct SelectorLut {
    inx_config: Vec<ConfigBit>,
}

impl SelectorLut {
    pub fn inx_config(&self) -> &[ConfigBit] {
        &self.inx_config
    }

    pub fn verify_integrity(&self, sources_len: usize) -> Result<(), Error> {
        // TODO
        let pow_len = 1usize << self.inx_config.len();
        if pow_len != sources_len {
            return Err(Error::OtherStr("problem with `SelectorLut` validation"));
        }
        Ok(())
    }
}

/// The arbitrary can use its configuration bits to change into any LUT.
#[derive(Debug, Clone)]
pub struct ArbitraryLut {
    lut_config: Vec<ConfigBit>,
}

impl ArbitraryLut {
    pub fn lut_config(&self) -> &[ConfigBit] {
        &self.lut_config
    }

    pub fn verify_integrity(&self, inx_len: usize) -> Result<(), Error> {
        // TODO
        let pow_len = 1usize << inx_len;
        if self.lut_config.len() != pow_len {
            return Err(Error::OtherStr("problem with `ArbitraryLut` validation"));
        }
        Ok(())
    }
}

/// Used by higher order edges to tell what it is capable of overall
#[derive(Debug, Clone)]
pub struct BulkProperties {
    /// The number of bits that can enter this channel's sources
    pub channel_entry_widths: Vec<usize>,
    /// The number of bits that can exit this channel
    pub channel_exit_widths: Vec<usize>,
    pub lut_bits: usize,
    // this counts the total number of `lvl == 0` subnodes
    pub base_subnodes: usize,
}

impl BulkProperties {
    pub fn new(base_subnodes: usize, lut_bits: usize) -> Self {
        Self {
            channel_entry_widths: vec![],
            channel_exit_widths: vec![],
            lut_bits,
            base_subnodes,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Programmability {
    // We do need this in the end because of cases like the NAND-only FPGA, we could potentially
    // find a way to convert to `ArbitraryLut`s etc, but it would necessitate a lot of structural
    // inefficiency about what subsets of the routing are used to emulate dynamic LUTs, how we
    // leave enough extra routing behind, etc. Instead, the program ensemble equivalences must be
    // manipulated into the `StaticLut`s needed.
    StaticLut(Awi),

    // `DynamicLut`s can go in one of two ways: the table bits all directly connect with unique
    // configurable bits and thus it can behave as an `ArbitraryLut`, or the inx bits directly
    // connect with configurable bits and thus can behave as `SelectorLut`s. Other cases must
    // be reduced to the two or `StaticLut`s
    /// Can behave as an arbitrary lookup table
    ArbitraryLut(ArbitraryLut),
    /// Can behave as an arbitrary selector that multiplexes one of the input
    /// bits to the output
    SelectorLut(SelectorLut),

    /// Bulk behavior
    Bulk(BulkProperties),
}

impl Programmability {
    pub fn debug_strings(&self) -> Vec<String> {
        let mut v = vec![];
        match self {
            Programmability::StaticLut(lut) => v.push(format!("{}", lut)),
            Programmability::ArbitraryLut(arbitrary_lut) => {
                v.push(format!("ArbLut {}", arbitrary_lut.lut_config.len()))
            }
            Programmability::SelectorLut(selector_lut) => {
                v.push(format!("SelLut {}", selector_lut.inx_config.len()))
            }
            Programmability::Bulk(bulk) => {
                let mut s = String::new();
                for (i, width) in bulk.channel_entry_widths.iter().copied().enumerate() {
                    if i == 0 {
                        write!(s, "{}", width).unwrap();
                    } else {
                        write!(s, " {}", width).unwrap();
                    }
                }
                v.push(s);
            }
        }
        v
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Source {
    pub p_cnode: PCNode,
}

impl Recast<PCNode> for Source {
    fn recast<R: Recaster<Item = PCNode>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_cnode.recast(recaster)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sink {
    pub p_cnode: PCNode,
    /// The weight needs to be at least 1 to prevent the algorithm from doing
    /// very bad routes
    pub delay_weight: NonZeroU32,
}

impl Recast<PCNode> for Sink {
    fn recast<R: Recaster<Item = PCNode>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_cnode.recast(recaster)
    }
}

impl Channeler {
    pub fn from_target(
        target_epoch: &SuspendedEpoch,
        configurator: &Configurator,
    ) -> Result<Self, Error> {
        target_epoch.ensemble_mut(|ensemble| Self::new(ensemble, configurator))
    }

    pub fn from_program(target_epoch: &SuspendedEpoch) -> Result<Self, Error> {
        target_epoch.ensemble_mut(|ensemble| Self::new(ensemble, &Configurator::new()))
    }

    pub fn make_cedge(&mut self, source: Source, sink: Sink) {
        self.cnodes
            .get_mut(source.p_cnode)
            .unwrap()
            .sinks
            .push(sink);
        self.cnodes
            .get_mut(sink.p_cnode)
            .unwrap()
            .sources
            .push(source);
    }

    /// Assumes that the ensemble has been optimized
    pub fn new(ensemble: &mut Ensemble, configurator: &Configurator) -> Result<Self, Error> {
        let mut channeler = Channeler::empty();

        struct Tmp {
            p_equiv: PEquiv,
            p_cnode: PCNode,
            driven_by_count: usize,
            driver_of_count: usize,
            config: Option<ConfigBit>,
        }

        // not actual embeddings, we use the `PEmbed` `Ptr`s on the target ensemble to
        // avoid `OrdArena`s when doing this
        let mut tmp_embeddings = Arena::<PEmbed, Tmp>::new();

        // for each equivalence make a temporary embedding, for the `LNode` driven
        // equivalences we make `CNode`s
        for equiv in ensemble.backrefs.vals() {
            // We also check for overdriving here. The target should not have any
            // overdriving, any overdriving for optimizations is temporary and should be
            // fixed by an optimization flag.
            let p_equiv = equiv.p_self_equiv;
            let mut driven_by_count = 0;
            let mut driver_of_count = 0;
            let mut adv = ensemble.backrefs.advancer_surject(p_equiv.into());
            while let Some(p_ref) = adv.advance(&ensemble.backrefs) {
                use crate::ensemble::Referent::*;
                match *ensemble.backrefs.get_key(p_ref).unwrap() {
                    ThisEquiv | ThisStateBit(..) | ThisRNode(_) => (),
                    Input(_) => {
                        driver_of_count += 1;
                    }
                    Driver(_) => {
                        driver_of_count += 1;
                    }
                    ThisLNode(_) => {
                        driven_by_count += 1;
                    }
                    ThisTNode(_) => {
                        driven_by_count += 1;
                    }
                }
            }
            if driven_by_count > 1 {
                return Err(Error::OtherString(format!(
                    "ensemble equivalence {p_equiv:?} has more than one driver (this can be from \
                     a valid equivalence case from certain LUT optimizations, or from a bug), the \
                     target ensemble needs to be optimized with the TODO optimization flag before \
                     being used in a `Channeler`",
                )));
            }

            // this will later be fixed to be more specific
            let p_cnode =
                channeler.make_cnode(vec![], 0, Programmability::Bulk(BulkProperties::new(0, 0)));
            tmp_embeddings.insert(Tmp {
                p_equiv,
                p_cnode,
                driver_of_count,
                driven_by_count,
                config: None,
            });
        }

        // setup temporary back references
        for (p_tmp_embed, tmp) in &tmp_embeddings {
            ensemble
                .backrefs
                .get_val_mut(tmp.p_equiv.into())
                .unwrap()
                .p_embed = Some(p_tmp_embed);
        }
        let translate_backref =
            |ensemble: &Ensemble, tmp_embeddings: &Arena<PEmbed, Tmp>, p_back: PBack| {
                let p_embed = ensemble.backrefs.get_val(p_back).unwrap().p_embed.unwrap();
                tmp_embeddings.get(p_embed).unwrap()
            };
        let translate_backref_mut =
            |ensemble: &Ensemble, tmp_embeddings: &mut Arena<PEmbed, Tmp>, p_back: PBack| {
                let p_embed = ensemble.backrefs.get_val(p_back).unwrap().p_embed.unwrap();
                tmp_embeddings.get_mut(p_embed).unwrap()
            };

        // check that all the configurations point to things that exist, and that the
        // configurations are not being driven. This protects against things like
        // accidentally using the program as the target or if the configurator
        // was used in multiple ensembles
        for (p_config, p_external, _) in &configurator.configurations {
            let p_external = *p_external;
            if let Ok((_, rnode)) = ensemble.notary.get_rnode(p_external) {
                if let Some(bits) = rnode.bits() {
                    for (bit_i, bit) in bits.iter().copied().enumerate() {
                        if let Some(bit) = bit {
                            let tmp = translate_backref_mut(&ensemble, &mut tmp_embeddings, bit);
                            if tmp.driven_by_count > 0 {
                                return Err(Error::OtherString(format!(
                                    "configuration {p_external:#?} is being driven by a `LNode` \
                                     or `TNode`, this is only normally possible if a `LazyAwi` is \
                                     being driven by a `EvalAwi`, which is not allowed on a bit \
                                     marked for router configuration by a `Configurator`"
                                )))
                            }
                            if tmp.driver_of_count > 1 {
                                // TODO
                                return Err(Error::OtherString(format!(
                                    "configuration bit {p_external:#?} is directly driving more \
                                     than one thing, which is currently unsupported by the router"
                                )));
                            }
                            tmp.config = Some(ConfigBit { p_config, bit_i });
                        }
                    }
                } else {
                    return Err(Error::OtherStr(
                        "when creating a target `Channeler` for a router, found that the target \
                         epoch has not been lowered or preferably optimized",
                    ));
                }
            } else {
                return Err(Error::ConfigurationNotFound(p_external))
            }
        }

        let mut max_delay = 1;

        // TODO handle or warn about crazy magnitude difference cases
        let delay_divisor = (max_delay >> 16).saturating_add(1);

        // originally `TNode`s would get their own edges, but it is more important for
        // there to be fewer `CNode`s for the router to deal with (as it will be going
        // over each node many times), and better for each edge input to get its own
        // delay (if there is only one delay there is no way to give the router correct
        // hueristicts when something drives both another thing with small delay and
        // another with huge delay).

        // We should be able to handle `TNode` plain copy cycles or diamonds, I suspect
        // there are valid boilerplate programs that would get simplified into such
        // things. They shouldn't be common, we will just use an unstructured search
        // (besides using visit numbers to prevent nontermination) to first unify all
        // the `CNode`s, then when calculating per-input delays there is another
        // unstructured search from the sink to the source (or just using
        // whatever the value is if we encounter a loop).

        // make sets of equivalences connected by `TNode`s all share the same `CNode`
        let visit = ensemble.next_alg_visit();
        for tnode in ensemble.tnodes.vals() {
            // note that single node `TNode` cycles are handled by the prelude and inner
            // loop arrangement
            let mut nodes = vec![];
            let tmp = translate_backref(&ensemble, &tmp_embeddings, tnode.p_driver);
            let p_forward = tmp.p_cnode;
            let node_visit = &mut ensemble
                .backrefs
                .get_val_mut(tnode.p_driver)
                .unwrap()
                .alg_visit;
            if *node_visit == visit {
                // already done, avoid quadratics
                continue
            }
            *node_visit = visit;
            // will explore from here and handle `p_self`, could have started from either
            // one
            nodes.push(tnode.p_driver);
            while let Some(p_back) = nodes.pop() {
                let tmp = translate_backref_mut(&ensemble, &mut tmp_embeddings, p_back);
                let p_cnode_old = tmp.p_cnode;
                if p_cnode_old != p_forward {
                    // remove cnode, because of cycles we can't have the cnode generation phase
                    // decide to only insert one in the first place, we must remove all but
                    // `p_forward` here.
                    channeler.cnodes.remove(p_cnode_old).unwrap();
                    // set new `CNode`
                    tmp.p_cnode = p_forward;
                }
                let mut adv = ensemble.backrefs.advancer_surject(p_back);
                while let Some(p_ref) = adv.advance(&ensemble.backrefs) {
                    use crate::ensemble::Referent::*;
                    match *ensemble.backrefs.get_key(p_ref).unwrap() {
                        ThisEquiv | ThisLNode(_) | ThisStateBit(..) | Input(_) | ThisRNode(_) => (),
                        ThisTNode(p_tnode) | Driver(p_tnode) => {
                            let tnode = ensemble.tnodes.get(p_tnode).unwrap();
                            let alg_visit = &mut ensemble
                                .backrefs
                                .get_val_mut(tnode.p_driver)
                                .unwrap()
                                .alg_visit;
                            if *alg_visit != visit {
                                *alg_visit = visit;
                                nodes.push(tnode.p_driver);
                            }
                            let alg_visit = &mut ensemble
                                .backrefs
                                .get_val_mut(tnode.p_self)
                                .unwrap()
                                .alg_visit;
                            if *alg_visit != visit {
                                *alg_visit = visit;
                                nodes.push(tnode.p_self);
                            }
                        }
                    }
                }
            }
        }

        // perform a compression step because of the `CNode` removals, want the base
        // layer to be compact
        let cnode_recaster = channeler.cnodes.compress_and_shrink_recaster();
        channeler.recast(&cnode_recaster).unwrap();

        // connect `CNode`s according to `LNode`s
        let mut adv = ensemble.lnodes.advancer();
        while let Some(p_lnode) = adv.advance(&ensemble.lnodes) {
            let mut inputs = SmallVec::<[PBack; 8]>::new();
            let lnode = ensemble.lnodes.get(p_lnode).unwrap();
            let tmp = translate_backref(&ensemble, &tmp_embeddings, lnode.p_self);
            let p_self = tmp.p_cnode;
            let p_cedge = match &lnode.kind {
                LNodeKind::Copy(_) => return Err(Error::OtherStr("the epoch was not optimized")),
                LNodeKind::Lut(inp, awi) => {
                    for input in inp.iter().copied() {
                        let tmp_input = translate_backref(&ensemble, &tmp_embeddings, input);
                        if let Some(_) = tmp_input.config {
                            // TODO transform into canonical cases in earlier pass
                            unreachable!()
                        } else {
                            channeler.make_cedge(
                                Source {
                                    p_cnode: tmp_input.p_cnode,
                                },
                                Sink {
                                    p_cnode: p_self,
                                    delay_weight: NonZeroU32::new(1).unwrap(),
                                },
                            );
                            inputs.push(input);
                        }
                    }
                    channeler.cnodes.get_mut(p_self).unwrap().programmability =
                        Programmability::StaticLut(awi.clone());
                }
                LNodeKind::DynamicLut(inp, lut) => {
                    // figure out if we have a full selector or a full arbitrary
                    let mut config = vec![];
                    for input in inp.iter().copied() {
                        let tmp_input = translate_backref(&ensemble, &tmp_embeddings, input);
                        let p_equiv = ensemble.get_p_equiv(input).unwrap();
                        if let Some(config_bit) = tmp_input.config {
                            // probably also want to transform into one of the two canonical dynamic
                            // cases
                            config.push(config_bit);
                        } else if !config.is_empty() {
                            // has selection configuration but is not full

                            // TODO this should be handled earlier in a optimization pass specific
                            // to the target `Ensemble`
                            unreachable!()
                        } else {
                            let p_cnode = channeler.translate_equiv(p_equiv).unwrap();
                            sources.push(Source {
                                p_cnode,
                                delay_weight: NonZeroU32::new(1).unwrap(),
                            });
                            inputs.push(input);
                        }
                    }
                    if config.is_empty() {
                        // should be a full arbitrary
                        for lut_bit in lut.iter().copied() {
                            if let DynamicValue::Dynam(p) = lut_bit {
                                let p_equiv = ensemble.get_p_equiv(p).unwrap();
                                if let Some(p_config) = configurator.find(p_equiv) {
                                    // probably also want to transform into one of the two canonical
                                    // dynamic cases
                                    config.push(p_config);
                                } else {
                                    // should be arbitrary configuration, should be handled in a
                                    // earlier pass
                                    unreachable!()
                                }
                            } else {
                                // should be arbitrary configuration, should be handled in a earlier
                                // pass
                                unreachable!()
                            }
                        }
                        channeler.make_cedge(
                            sources,
                            p_self,
                            Programmability::ArbitraryLut(ArbitraryLut { lut_config: config }),
                        )
                    } else {
                        // should be a full selector
                        for lut_bit in lut.iter().copied() {
                            match lut_bit {
                                DynamicValue::Dynam(input) => {
                                    let (_, p_cnode) =
                                        channeler.translate_backref(ensemble, input).unwrap();
                                    sources.push(Source {
                                        p_cnode,
                                        delay_weight: NonZeroU32::new(1).unwrap(),
                                    });
                                    inputs.push(input);
                                }
                                // target ensemble is not correct
                                DynamicValue::ConstUnknown | DynamicValue::Const(_) => {
                                    unreachable!()
                                }
                            }
                        }
                        channeler.make_cedge(
                            sources,
                            p_self,
                            Programmability::SelectorLut(SelectorLut { inx_config: config }),
                        )
                    }
                }
            };

            // find delays if there is a `TNode` inbetween the input sink and its source
            for (input_i, input) in inputs.iter().copied().enumerate() {
                let mut total_delay = NonZeroU32::new(1).unwrap();
                let visit = ensemble.next_alg_visit();
                ensemble.backrefs.get_val_mut(input).unwrap().alg_visit = visit;
                let mut next_node = Some(input);
                'outer: while let Some(p_back) = next_node.take() {
                    let mut adv = ensemble.backrefs.advancer_surject(p_back);
                    while let Some(p_ref) = adv.advance(&ensemble.backrefs) {
                        use crate::ensemble::Referent::*;
                        match *ensemble.backrefs.get_key(p_ref).unwrap() {
                            ThisEquiv | ThisLNode(_) | ThisStateBit(..) | Input(_)
                            | ThisRNode(_) => (),
                            Driver(_) => (),
                            // go in the driver direction
                            ThisTNode(p_tnode) => {
                                let tnode = ensemble.tnodes.get(p_tnode).unwrap();
                                let delay_weight = u32::try_from(
                                    tnode
                                        .delay()
                                        .amount()
                                        .wrapping_div(delay_divisor)
                                        .clamp(1, 1 << 16),
                                )
                                .unwrap();
                                total_delay = total_delay.saturating_add(delay_weight);
                                // unstructured, diamonds should be rare
                                let alg_visit = &mut ensemble
                                    .backrefs
                                    .get_val_mut(tnode.p_driver)
                                    .unwrap()
                                    .alg_visit;
                                // this is to prevent nontermination in loops
                                if *alg_visit != visit {
                                    *alg_visit = visit;
                                    next_node = Some(tnode.p_driver);
                                    continue 'outer;
                                }
                            }
                        }
                    }
                }
                // use the weight for the edge
                channeler.cedges.get_mut(p_cedge).unwrap().sources_mut()[input_i].delay_weight =
                    total_delay;
            }
        }

        generate_hierarchy(&mut channeler)?;

        Ok(channeler)
    }
}
