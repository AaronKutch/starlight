use std::{fmt::Write, num::NonZeroUsize};

use awint::{
    awint_dag::triple_arena::{surject_iterators::SurjectPtrAdvancer, Advancer, OrdArena, Ptr},
    Awi,
};

use crate::{
    awint_dag::smallvec::SmallVec,
    ensemble::{DynamicValue, Ensemble, LNodeKind, PBack},
    misc::SmallSet,
    route::{
        channel::Referent,
        cnode::{generate_hierarchy, InternalBehavior},
        CNode, Channeler, Configurator, PEmbedding,
    },
    triple_arena::ptr_struct,
    Error, SuspendedEpoch,
};

#[derive(Debug, Clone, Copy)]
pub enum SelectorValue {
    Dynam,
    ConstUnknown,
    Const(bool),
}

/// The `Vec<PBack>` has the configuration indexes, the two `Awi`s
/// have bitwidths equal to `1 << len` where `len` is the number of indexes
///
/// Logically, the selector selects from the power-of-two array which may have
/// constants and unused `ConstUnknown`s in addition to the routes for dynamics.
/// The incidents only include the dynamics, and thus we need to know where the
/// gaps are. The `Awi` is broken up into pairs of bits used to indicate the
/// following states in incrementing order: dynamic, const unknown, const zero,
/// const one
#[derive(Debug, Clone)]
pub struct SelectorLut {
    awi: Awi,
    v: Vec<PBack>,
}

impl SelectorLut {
    pub fn get_selector_value(&self, bit_i: usize) -> SelectorValue {
        debug_assert!(bit_i < (isize::MAX as usize));
        let start = bit_i << 1;
        debug_assert!((bit_i << 1) < self.awi.bw());
        match (
            self.awi.get(start).unwrap(),
            self.awi.get(start.wrapping_add(1)).unwrap(),
        ) {
            (false, false) => SelectorValue::Dynam,
            (true, false) => SelectorValue::ConstUnknown,
            (b, true) => SelectorValue::Const(b),
        }
    }

    pub fn verify_integrity(&self, sources_len: usize) -> Result<(), Error> {
        // TODO
        let pow_len = 1usize << self.v.len();
        if pow_len.checked_mul(2).unwrap() != self.awi.bw() {
            return Err(Error::OtherStr("problem with `SelectorLut` validation"));
        }
        let mut dynam_len = 0;
        for i in 0..pow_len {
            if let SelectorValue::Dynam = self.get_selector_value(i) {
                dynam_len += 1;
            }
        }
        if dynam_len != sources_len {
            return Err(Error::OtherStr("problem with `SelectorLut` validation"));
        }
        Ok(())
    }
}

/// Used by higher order edges to tell what it is capable of overall
#[derive(Debug, Clone)]
pub struct BulkBehavior {
    /// The number of bits that can enter this channel's sources
    pub channel_entry_widths: Vec<usize>,
    /// The number of bits that can exit this channel
    pub channel_exit_width: usize,
    /// For now, we just add up the number of LUT bits in the channel
    pub lut_bits: usize,
}

