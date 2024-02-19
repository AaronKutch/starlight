use std::{
    fmt,
    num::{NonZeroU128, NonZeroU64, NonZeroUsize},
};

use awint::awint_dag::{
    smallvec::{smallvec, SmallVec},
    triple_arena::{
        ptr_struct,
        utils::{PtrGen, PtrInx},
        Arena, OrdArena, Ptr, Recast, Recaster,
    },
    Location, PState,
};

use crate::{
    awi::*,
    ensemble::{CommonValue, Delay, Ensemble, PBack, Referent, Value},
    epoch::{get_current_epoch, EpochShared},
    utils::{DisplayStr, HexadecimalNonZeroU128},
    Error,
};

ptr_struct!(PRNode);

// substituted because we need a custom `Debug` impl
/*ptr_struct!(
    PExternal[NonZeroU128]()
    doc="A UUID `Ptr` for external use that maps to an internal `PRNode`"
);*/

/// A UUID `Ptr` for external use that maps to an internal `PRNode`
#[derive(
    core::hash::Hash,
    core::clone::Clone,
    core::marker::Copy,
    core::cmp::PartialEq,
    core::cmp::Eq,
    core::cmp::PartialOrd,
    core::cmp::Ord,
)]
pub struct PExternal {
    // note: in this order `PartialOrd` will order primarily off of `_internal_inx`
    #[doc(hidden)]
    _internal_inx: NonZeroU128,
    #[doc(hidden)]
    _internal_gen: (),
}

unsafe impl Ptr for PExternal {
    type Gen = ();
    type Inx = NonZeroU128;

    fn name() -> &'static str {
        "PExternal"
    }

    #[inline]
    fn invalid() -> Self {
        Self {
            _internal_inx: PtrInx::new(<Self::Inx as PtrInx>::max()),
            _internal_gen: PtrGen::one(),
        }
    }

    #[inline]
    fn inx(self) -> Self::Inx {
        self._internal_inx
    }

    #[inline]
    fn gen(self) -> Self::Gen {
        self._internal_gen
    }

    #[inline]
    #[doc(hidden)]
    fn _from_raw(_internal_inx: Self::Inx, _internal_gen: Self::Gen) -> Self {
        Self {
            _internal_inx,
            _internal_gen,
        }
    }
}

impl core::default::Default for PExternal {
    #[inline]
    fn default() -> Self {
        Ptr::invalid()
    }
}

impl core::fmt::Display for PExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

impl Recast<Self> for PExternal {
    fn recast<R: Recaster<Item = Self>>(
        &mut self,
        recaster: &R,
    ) -> core::result::Result<(), <R as Recaster>::Item> {
        recaster.recast_item(self)
    }
}

impl fmt::Debug for PExternal {
    /// Can only display some fields if the `Epoch` `self` was created in is
    /// active
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            let mut tmp = f.debug_struct("PExternal");
            tmp.field("p_external", &HexadecimalNonZeroU128(self.inx()));
            if let Ok(epoch) = get_current_epoch() {
                if let Ok(lock) = epoch.epoch_data.try_borrow() {
                    if let Ok((_, rnode)) = lock.ensemble.notary.get_rnode(*self) {
                        if let Some(ref name) = rnode.debug_name {
                            tmp.field("debug_name", &DisplayStr(name));
                        }
                        /*if let Some(s) = lock.ensemble.get_state_debug(self.state()) {
                            tmp.field("state", &DisplayStr(&s));
                        }
                        tmp.field("bits", &rnode.bits());*/
                    }
                }
            }
            tmp.finish()
        } else {
            f.write_fmt(format_args!(
                "{}[{:x?}]",
                <Self as Ptr>::name(),
                Ptr::inx(*self),
            ))
        }
    }
}

