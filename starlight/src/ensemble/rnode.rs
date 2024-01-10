use std::num::{NonZeroU128, NonZeroUsize};

use awint::awint_dag::{
    smallvec::{smallvec, SmallVec},
    triple_arena::{ptr_struct, Arena, OrdArena, Ptr, Recast, Recaster},
    PState,
};

use crate::{
    awi::*,
    ensemble::{CommonValue, Ensemble, PBack, Referent, Value},
    epoch::get_current_epoch,
    Error,
};

ptr_struct!(PRNode);

ptr_struct!(
    PExternal[NonZeroU128]()
    doc="A UUID `Ptr` for external use that maps to an internal `PRNode`"
);

/// Reference/Register/Report node, used for external references kept alive
/// after `State` pruning
#[derive(Debug, Clone)]
pub struct RNode {
    nzbw: NonZeroUsize,
    bits: SmallVec<[Option<PBack>; 1]>,
    read_only: bool,
    pub associated_state: Option<PState>,
    pub lower_before_pruning: bool,
}

impl Recast<PBack> for RNode {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.bits.as_mut_slice().recast(recaster)
    }
}

impl RNode {
    pub fn new(
        nzbw: NonZeroUsize,
        read_only: bool,
        associated_state: Option<PState>,
        lower_before_pruning: bool,
    ) -> Self {
        Self {
            nzbw,
            read_only,
            bits: smallvec![],
            associated_state,
            lower_before_pruning,
        }
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.nzbw
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Returns `None` if the `RNode` has not been initialized yet
    pub fn bits(&self) -> Option<&[Option<PBack>]> {
        if self.bits.is_empty() {
            None
        } else {
            Some(&self.bits)
        }
    }

    pub fn bits_mut(&mut self) -> Option<&mut [Option<PBack>]> {
        if self.bits.is_empty() {
            None
        } else {
            Some(&mut self.bits)
        }
    }
}

/// Used for managing external references
#[derive(Debug, Clone)]
pub struct Notary {
    rnodes: OrdArena<PRNode, PExternal, RNode>,
    next_external: NonZeroU128,
}

impl Recast<PBack> for Notary {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.rnodes.recast(recaster)
    }
}

impl Notary {
    pub fn new() -> Self {
        Self {
            rnodes: OrdArena::new(),
            next_external: rand::random(),
        }
    }

    pub fn recast_p_rnode(&mut self) -> Arena<PRNode, PRNode> {
        self.rnodes.compress_and_shrink_recaster()
    }

    pub fn rnodes(&self) -> &OrdArena<PRNode, PExternal, RNode> {
        &self.rnodes
    }

    pub fn insert_rnode(&mut self, rnode: RNode) -> (PRNode, PExternal) {
        let p_external = PExternal::_from_raw(self.next_external, ());
        let (res, replaced) = self.rnodes.insert(p_external, rnode);
        // there is an astronomically small chance this fails naturally when
        // `PExternal`s from other `Notary`s are involved
        assert!(replaced.is_none());
        // wrapping increment except that zero is skipped
        self.next_external = NonZeroU128::new(self.next_external.get().wrapping_add(1))
            .unwrap_or(NonZeroU128::new(1).unwrap());
        (res, p_external)
    }

    pub fn get_rnode(&self, p_external: PExternal) -> Option<(PRNode, &RNode)> {
        let p_rnode = self.rnodes.find_key(&p_external)?;
        Some((p_rnode, self.rnodes.get_val(p_rnode).unwrap()))
    }

    pub fn get_rnode_mut(&mut self, p_external: PExternal) -> Option<(PRNode, &mut RNode)> {
        let p_rnode = self.rnodes.find_key(&p_external)?;
        Some((p_rnode, self.rnodes.get_val_mut(p_rnode).unwrap()))
    }

    pub fn get_rnode_by_p_rnode_mut(&mut self, p_rnode: PRNode) -> Option<&mut RNode> {
        self.rnodes.get_val_mut(p_rnode)
    }
}

impl Ensemble {
    #[must_use]
    pub fn make_rnode_for_pstate(
        &mut self,
        p_state: PState,
        read_only: bool,
        lower_before_pruning: bool,
    ) -> Option<PExternal> {
        let nzbw = self.stator.states[p_state].nzbw;
        let (_, p_external) = self.notary.insert_rnode(RNode::new(
            nzbw,
            read_only,
            Some(p_state),
            lower_before_pruning,
        ));
        Some(p_external)
    }