impl BulkBehavior {
    pub fn empty() -> Self {
        Self {
            channel_entry_widths: vec![],
            channel_exit_width: 0,
            lut_bits: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Programmability {
    TNode,

    StaticLut(Awi),

    // `DynamicLut`s can go in one of two ways: the table bits directly connect with configurable
    // bits and thus it can behave as an `ArbitraryLut`, or the inx bits directly connect with
    // configurable bits and thus can behave as `SelectorLut`s. Currently we will trigger
    // lowerings when a LUT doesn't fit into any category and lower down into just `StaticLut`s if
    // necessary.
    /// Can behave as an arbitrary lookup table outputting a bit and taking the
    /// input bits.
    ArbitraryLut(Vec<PBack>),
    /// Can behave as an arbitrary selector that multiplexes one of the input
    /// bits to the output
    SelectorLut(SelectorLut),

    /// Bulk behavior
    Bulk(BulkBehavior),
}

impl Programmability {
    pub fn debug_strings(&self) -> Vec<String> {
        let mut v = vec![];
        match self {
            Programmability::TNode => v.push("tnode".to_owned()),
            Programmability::StaticLut(lut) => v.push(format!("{}", lut)),
            Programmability::ArbitraryLut(lut) => v.push(format!("ArbLut {}", lut.len())),
            Programmability::SelectorLut(selector_lut) => {
                v.push(format!("SelLut {}", selector_lut.v.len()))
            }
            Programmability::Bulk(bulk) => {
                let mut s = String::new();
                for width in &bulk.channel_entry_widths {
                    write!(s, " {}", width).unwrap();
                }
                v.push(s);
                v.push(format!("lut_bits {}", bulk.lut_bits));
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
    // Ideally when `CNode`s are merged, they keep approximately the same weight distribution for
    // wide edges delay_weight: u64,
    //lagrangian_weight: u64,
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

    pub fn is_base(&self) -> bool {
        matches!(self.programmability(), Programmability::Bulk(_))
    }

    /*pub fn channel_entry_width(&self) -> usize {
        match self.programmability() {
            Programmability::StaticLut(awi) => awi.bw().trailing_zeros() as usize,
            Programmability::ArbitraryLut(table) => table.len().trailing_zeros() as usize,
            Programmability::SelectorLut(selector_lut) => selector_lut.v.len(),
            Programmability::Bulk(bulk) => bulk.channel_entry_widths.sum(),
        }
    }

    pub fn channel_exit_width(&self) -> usize {
        match self.programmability() {
            Programmability::StaticLut(awi) => 1,
            Programmability::ArbitraryLut(table) => 1,
            Programmability::SelectorLut(selector_lut) => 1,
            Programmability::Bulk(bulk) => bulk.channel_exit_width,
        }
    }

    /// Takes the minimum of the channel entry width and channel exit width
    pub fn channel_width(&self) -> usize {
        min(self.channel_entry_width(), self.channel_exit_width())
    }*/
}

impl<PCNode: Ptr, PCEdge: Ptr> Channeler<PCNode, PCEdge> {
    /// Given the source and sink incidences (which should point to unique
    /// `ThisCNode`s), this will manage the backrefs
    pub fn make_cedge(
        &mut self,
        sources: &[PCNode],
        sink: PCNode,
        programmability: Programmability,
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
            if configurator
                .configurations
                .find_key(&equiv.p_self_equiv)
                .is_none()
            {
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
                        v.push(channeler.translate(ensemble, *input).1.unwrap());
                    }
                    channeler.make_cedge(&v, p_self, Programmability::StaticLut(awi.clone()));
                }
                LNodeKind::DynamicLut(inp, lut) => {
                    let mut is_full_selector = true;
                    for input in inp {
                        let p_equiv = channeler.translate(ensemble, *input).0;
                        if configurator.find(p_equiv).is_none() {
                            is_full_selector = false;
                        }
                    }
                    let mut is_full_arbitrary = true;
                    for lut_bit in lut.iter() {
                        match lut_bit {
                            DynamicValue::ConstUnknown | DynamicValue::Const(_) => {
                                // TODO we should handle intermediates inbetween arbitrary and
                                // static
                                is_full_arbitrary = false;
                            }
                            DynamicValue::Dynam(p) => {
                                let p_equiv = channeler.translate(ensemble, *p).0;
                                if configurator.find(p_equiv).is_none() {
                                    is_full_arbitrary = false;
                                }
                            }
                        }
                    }
                    match (is_full_selector, is_full_arbitrary) {
                        (true, false) => {
                            let mut v = SmallVec::<[PCNode; 8]>::with_capacity(inp.len());
                            let mut config = vec![];
                            for input in inp.iter() {
                                config.push(channeler.translate(ensemble, *input).0);
                            }
                            let mut awi = Awi::zero(NonZeroUsize::new(2 << inp.len()).unwrap());
                            for (i, lut_bit) in lut.iter().enumerate() {
                                let i = i << 1;
                                match lut_bit {
                                    DynamicValue::ConstUnknown => {
                                        awi.set(i, true).unwrap();
                                    }
                                    DynamicValue::Const(b) => {
                                        awi.set(i.wrapping_add(1), true).unwrap();
                                        if *b {
                                            awi.set(i, true).unwrap();
                                        }
                                    }
                                    DynamicValue::Dynam(p) => {
                                        v.push(channeler.translate(ensemble, *p).1.unwrap());
                                    }
                                }
                            }
                            channeler.make_cedge(
                                &v,
                                p_self,
                                Programmability::SelectorLut(SelectorLut { awi, v: config }),
                            );
                        }
                        (false, true) => {
                            let mut v = SmallVec::<[PCNode; 8]>::with_capacity(inp.len());
                            for input in inp {
                                v.push(channeler.translate(ensemble, *input).1.unwrap());
                            }
                            let mut config = vec![];
                            for lut_bit in lut.iter() {
                                if let DynamicValue::Dynam(p) = lut_bit {
                                    let p_equiv = channeler.translate(ensemble, *p).0;
                                    config.push(p_equiv);
                                } else {
                                    unreachable!()
                                }
                            }
                            channeler.make_cedge(&v, p_self, Programmability::ArbitraryLut(config));
                        }
                        // we will need interaction with the `Ensemble` to do `LNode` side lowering
                        _ => todo!(),
                    }
                }
            }
        }

        // add `CEdge`s according to `TNode`s
        for tnode in ensemble.tnodes.vals() {
            let v = [channeler.translate(ensemble, tnode.p_driver).1.unwrap()];
            channeler.make_cedge(
                &v,
                channeler.translate(ensemble, tnode.p_self).1.unwrap(),
                Programmability::TNode,
            );
        }

        generate_hierarchy(&mut channeler)?;

        Ok(channeler)
    }

    /// Returns an `OrdArena` of `ThisCNode` `PCNode`s of `p` itself and all
    /// nodes directly incident to it through edges.
    pub fn related_nodes(&self, p: PCNode) -> OrdArena<PUniqueCNode, PCNode, ()> {
        let mut res = OrdArena::new();
        let _ = res.insert(p, ());
        let mut adv = self.cnodes.advancer_surject(p);
        while let Some(p_referent) = adv.advance(&self.cnodes) {
            if let Referent::CEdgeIncidence(p_cedge, _) = self.cnodes.get_key(p_referent).unwrap() {
                let cedge = self.cedges.get(*p_cedge).unwrap();
                cedge.incidents(|p_incident| {
                    let p_tmp = self.cnodes.get_val(p_incident).unwrap().p_this_cnode;
                    let _ = res.insert(p_tmp, ());
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

ptr_struct!(PUniqueCNode);

pub struct CNodeSubnodeAdvancer<PCNode: Ptr, PCEdge: Ptr> {
    adv: SurjectPtrAdvancer<PCNode, Referent<PCNode, PCEdge>, CNode<PCNode>>,
}

impl<PCNode: Ptr, PCEdge: Ptr> Advancer for CNodeSubnodeAdvancer<PCNode, PCEdge> {
    type Collection = Channeler<PCNode, PCEdge>;
    type Item = PCNode;

    fn advance(&mut self, collection: &Self::Collection) -> Option<Self::Item> {
        while let Some(p_referent) = self.adv.advance(&collection.cnodes) {
            if let Referent::SubNode(p_subnode_ref) = collection.cnodes.get_key(p_referent).unwrap()
            {
                let p_cnode = collection
                    .cnodes
                    .get_val(*p_subnode_ref)
                    .unwrap()
                    .p_this_cnode;
                return Some(p_cnode);
            }
        }
        None
    }
}