/// Reference/Register/Report node, used for external references kept alive
/// after `State` pruning
#[derive(Debug, Clone)]
pub struct RNode {
    nzbw: NonZeroUsize,
    bits: SmallVec<[Option<PBack>; 1]>,
    read_only: bool,
    /// Number of references like `LazyAwi`s and `ExtAwi`s
    pub extern_rc: u64,
    /// Associated state that this `RNode` was initialized from
    pub associated_state: Option<PState>,
    /// If the associated state needs to be lowered before states are pruned
    pub lower_before_pruning: bool,
    /// Location where this `RNode` was created
    pub location: Option<Location>,
    /// Name used for debug renders and more
    pub debug_name: Option<String>,
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
        extern_rc: u64,
        location: Option<Location>,
        associated_state: Option<PState>,
        lower_before_pruning: bool,
    ) -> Self {
        Self {
            nzbw,
            read_only,
            bits: smallvec![],
            extern_rc,
            associated_state,
            lower_before_pruning,
            location,
            debug_name: None,
        }
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.nzbw
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    /// Returns `None` if the `RNode` has not been initialized yet
    #[must_use]
    pub fn bits(&self) -> Option<&[Option<PBack>]> {
        if self.bits.is_empty() {
            None
        } else {
            Some(&self.bits)
        }
    }

    #[must_use]
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
    pub(crate) rnodes: OrdArena<PRNode, PExternal, RNode>,
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

    /// Finds the `(PRNode, &RNode)` pair corresponding to `p_external`
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidPExternal(p_external)` if `p_external` could not
    /// be found
    pub fn get_rnode(&self, p_external: PExternal) -> Result<(PRNode, &RNode), Error> {
        if let Some(p_rnode) = self.rnodes.find_key(&p_external) {
            Ok((p_rnode, self.rnodes.get_val(p_rnode).unwrap()))
        } else {
            Err(Error::InvalidPExternal(p_external))
        }
    }

    /// Finds the `(PRNode, &mut RNode)` pair corresponding to `p_external`
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidPExternal(p_external)` if `p_external` could not
    /// be found
    pub fn get_rnode_mut(&mut self, p_external: PExternal) -> Result<(PRNode, &mut RNode), Error> {
        if let Some(p_rnode) = self.rnodes.find_key(&p_external) {
            Ok((p_rnode, self.rnodes.get_val_mut(p_rnode).unwrap()))
        } else {
            Err(Error::InvalidPExternal(p_external))
        }
    }

    #[must_use]
    pub fn get_rnode_by_p_rnode_mut(&mut self, p_rnode: PRNode) -> Option<&mut RNode> {
        self.rnodes.get_val_mut(p_rnode)
    }
}

impl Ensemble {
    /// Makes a new `RNode` with `extern_rc = 1`
    pub fn make_rnode_for_pstate(
        &mut self,
        p_state: PState,
        location: Option<Location>,
        read_only: bool,
        lower_before_pruning: bool,
    ) -> Result<PExternal, Error> {
        if let Some(state) = self.stator.states.get_mut(p_state) {
            state.inc_extern_rc();
            let nzbw = state.nzbw;
            let (_, p_external) = self.notary.insert_rnode(RNode::new(
                nzbw,
                read_only,
                1,
                location,
                Some(p_state),
                lower_before_pruning,
            ));
            Ok(p_external)
        } else {
            Err(Error::OtherString(format!(
                "state {p_state} has been pruned or is from a different epoch"
            )))
        }
    }

    /// Returns if anything was actually initialized
    pub fn initialize_rnode_if_needed_no_lowering(
        &mut self,
        p_rnode: PRNode,
        allow_pruned: bool,
    ) -> Result<bool, Error> {
        let rnode = &self.notary.rnodes()[p_rnode];
        if rnode.bits.is_empty() {
            if let Some(p_state) = rnode.associated_state {
                self.initialize_state_bits_if_needed(p_state)?;
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
                Ok(true)
            } else if !allow_pruned {
                Err(Error::OtherStr("failed to initialize `RNode`"))
            } else {
                Ok(false)
            }
        } else {
            Ok(false)
        }
    }