    /// If the state is allowed to be pruned, `allow_pruned` can be set.
    pub fn initialize_rnode_if_needed(
        &mut self,
        p_rnode: PRNode,
        allow_pruned: bool,
    ) -> Result<(), Error> {
        let rnode = &self.notary.rnodes()[p_rnode];
        if rnode.bits.is_empty() {
            if let Some(p_state) = rnode.associated_state {
                if self.initialize_state_bits_if_needed(p_state).is_some() {
                    let len = self.stator.states[p_state].p_self_bits.len();
                    for i in 0..len {
                        let p_bit = self.stator.states[p_state].p_self_bits[i];
                        if let Some(p_bit) = p_bit {
                            let p_equiv = self.backrefs.get_val(p_bit).unwrap().p_self_equiv;
                            let p_back_new = self
                                .backrefs
                                .insert_key(p_equiv, Referent::ThisRNode(p_rnode))
                                .unwrap();
                            self.notary.rnodes[p_rnode].bits.push(Some(p_back_new));
                        } else {
                            self.notary.rnodes[p_rnode].bits.push(None);
                        }
                    }
                    return Ok(())
                }
            }
            if !allow_pruned {
                return Err(Error::OtherStr("failed to initialize `RNode`"))
            }
        }
        Ok(())
    }

    pub fn remove_rnode(&mut self, p_external: PExternal) -> Result<(), Error> {
        if let Some(p_rnode) = self.notary.rnodes.find_key(&p_external) {
            let rnode = self.notary.rnodes.remove(p_rnode).unwrap().1;
            for p_back in rnode.bits {
                if let Some(p_back) = p_back {
                    let referent = self.backrefs.remove_key(p_back).unwrap().0;
                    debug_assert!(matches!(referent, Referent::ThisRNode(_)));
                }
            }
            Ok(())
        } else {
            Err(Error::InvalidPtr)
        }
    }

    pub fn get_thread_local_rnode_nzbw(p_external: PExternal) -> Result<NonZeroUsize, Error> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some((_, rnode)) = ensemble.notary.get_rnode(p_external) {
            Ok(rnode.nzbw)
        } else {
            Err(Error::OtherStr(
                "could not find thread local `RNode`, probably an `EvalAwi` or `LazyAwi` was used \
                 outside of the `Epoch` it was created in",
            ))
        }
    }

    /// Note: `make_const` cannot be true at the same time as the basic type is
    /// opaque
    pub fn change_thread_local_rnode_value(
        p_external: PExternal,
        common_value: CommonValue<'_>,
        make_const: bool,
    ) -> Result<(), Error> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some(p_rnode) = ensemble.notary.rnodes.find_key(&p_external) {
            ensemble.initialize_rnode_if_needed(p_rnode, true)?;
            if !ensemble.notary.rnodes[p_rnode].bits.is_empty() {
                if ensemble.notary.rnodes[p_rnode].bits.len() != common_value.bw() {
                    return Err(Error::WrongBitwidth);
                }
                for bit_i in 0..common_value.bw() {
                    let p_back = ensemble.notary.rnodes[p_rnode].bits[bit_i];
                    if let Some(p_back) = p_back {
                        let bit = common_value.get(bit_i).unwrap();
                        let bit = if make_const {
                            if let Some(bit) = bit {
                                Value::Const(bit)
                            } else {
                                Value::ConstUnknown
                            }
                        } else if let Some(bit) = bit {
                            Value::Dynam(bit)
                        } else {
                            Value::Unknown
                        };
                        ensemble.change_value(p_back, bit)?;
                    }
                }
            }
            // else the state was pruned
        } else {
            return Err(Error::OtherStr(
                "could not find thread local `RNode`, probably a `LazyAwi` was used outside of \
                 the `Epoch` it was created in",
            ))
        }
        Ok(())
    }

    pub fn calculate_thread_local_rnode_value(
        p_external: PExternal,
        bit_i: usize,
    ) -> Result<Value, Error> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some((p_rnode, _)) = ensemble.notary.get_rnode(p_external) {
            ensemble.initialize_rnode_if_needed(p_rnode, false)?;
        }
        let p_back = if let Some((_, rnode)) = ensemble.notary.get_rnode(p_external) {
            if bit_i >= rnode.bits.len() {
                return Err(Error::OtherStr("something went wrong with rnode bitwidth"));
            }
            if let Some(p_back) = rnode.bits[bit_i] {
                p_back
            } else {
                return Err(Error::OtherStr(
                    "something went wrong, found `RNode` for evaluator but a bit was pruned",
                ))
            }
        } else {
            return Err(Error::OtherStr(
                "could not find thread local `RNode`, probably an `EvalAwi` was used outside of \
                 the `Epoch` it was created in",
            ))
        };
        if ensemble.stator.states.is_empty() {
            // optimization after total pruning from `optimization`
            ensemble.calculate_value(p_back)
        } else {
            drop(lock);
            Ensemble::calculate_value_with_lower_capability(&epoch_shared, p_back)
        }
    }
}

impl Default for Notary {
    fn default() -> Self {
        Self::new()
    }
}
