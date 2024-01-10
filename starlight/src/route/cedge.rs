use std::num::NonZeroUsize;

use awint::{awint_dag::EvalError, Awi};

use crate::{
    awint_dag::smallvec::SmallVec,
    ensemble,
    ensemble::{DynamicValue, Ensemble, LNodeKind},
    route::{channel::Referent, Channeler, Configurator, PBack},
    triple_arena::ptr_struct,
    SuspendedEpoch,
};

ptr_struct!(PCEdge);

#[derive(Debug, Clone, Copy)]
pub enum SelectorValue {
    Dynam,
    ConstUnknown,
    Const(bool),
}

/// The `Vec<ensemble::PBack>` has the configuration indexes, the two `Awi`s
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
    v: Vec<ensemble::PBack>,
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

    pub fn verify_integrity(&self, sources_len: usize, sinks_len: usize) -> Result<(), EvalError> {
        // TODO
        let pow_len = 1usize << self.v.len();
        if (pow_len.checked_mul(2).unwrap() != self.awi.bw()) || (sinks_len != 1) {
            return Err(EvalError::OtherStr("problem with `SelectorLut` validation"));
        }
        let mut dynam_len = 0;
        for i in 0..pow_len {
            if let SelectorValue::Dynam = self.get_selector_value(i) {
                dynam_len += 1;
            }
        }
        if dynam_len != sources_len {
            return Err(EvalError::OtherStr("problem with `SelectorLut` validation"));
        }
        Ok(())
    }
}

/// Used by higher order edges to tell what it is capable of overall
#[derive(Debug, Clone)]
pub struct BulkBehavior {
    /// The number of bits that can enter this channel
    pub channel_entry_width: usize,
    /// The number of bits that can exit this channel
    pub channel_exit_width: usize,
    /// For now, we just add up the number of LUT bits in the channel
    pub lut_bits: usize,
}

#[derive(Debug, Clone)]
pub enum Programmability {
    /// Nothing can happen between nodes, used for connecting top level nodes
    /// that have no connection to each other
    Noop,

    StaticLut(Awi),

    // `DynamicLut`s can go in one of two ways: the table bits directly connect with configurable
    // bits and thus it can behave as an `ArbitraryLut`, or the inx bits directly connect with
    // configurable bits and thus can behave as `SelectorLut`s. Currently we will trigger
    // lowerings when a LUT doesn't fit into any category and lower down into just `StaticLut`s if
    // necessary.
    /// Can behave as an arbitrary lookup table outputting a bit and taking the
    /// input bits.
    ArbitraryLut(Vec<ensemble::PBack>),
    /// Can behave as an arbitrary selector that multiplexes one of the input
    /// bits to the output
    SelectorLut(SelectorLut),

    /// Bulk behavior
    Bulk(BulkBehavior),
}

/// An edge between channels
#[derive(Debug, Clone)]
pub struct CEdge {
    // sources and sinks incident to nodes
    sources: Vec<PBack>,
    sinks: Vec<PBack>,

    programmability: Programmability,
    // Ideally when `CNode`s are merged, they keep approximately the same weight distribution for
    // wide edges delay_weight: u64,
    //lagrangian_weight: u64,
}

impl CEdge {
    pub fn programmability(&self) -> &Programmability {
        &self.programmability
    }

    pub fn sources(&self) -> &[PBack] {
        &self.sources
    }

    pub fn sinks(&self) -> &[PBack] {
        &self.sinks
    }

    pub fn incidents<F: FnMut(PBack)>(&self, mut f: F) {
        for source in self.sources() {
            f(*source)
        }
        for sink in self.sinks() {
            f(*sink)
        }
    }

    pub fn incidents_len(&self) -> usize {
        self.sources()
            .len()
            .checked_add(self.sinks().len())
            .unwrap()
    }
}

impl Channeler {
    /// Given the source and sink incidences (which should point to unique
    /// `ThisCNode`s), this will manage the backrefs
    fn make_cedge(
        &mut self,
        sources: &[PBack],
        sink: &[PBack],
        programmability: Programmability,
    ) -> PCEdge {
        self.cedges.insert_with(|p_self| {
            let mut fixed_sources = vec![];
            let mut fixed_sinks = vec![];
            for (i, source) in sources.iter().enumerate() {
                fixed_sources.push(
                    self.cnodes
                        .insert_key(*source, Referent::CEdgeIncidence(p_self, i, false))
                        .unwrap(),
                );
            }
            for (i, sink) in sink.iter().enumerate() {
                fixed_sinks.push(
                    self.cnodes
                        .insert_key(*sink, Referent::CEdgeIncidence(p_self, i, true))
                        .unwrap(),
                );
            }
            CEdge {
                sources: fixed_sources,
                sinks: fixed_sinks,
                programmability,
            }
        })
    }