    /// If the state is allowed to be pruned, `allow_pruned` can be set. This
    /// also runs DFS state lowering. Returns if anything was actually
    /// initialized
    pub fn initialize_rnode_if_needed(
        epoch_shared: &EpochShared,
        p_rnode: PRNode,
        allow_pruned: bool,
    ) -> Result<bool, Error> {
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let rnode = lock.ensemble.notary.rnodes.get_val_mut(p_rnode).unwrap();
        if rnode.lower_before_pruning {
            rnode.lower_before_pruning = false;
            let p_state = rnode.associated_state.unwrap();
            if lock.ensemble.stator.states.contains(p_state) {
                drop(lock);
                Ensemble::dfs_lower(epoch_shared, p_state)?;
            } else {
                drop(lock);
            }
        } else {
            drop(lock);
        }
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        lock.ensemble
            .initialize_rnode_if_needed_no_lowering(p_rnode, allow_pruned)
    }

    /// This unconditionally removes the `RNode`, you may want `rnode_dec_rc`
    /// instead
    pub fn remove_rnode(&mut self, p_rnode: PRNode) {
        let rnode = self.notary.rnodes.remove(p_rnode).unwrap().1;
        if let Some(p_state) = rnode.associated_state {
            self.state_dec_extern_rc(p_state).unwrap();
        }
        for p_back in rnode.bits {
            if let Some(p_back) = p_back {
                let referent = self.backrefs.remove_key(p_back).unwrap().0;
                debug_assert!(matches!(referent, Referent::ThisRNode(_)));
            }
        }
    }

    /// Increments the `extern_rc` of the `RNode` pointed to by `p_external`
    pub fn rnode_inc_rc(&mut self, p_external: PExternal) -> Result<PRNode, Error> {
        let (p_rnode, rnode) = self.notary.get_rnode_mut(p_external)?;
        rnode.extern_rc = rnode.extern_rc.checked_add(1).unwrap();
        Ok(p_rnode)
    }

    /// Decrements the `extern_rc` of the `RNode` pointed to by `p_external`,
    /// removing it if the count drops to zero
    pub fn rnode_dec_rc(&mut self, p_external: PExternal) -> Result<(), Error> {
        let (p_rnode, rnode) = self.notary.get_rnode_mut(p_external)?;
        rnode.extern_rc = rnode.extern_rc.checked_sub(1).unwrap();
        if rnode.extern_rc == 0 {
            self.remove_rnode(p_rnode);
        }
        Ok(())
    }

    /// Sets all `associated_state`s to `None`
    pub fn remove_all_rnode_associated_states(&mut self) {
        let mut states_to_dec_rc = vec![];
        for rnode in self.notary.rnodes.vals_mut() {
            if let Some(p_state) = rnode.associated_state {
                states_to_dec_rc.push(p_state);
                rnode.associated_state = None;
            }
        }
        for p_state in states_to_dec_rc {
            self.state_dec_extern_rc(p_state).unwrap();
        }
    }

    pub fn get_thread_local_rnode_nzbw(p_external: PExternal) -> Result<NonZeroUsize, Error> {
        let epoch_shared = get_current_epoch()?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        let (_, rnode) = ensemble.notary.get_rnode(p_external)?;
        Ok(rnode.nzbw)
    }

