use std::{
    cmp::max,
    fmt::Write,
    num::{NonZeroU32, NonZeroU64},
};

use awint::{
    awint_dag::triple_arena::{surject_iterators::SurjectPtrAdvancer, Advancer, Ptr},
    Awi,
};

use crate::{
    awint_dag::smallvec::SmallVec,
    ensemble::{DynamicValue, Ensemble, LNodeKind, PBack},
    route::{
        channel::Referent,
        cnode::{generate_hierarchy, InternalBehavior},
        CNode, Channeler, Configurator, PConfig, PEmbedding,
    },
    utils::SmallSet,
    Error, SuspendedEpoch,
};

/// The selector can use its configuration bits to arbitrarily select from any
/// of the `SelectorValues` in a power-of-two array.
#[derive(Debug, Clone)]
pub struct SelectorLut {
    inx_config: Vec<PConfig>,
}

impl SelectorLut {
    pub fn inx_config(&self) -> &[PConfig] {
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
    lut_config: Vec<PConfig>,
}

impl ArbitraryLut {
    pub fn lut_config(&self) -> &[PConfig] {
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
pub struct ChannelWidths {
    /// The number of bits that can enter this channel's sources
    pub channel_entry_widths: Vec<usize>,
    /// The number of bits that can exit this channel
    pub channel_exit_width: usize,
}

impl ChannelWidths {
    pub fn empty() -> Self {
        Self {
            channel_entry_widths: vec![],
            channel_exit_width: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Programmability {
    TNode,

    StaticLut(Awi),

    // `DynamicLut`s can go in one of two ways: the table bits all directly connect with unique
    // configurable bits and thus it can behave as an `ArbitraryLut`, or the inx bits directly
    // connect with configurable bits and thus can behave as `SelectorLut`s. Other cases must
    // be reduced to the two
    /// Can behave as an arbitrary lookup table
    ArbitraryLut(ArbitraryLut),
    /// Can behave as an arbitrary selector that multiplexes one of the input
    /// bits to the output
    SelectorLut(SelectorLut),

    /// Bulk behavior
    Bulk(ChannelWidths),
}

impl Programmability {
    pub fn debug_strings(&self) -> Vec<String> {
        let mut v = vec![];
        match self {
            Programmability::TNode => v.push("tnode".to_owned()),
            Programmability::StaticLut(lut) => v.push(format!("{}", lut)),
            Programmability::ArbitraryLut(arbitrary_lut) => {
                v.push(format!("ArbLut {}", arbitrary_lut.lut_config.len()))
            }
            Programmability::SelectorLut(selector_lut) => {
                v.push(format!("SelLut {}", selector_lut.inx_config.len()))
            }
            Programmability::Bulk(bulk) => {
                let mut s = String::new();
                for (i, width) in bulk.channel_entry_widths.iter().cloned().enumerate() {
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

/// An edge between channels
#[derive(Debug, Clone)]
pub struct CEdge<PCNode: Ptr> {
    // sources incident to nodes
    sources: Vec<PCNode>,
    // the sink incident to nodes
    sink: PCNode,

    programmability: Programmability,

    pub embeddings: SmallSet<PEmbedding>,

    /// The weight needs to be at least 1 to prevent the algorithm from doing
    /// very bad routes
    pub delay_weight: NonZeroU32,
    /// The lagrangian multiplier, fixed point such that (1 << 16) is 1.0
    pub lagrangian: u32,

    /// Used by algorithms
    pub alg_visit: NonZeroU64,
}

impl<PCNode: Ptr> CEdge<PCNode> {
    pub fn programmability(&self) -> &Programmability {
        &self.programmability
    }

    pub fn sources(&self) -> &[PCNode] {
        &self.sources
    }

    pub fn sink(&self) -> PCNode {
        self.sink
    }

    pub fn sources_mut(&mut self) -> &mut [PCNode] {
        &mut self.sources
    }

    pub fn sink_mut(&mut self) -> &mut PCNode {
        &mut self.sink
    }

    pub fn incidents<F: FnMut(PCNode)>(&self, mut f: F) {
        for source in self.sources() {
            f(*source)
        }
        f(self.sink)
    }

    pub fn incidents_len(&self) -> usize {
        self.sources().len().checked_add(1).unwrap()
    }
}

impl<PCNode: Ptr, PCEdge: Ptr> Channeler<PCNode, PCEdge> {
    /// Given the source and sink incidences (which should point to unique
    /// `ThisCNode`s), this will manage the backrefs
    pub fn make_cedge(
        &mut self,
        sources: &[PCNode],
        sink: PCNode,
        programmability: Programmability,
        delay_weight: NonZeroU32,
    ) -> PCEdge {
        self.cedges.insert_with(|p_self| {
            let mut fixed_sources = vec![];
            for (i, source) in sources.iter().enumerate() {
                fixed_sources.push(
                    self.cnodes
                        .insert_key(*source, Referent::CEdgeIncidence(p_self, Some(i)))
                        .unwrap(),
                );
            }
            let fixed_sink = self
                .cnodes
                .insert_key(sink, Referent::CEdgeIncidence(p_self, None))
                .unwrap();
            CEdge {
                sources: fixed_sources,
                sink: fixed_sink,
                programmability,
                embeddings: SmallSet::new(),
                delay_weight,
                lagrangian: 0,
                alg_visit: NonZeroU64::new(1).unwrap(),
            }
        })
    }

    pub fn from_target(
        target_epoch: &SuspendedEpoch,
        configurator: &Configurator,
    ) -> Result<Self, Error> {
        target_epoch.ensemble(|ensemble| Self::new(ensemble, configurator))
    }

    pub fn from_program(target_epoch: &SuspendedEpoch) -> Result<Self, Error> {
        target_epoch.ensemble(|ensemble| Self::new(ensemble, &Configurator::new()))
    }

    // translate from any ensemble backref to the equivalence backref to the
    // channeler backref
    fn translate(&self, ensemble: &Ensemble, ensemble_backref: PBack) -> (PBack, Option<PCNode>) {
        let p_equiv = ensemble
            .backrefs
            .get_val(ensemble_backref)
            .unwrap()
            .p_self_equiv;
        let p0 = self
            .ensemble_backref_to_channeler_backref
            .find_key(&p_equiv);
        if let Some(p0) = p0 {
            let channeler_p_back = *self
                .ensemble_backref_to_channeler_backref
                .get_val(p0)
                .unwrap();
            (p_equiv, Some(channeler_p_back))
        } else {
            (p_equiv, None)
        }
    }

    /// Assumes that the ensemble has been optimized
    pub fn new(ensemble: &Ensemble, configurator: &Configurator) -> Result<Self, Error> {
        let mut channeler = Self::empty();

        // for each equivalence make a `CNode` with associated `EnsembleBackref`, unless
        // it is one of the configurable bits
        for equiv in ensemble.backrefs.vals() {
            if let Some(p_config) = configurator.configurations.find_key(&equiv.p_self_equiv) {
                let config = configurator.configurations.get_val(p_config).unwrap();
                let p_external = config.p_external;
                let mut input_count = 0;
                // we have a configurable bit, check if it is by itself or can affect other
                // things
                let mut adv = ensemble.backrefs.advancer_surject(equiv.p_self_equiv);
                while let Some(p_ref) = adv.advance(&ensemble.backrefs) {
                    use crate::ensemble::Referent::*;
                    match ensemble.backrefs.get_key(p_ref).unwrap() {
                        ThisEquiv | ThisStateBit(..) | ThisRNode(_) => (),
                        Input(_) => input_count += 1,
                        Driver(_) => {
                            return Err(Error::OtherString(format!(
                                "configuration bit {p_external:#?} is directly driving a temporal \
                                 node, configurations should not have a delay immediately after \
                                 them"
                            )))
                        }
                        ThisLNode(_) | ThisTNode(_) => {
                            return Err(Error::OtherString(format!(
                                "configuration bit {p_external:#?} is driven, which shouldn't \
                                 normally be possible"
                            )))
                        }
                    }
                }
                if input_count > 1 {
                    // TODO have the router interact with the `Ensemble` to find ways to merge
                    // `LNode`s if necessary, there are probably natural cases where a single
                    // `LNode` could be broken up earlier. In the future we may want something more
                    // advanced that can actually handle multiple driver constraints.
                    return Err(Error::OtherString(format!(
                        "configuration bit {p_external:#?} is directly driving more than one \
                         thing, which is currently unsupported by the router"
                    )));
                }
            } else {
                let p_cnode = channeler.make_top_level_cnode(vec![], 0, InternalBehavior::empty());
                let replaced = channeler
                    .ensemble_backref_to_channeler_backref
                    .insert(equiv.p_self_equiv, p_cnode)
                    .1;
                assert!(replaced.is_none());
            }
        }

        // add `CEdge`s according to `LNode`s
        for lnode in ensemble.lnodes.vals() {
            let p_self = channeler.translate(ensemble, lnode.p_self).1.unwrap();
            match &lnode.kind {
                LNodeKind::Copy(_) => return Err(Error::OtherStr("the epoch was not optimized")),
                LNodeKind::Lut(inp, awi) => {
                    let mut v = SmallVec::<[PCNode; 8]>::with_capacity(inp.len());
                    for input in inp {
                        let (p_equiv, p_cnode) = channeler.translate(ensemble, *input);
                        if let Some(_p_config) = configurator.find(p_equiv) {
                            // probably also want to transform into one of the two canonical dynamic
                            // cases
                            todo!()
                        }
                        v.push(p_cnode.unwrap());
                    }
                    channeler.make_cedge(
                        &v,
                        p_self,
                        Programmability::StaticLut(awi.clone()),
                        NonZeroU32::new(1).unwrap(),
                    );
                }
                LNodeKind::DynamicLut(inp, lut) => {
                    // figure out if we have a full selector or a full arbitrary
                    let mut sources = SmallVec::<[PCNode; 8]>::new();
                    let mut config = vec![];
                    for input in inp.iter().copied() {
                        let (p_equiv, p_cnode) = channeler.translate(ensemble, input);
                        if let Some(p_config) = configurator.find(p_equiv) {
                            // probably also want to transform into one of the two canonical dynamic
                            // cases
                            config.push(p_config);
                        } else if !config.is_empty() {
                            // has selection configuration but is not full

                            // TODO this should be handled earlier in a optimization pass specific
                            // to the target `Ensemble`
                            unreachable!()
                        } else {
                            sources.push(p_cnode.unwrap());
                        }
                    }
                    if config.is_empty() {
                        // should be a full arbitrary
                        for lut_bit in lut.iter().copied() {
                            if let DynamicValue::Dynam(p) = lut_bit {
                                let p_equiv = ensemble.backrefs.get_val(p).unwrap().p_self_equiv;
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
                            &sources,
                            p_self,
                            Programmability::ArbitraryLut(ArbitraryLut { lut_config: config }),
                            NonZeroU32::new(1).unwrap(),
                        );
                    } else {
                        // should be a full selector
                        for lut_bit in lut.iter().copied() {
                            match lut_bit {
                                DynamicValue::Dynam(p) => {
                                    let (p_equiv, p_cnode) = channeler.translate(ensemble, p);
                                    if configurator.find(p_equiv).is_some() {
                                        // has selection configuration but also arbitrary
                                        // configuration, should be handled in a earlier pass
                                        unreachable!()
                                    }
                                    sources.push(p_cnode.unwrap());
                                }
                                // target ensemble is not correct
                                DynamicValue::ConstUnknown | DynamicValue::Const(_) => {
                                    unreachable!()
                                }
                            }
                        }
                        channeler.make_cedge(
                            &sources,
                            p_self,
                            Programmability::SelectorLut(SelectorLut { inx_config: config }),
                            NonZeroU32::new(1).unwrap(),
                        );
                    }
                }
            }
        }

        // TODO handle or warn about crazy magnitude difference cases
        let mut max_delay = 1;
        for tnode in ensemble.tnodes.vals() {
            max_delay = max(max_delay, tnode.delay().amount());
        }
        let divisor = (max_delay >> 16).saturating_add(1);

        // add `CEdge`s according to `TNode`s
        for tnode in ensemble.tnodes.vals() {
            let v = [channeler.translate(ensemble, tnode.p_driver).1.unwrap()];

            channeler.make_cedge(
                &v,
                channeler.translate(ensemble, tnode.p_self).1.unwrap(),
                Programmability::TNode,
                NonZeroU32::new(
                    u32::try_from(
                        tnode
                            .delay()
                            .amount()
                            .wrapping_div(divisor)
                            .clamp(1, 1 << 16),
                    )
                    .unwrap(),
                )
                .unwrap(),
            );
        }

        generate_hierarchy(&mut channeler)?;

        Ok(channeler)
    }

    /// Returns `PCNode`s of `p` itself and all nodes directly incident to it
    /// through edges. Node that this modifies the `alg_visit` of local
    /// nodes.
    pub fn related_nodes(&mut self, p: PCNode) -> Vec<PCNode> {
        let related_visit = self.next_alg_visit();
        let cnode = self.cnodes.get_val_mut(p).unwrap();
        cnode.alg_visit = related_visit;
        let mut res = vec![p];
        let mut adv = self.cnodes.advancer_surject(p);
        while let Some(p_referent) = adv.advance(&self.cnodes) {
            if let Referent::CEdgeIncidence(p_cedge, _) = *self.cnodes.get_key(p_referent).unwrap()
            {
                let cedge = self.cedges.get(p_cedge).unwrap();
                cedge.incidents(|p_incident| {
                    let cnode = self.cnodes.get_val_mut(p_incident).unwrap();
                    if cnode.alg_visit != related_visit {
                        cnode.alg_visit = related_visit;
                        res.push(cnode.p_this_cnode);
                    }
                });
            }
        }
        res
    }

    /// Advances over all subnodes of a node
    pub fn advancer_subnodes_of_node(&self, p: PCNode) -> CNodeSubnodeAdvancer<PCNode, PCEdge> {
        CNodeSubnodeAdvancer {
            adv: self.cnodes.advancer_surject(p),
        }
    }
}

pub struct CNodeSubnodeAdvancer<PCNode: Ptr, PCEdge: Ptr> {
    adv: SurjectPtrAdvancer<PCNode, Referent<PCNode, PCEdge>, CNode<PCNode, PCEdge>>,
}

impl<PCNode: Ptr, PCEdge: Ptr> Advancer for CNodeSubnodeAdvancer<PCNode, PCEdge> {
    type Collection = Channeler<PCNode, PCEdge>;
    type Item = PCNode;

    fn advance(&mut self, collection: &Self::Collection) -> Option<Self::Item> {
        while let Some(p_referent) = self.adv.advance(&collection.cnodes) {
            if let Referent::SubNode(p_subnode_ref) =
                *collection.cnodes.get_key(p_referent).unwrap()
            {
                let p_cnode = collection
                    .cnodes
                    .get_val(p_subnode_ref)
                    .unwrap()
                    .p_this_cnode;
                return Some(p_cnode);
            }
        }
        None
    }
}