    pub fn from_target(
        target_epoch: &SuspendedEpoch,
        configurator: &Configurator,
    ) -> Result<Self, EvalError> {
        target_epoch.ensemble(|ensemble| Self::new(ensemble, configurator))
    }

    pub fn from_program(target_epoch: &SuspendedEpoch) -> Result<Self, EvalError> {
        target_epoch.ensemble(|ensemble| Self::new(ensemble, &Configurator::new()))
    }

    /// Assumes that the ensemble has been optimized
    pub fn new(ensemble: &Ensemble, configurator: &Configurator) -> Result<Self, EvalError> {
        let mut channeler = Self::empty();

        // for each equivalence make a `CNode` with associated `EnsembleBackref`
        for equiv in ensemble.backrefs.vals() {
            let p_cnode = channeler.make_top_level_cnode(vec![]);
            let channeler_backref = channeler
                .cnodes
                .insert_key(p_cnode, Referent::EnsembleBackRef(equiv.p_self_equiv))
                .unwrap();
            let replaced = channeler
                .ensemble_backref_to_channeler_backref
                .insert(equiv.p_self_equiv, channeler_backref)
                .1;
            assert!(replaced.is_none());
        }

        // translate from any ensemble backref to the equivalence backref to the
        // channeler backref
        fn translate(
            ensemble: &Ensemble,
            channeler: &Channeler,
            ensemble_backref: ensemble::PBack,
        ) -> (ensemble::PBack, PBack) {
            let p_equiv = ensemble
                .backrefs
                .get_val(ensemble_backref)
                .unwrap()
                .p_self_equiv;
            let p0 = channeler
                .ensemble_backref_to_channeler_backref
                .find_key(&p_equiv)
                .unwrap();
            let channeler_p_back = *channeler
                .ensemble_backref_to_channeler_backref
                .get_val(p0)
                .unwrap();
            (p_equiv, channeler_p_back)
        }

        // add `CEdge`s according to `LNode`s
        for lnode in ensemble.lnodes.vals() {
            let p_self = translate(ensemble, &channeler, lnode.p_self).1;
            match &lnode.kind {
                LNodeKind::Copy(_) => {
                    return Err(EvalError::OtherStr("the epoch was not optimized"))
                }
                LNodeKind::Lut(inp, awi) => {
                    let mut v = SmallVec::<[PBack; 8]>::with_capacity(inp.len());
                    for input in inp {
                        v.push(translate(ensemble, &channeler, *input).1);
                    }
                    channeler.make_cedge(&v, &[p_self], Programmability::StaticLut(awi.clone()));
                }
                LNodeKind::DynamicLut(inp, lut) => {
                    //let p_self = translate(ensemble, &channeler, lnode.p_self).1;
                    let mut is_full_selector = true;
                    for input in inp {
                        let p_equiv = translate(ensemble, &channeler, *input).0;
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
                                let p_equiv = translate(ensemble, &channeler, *p).0;
                                if configurator.find(p_equiv).is_none() {
                                    is_full_arbitrary = false;
                                }
                            }
                        }
                    }
                    match (is_full_selector, is_full_arbitrary) {
                        (true, false) => {
                            let mut v = SmallVec::<[PBack; 8]>::with_capacity(inp.len());
                            let mut config = vec![];
                            for input in inp.iter() {
                                config.push(translate(ensemble, &channeler, *input).0);
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
                                        v.push(translate(ensemble, &channeler, *p).1);
                                    }
                                }
                            }
                            channeler.make_cedge(
                                &v,
                                &[p_self],
                                Programmability::SelectorLut(SelectorLut { awi, v: config }),
                            );
                        }
                        (false, true) => {
                            let mut v = SmallVec::<[PBack; 8]>::with_capacity(inp.len());
                            for input in inp {
                                v.push(translate(ensemble, &channeler, *input).1);
                            }
                            let mut config = vec![];
                            for lut_bit in lut.iter() {
                                if let DynamicValue::Dynam(p) = lut_bit {
                                    let p_equiv = translate(ensemble, &channeler, *p).0;
                                    config.push(p_equiv);
                                } else {
                                    unreachable!()
                                }
                            }
                            channeler.make_cedge(
                                &v,
                                &[p_self],
                                Programmability::ArbitraryLut(config),
                            );
                        }
                        // we will need interaction with the `Ensemble` to do `LNode` side lowering
                        _ => todo!(),
                    }
                }
            }
        }

        Ok(channeler)
    }
}