    /// Note: `make_const` cannot be true at the same time as the basic type is
    /// opaque
    pub fn change_thread_local_rnode_value(
        p_external: PExternal,
        common_value: CommonValue<'_>,
        make_const: bool,
    ) -> Result<(), Error> {
        let epoch_shared = get_current_epoch()?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        let (p_rnode, _) = ensemble.notary.get_rnode(p_external)?;
        drop(lock);
        // `restart_request` not needed if an initialization happens here, because we
        // are in change phase and any change later will fix the process
        Ensemble::initialize_rnode_if_needed(&epoch_shared, p_rnode, true)?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if !ensemble.notary.rnodes[p_rnode].bits.is_empty() {
            let lhs_w = ensemble.notary.rnodes[p_rnode].bits.len();
            let rhs_w = common_value.bw();
            if lhs_w != rhs_w {
                return Err(Error::BitwidthMismatch(lhs_w, rhs_w));
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
                    // if an error occurs, no event is inserted and we do not insert anything
                    // here, the change is treated as having never occured
                    ensemble.change_value(p_back, bit, NonZeroU64::new(1).unwrap())?;
                }
            }
        }
        // else the state was pruned
        Ok(())
    }

    pub fn request_thread_local_rnode_value(
        p_external: PExternal,
        bit_i: usize,
    ) -> Result<Value, Error> {
        let epoch_shared = get_current_epoch()?;
        let lock = epoch_shared.epoch_data.borrow();
        // first check if it already exists in current epoch
        let init = if let Ok((p_rnode, _)) = lock.ensemble.notary.get_rnode(p_external) {
            drop(lock);
            Ensemble::initialize_rnode_if_needed(&epoch_shared, p_rnode, false)?
        } else {
            drop(lock);
            false
        };
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        if init {
            // if anything was initialized and we are already in request phase, there are
            // cases where we need to do this to clear events before the value is requested
            lock.ensemble.restart_request_phase()?;
        }
        // then start returning errors about not being the right epoch
        let (_, rnode) = lock.ensemble.notary.get_rnode(p_external)?;
        if bit_i >= rnode.bits.len() {
            return Err(Error::OtherStr(
                "something went wrong with an rnode bitwidth",
            ));
        }
        if let Some(p_back) = rnode.bits[bit_i] {
            lock.ensemble.request_value(p_back)
        } else {
            Err(Error::OtherStr(
                "something went wrong, found `RNode` for evaluator but a bit was pruned",
            ))
        }
    }

    pub fn tnode_drive_thread_local_rnode(
        p_source: PExternal,
        source_bit_i: usize,
        p_driver: PExternal,
        driver_bit_i: usize,
        delay: Delay,
    ) -> Result<(), Error> {
        let epoch_shared = get_current_epoch()?;
        // first check if it already exists in current epoch
        let lock = epoch_shared.epoch_data.borrow_mut();
        let mut init = if let Ok((p_rnode, _)) = lock.ensemble.notary.get_rnode(p_source) {
            drop(lock);
            Ensemble::initialize_rnode_if_needed(&epoch_shared, p_rnode, false)?
        } else {
            drop(lock);
            false
        };
        let lock = epoch_shared.epoch_data.borrow_mut();
        init |= if let Ok((p_rnode, _)) = lock.ensemble.notary.get_rnode(p_driver) {
            drop(lock);
            Ensemble::initialize_rnode_if_needed(&epoch_shared, p_rnode, false)?
        } else {
            drop(lock);
            false
        };
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        if init {
            lock.ensemble.restart_request_phase()?;
        }
        // then start returning errors about not being the right epoch
        let (_, source_rnode) = lock.ensemble.notary.get_rnode(p_source)?;
        if source_bit_i >= source_rnode.bits.len() {
            return Err(Error::OtherStr(
                "something went wrong with an rnode bitwidth",
            ));
        }
        let source_p_back = if let Some(p_back) = source_rnode.bits[source_bit_i] {
            p_back
        } else {
            return Err(Error::OtherStr(
                "something went wrong, found `RNode` for `TNode` driving but a bit was pruned",
            ))
        };
        let (_, driver_rnode) = lock.ensemble.notary.get_rnode(p_driver)?;
        if driver_bit_i >= driver_rnode.bits.len() {
            return Err(Error::OtherStr(
                "something went wrong with an rnode bitwidth",
            ));
        }
        let driver_p_back = if let Some(p_back) = driver_rnode.bits[driver_bit_i] {
            p_back
        } else {
            return Err(Error::OtherStr(
                "something went wrong, found `RNode` for `TNode` driving but a bit was pruned",
            ))
        };

        // now connect with `TNode`
        let p_tnode = lock
            .ensemble
            .make_tnode(source_p_back, driver_p_back, delay)
            .unwrap();
        // initial drive
        lock.ensemble.eval_tnode(p_tnode).unwrap();
        Ok(())
    }

    pub fn thread_local_rnode_set_debug_name(
        p_external: PExternal,
        debug_name: Option<&str>,
    ) -> Result<(), Error> {
        let epoch_shared = get_current_epoch()?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        let (p_rnode, _) = ensemble.notary.get_rnode(p_external)?;
        ensemble
            .notary
            .rnodes
            .get_val_mut(p_rnode)
            .unwrap()
            .debug_name = debug_name.map(|s| s.to_owned());
        Ok(())
    }
}

impl Default for Notary {
    fn default() -> Self {
        Self::new()
    }
}
