use std::{mem, num::NonZeroUsize};

use awint::{
    awint_dag::{
        smallvec::SmallVec,
        triple_arena::{Advancer, Ptr},
        PState,
    },
    Awi, InlAwi,
};

use crate::{
    ensemble::{DynamicValue, Ensemble, LNode, LNodeKind, PBack, PLNode, PTNode, Referent, Value},
    triple_arena::{ptr_struct, OrdArena},
    utils::SmallMap,
    Error,
};

ptr_struct!(POpt);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CostU8(pub u8);

/// These variants must occur generally in order of easiest and most affecting
/// to hardest and computationally expensive, so  that things like removing
/// unused nodes happens before wasting time on the harder optimizations.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Optimization {
    Preinvestigate(PBack),
    /// Removes an entire equivalence class because it is unused
    RemoveEquiv(PBack),
    /// This needs to point to the `Referent::ThisLNode` of the identity
    /// `LNode`. If an equivalence is an identity function, any referents should
    /// use its inputs instead. This is high priority because the principle
    /// source of a value needs to be known for various optimizations
    /// involving deduplication to work (such as early LUT simplification), and
    /// also because it eliminates useless identities early.
    ForwardEquiv(PBack),
    /// Removes all `LNode`s from an equivalence that has had a constant
    /// assigned to it, and notifies all referents.
    ConstifyEquiv(PBack),
    /// Removes a `LNode` because there is at least one other `LNode` in the
    /// equivalence that is stricly better
    RemoveLNode(PBack),
    /// If a backref is removed, investigate this equivalence. Note that
    /// `InvestigateUsed`s overwrite each other when multiple ones are fired on
    /// the same equivalence.
    // TODO should this one be moved up? Needs to be benchmarked.
    InvestigateUsed(PBack),
    /// If an input was constified
    InvestigateConst(PLNode),
    /// If a driver was constified
    InvestigateDriverConst(PTNode),
    /// The optimization state that equivalences are set to after the
    /// preinvestigation finds nothing
    InvestigateEquiv0(PBack),
    //InvertInput
    // (?) not sure if fusion + ordinary `const_eval_lnode` handles all cases cleanly,
    // might only do fission for routing
    //Fission
    // A fusion involving the number of inputs that will result
    //Fusion(u8, PBack)
}

#[derive(Debug, Clone)]
pub struct Optimizer {
    optimizations: OrdArena<POpt, Optimization, ()>,
}

impl Optimizer {
    pub fn new() -> Self {
        Self {
            optimizations: OrdArena::new(),
        }
    }

    /// Checks that there are no remaining optimizations, then shrinks
    /// allocations
    pub fn check_clear(&mut self) -> Result<(), Error> {
        if !self.optimizations.is_empty() {
            return Err(Error::OtherStr("optimizations need to be empty"));
        }
        self.optimizations.clear_and_shrink();
        Ok(())
    }

    pub fn insert(&mut self, optimization: Optimization) {
        let _ = self.optimizations.insert(optimization, ());
    }
}

impl Ensemble {
    /// Removes all `Const` inputs and assigns `Const` result if possible.
    /// Returns if a `Const` result was assigned (`Optimization::ConstifyEquiv`
    /// needs to be run by the caller).
    pub fn const_eval_lnode(&mut self, p_lnode: PLNode) -> Result<bool, Error> {
        let lnode = self.lnodes.get_mut(p_lnode).unwrap();
        Ok(match &mut lnode.kind {
            LNodeKind::Copy(inp) => {
                // wire propogation
                let input_equiv = self.backrefs.get_val_mut(*inp).unwrap();
                let val = input_equiv.val;
                if val.is_const() {
                    let equiv = self.backrefs.get_val_mut(lnode.p_self).unwrap();
                    equiv.val = val;
                    self.optimizer
                        .insert(Optimization::ConstifyEquiv(equiv.p_self_equiv));
                    true
                } else {
                    self.optimizer
                        .insert(Optimization::ForwardEquiv(lnode.p_self));
                    false
                }
            }
            LNodeKind::Lut(inp, original_lut) => {
                let mut lut = original_lut.clone();
                // acquire LUT inputs, for every constant input reduce the LUT
                let len = usize::from(u8::try_from(inp.len()).unwrap());
                let mut encountered_const_unknown = false;
                for i in (0..len).rev() {
                    let p_inp = inp[i];
                    let equiv = self.backrefs.get_val(p_inp).unwrap();
                    match equiv.val {
                        Value::ConstUnknown => encountered_const_unknown = true,
                        Value::Const(val) => {
                            // we will reducing the LUT and removing this input, mark it to be
                            // investigated
                            self.optimizer
                                .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                            self.backrefs.remove_key(p_inp).unwrap();
                            inp.remove(i);
                            LNode::reduce_lut(&mut lut, i, val);
                        }
                        Value::Unknown | Value::Dynam(_) => (),
                    }
                }

                // check for duplicate inputs of the same source
                'outer: loop {
                    // we have to reset every time because the removals can mess up any range of
                    // indexes
                    let mut set = SmallMap::new();
                    for i in 0..inp.len() {
                        let p_inp = inp[i];
                        let equiv = self.backrefs.get_val(p_inp).unwrap();
                        match set.insert(equiv.p_self_equiv.inx(), i) {
                            Ok(()) => (),
                            Err(j) => {
                                let next_bw = lut.bw() / 2;
                                let mut next_lut = Awi::zero(NonZeroUsize::new(next_bw).unwrap());
                                let mut to = 0;
                                for k in 0..lut.bw() {
                                    let inx = InlAwi::from_usize(k);
                                    if inx.get(i).unwrap() == inx.get(j).unwrap() {
                                        next_lut.set(to, lut.get(k).unwrap()).unwrap();
                                        to += 1;
                                    }
                                }
                                self.optimizer
                                    .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                self.backrefs.remove_key(inp[j]).unwrap();
                                inp.remove(j);
                                lut = next_lut;
                                continue 'outer
                            }
                        }
                    }
                    break
                }

                // now check for input independence, e.x. for 0101 the 2^1 bit changes nothing
                let len = inp.len();
                for i in (0..len).rev() {
                    if (lut.bw() > 1) && LNode::reduce_independent_lut(&mut lut, i) {
                        // independent of the `i`th bit
                        let p_inp = inp.remove(i);
                        let equiv = self.backrefs.get_val(p_inp).unwrap();
                        self.optimizer
                            .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                        self.backrefs.remove_key(p_inp).unwrap();
                    }
                }

                // TODO the sorting should be done on equiv ptr comparison, or the LUT could be
                // canonicalized, or the equivalences could be found on existence of common
                // inputs without any sorting
                /*
                // sort inputs so that `LNode`s can be compared later
                let mut changed = false;
                // TODO want a more efficient sort that is tailored for basis rotations
                loop {
                    for i in 1..inp.len() {
                        if inp[i - 1] > inp[i] {
                            changed = true;
                            inp.swap(i - 1, i);
                            LNode::rotate_lut(&mut lut, i - 1, i);
                        }
                    }
                    if changed {
                        changed = false;
                    } else {
                        break
                    }
                }
                */

                // input independence automatically reduces all zeros and all ones LUTs, so just
                // need to check if the LUT is one bit for constant generation
                if lut.bw() == 1 {
                    let equiv = self.backrefs.get_val_mut(lnode.p_self).unwrap();
                    equiv.val = Value::Const(lut.to_bool());
                    // fix the `lut` to its new state, do this even if we are doing the constant
                    // optimization
                    *original_lut = lut;
                    return Ok(true)
                } else if (lut.bw() == 2) && lut.get(1).unwrap() {
                    // the only `lut.bw() == 2` cases that survive independence removal is identity
                    // and inversion. If it is identity, register this for forwarding
                    lnode.kind = LNodeKind::Copy(inp[0]);
                    self.optimizer
                        .insert(Optimization::ForwardEquiv(lnode.p_self));
                    return Ok(false)
                }
                // only at the very end do we consider `ConstUnknown` inputs, because if we
                // naively try to constify to `ConstUnknown` when the LUT does not reduce, we
                // miss cases where other inputs can cause it to have known output or even const
                // known output. We only set constify to `ConstUnknown` when all inputs are
                // `ConstUnknown`, which we need to recheck here. The `ConstUnknown` inputs
                // otherwise unfortunately need to be kept around for correct evaluation
                // behavior.
                if encountered_const_unknown {
                    let mut all_const_unknown = true;
                    let len = inp.len();
                    for i in 0..len {
                        let p_inp = inp[i];
                        let equiv = self.backrefs.get_val(p_inp).unwrap();
                        match equiv.val {
                            Value::ConstUnknown => (),
                            Value::Const(_) | Value::Dynam(_) | Value::Unknown => {
                                all_const_unknown = false;
                                break
                            }
                        }
                    }
                    if all_const_unknown {
                        let equiv = self.backrefs.get_val_mut(lnode.p_self).unwrap();
                        equiv.val = Value::ConstUnknown;
                        *original_lut = lut;
                        return Ok(true)
                    }
                }
                *original_lut = lut;
                false
            }
            LNodeKind::DynamicLut(inp, ref mut lut) => {
                // acquire LUT table inputs, convert to constants
                for lut_bit in lut.iter_mut() {
                    if let DynamicValue::Dynam(p) = lut_bit {
                        let equiv = self.backrefs.get_val(*p).unwrap();
                        match equiv.val {
                            Value::ConstUnknown => {
                                // we will be removing the input, mark it to be investigated
                                self.optimizer
                                    .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                self.backrefs.remove_key(*p).unwrap();
                                *lut_bit = DynamicValue::ConstUnknown;
                            }
                            Value::Const(val) => {
                                // we will be removing the input, mark it to be investigated
                                self.optimizer
                                    .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                self.backrefs.remove_key(*p).unwrap();
                                *lut_bit = DynamicValue::Const(val);
                            }
                            Value::Unknown | Value::Dynam(_) => (),
                        }
                    }
                }
                // acquire LUT inputs, for every constant input reduce the LUT
                let mut len = usize::from(u8::try_from(inp.len()).unwrap());
                for i in (0..len).rev() {
                    let p_inp = inp[i];
                    let equiv = self.backrefs.get_val(p_inp).unwrap();
                    match equiv.val {
                        Value::ConstUnknown => (),
                        Value::Const(val) => {
                            len -= 1;
                            // we will be removing the input, mark it to be investigated
                            self.optimizer
                                .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                            self.backrefs.remove_key(p_inp).unwrap();
                            inp.remove(i);

                            let (tmp, removed) = LNode::reduce_dynamic_lut(lut, i, val);
                            *lut = tmp;
                            for remove in removed {
                                let equiv = self.backrefs.get_val(remove).unwrap();
                                self.optimizer
                                    .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                self.backrefs.remove_key(remove).unwrap();
                            }
                        }
                        Value::Unknown | Value::Dynam(_) => (),
                    }
                }

                // FIXME
                /*
                // check for duplicate inputs of the same source
                'outer: loop {
                    // we have to reset every time because the removals can mess up any range of
                    // indexes
                    let mut set = SmallMap::new();
                    for i in 0..inp.len() {
                        let p_inp = inp[i];
                        let equiv = self.backrefs.get_val(p_inp).unwrap();
                        match set.insert(equiv.p_self_equiv.inx(), i) {
                            Ok(()) => (),
                            Err(j) => {
                                let next_bw = lut.len() / 2;
                                let mut next_lut = vec![DynamicValue::Unknown; next_bw];
                                let mut removed = Vec::with_capacity(next_bw);
                                let mut to = 0;
                                for k in 0..lut.len() {
                                    let inx = InlAwi::from_usize(k);
                                    if inx.get(i).unwrap() == inx.get(j).unwrap() {
                                        next_lut[to] = lut[k];
                                        to += 1;
                                    } else if let DynamicValue::Dynam(p_back) = lut[k] {
                                        removed.push(p_back);
                                    }
                                }
                                self.optimizer
                                    .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                self.backrefs.remove_key(inp[j]).unwrap();
                                inp.remove(j);
                                *lut = next_lut;
                                for p_back in removed {
                                    let equiv = self.backrefs.get_val(p_back).unwrap();
                                    self.optimizer
                                        .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                    self.backrefs.remove_key(p_back).unwrap();
                                }
                                continue 'outer
                            }
                        }
                    }
                    break
                }*/

                // now check for input independence, e.x. for 0101 the 2^1 bit changes nothing
                for i in (0..len).rev() {
                    if lut.len() > 1 {
                        if let Some((reduced, removed)) =
                            LNode::reduce_independent_dynamic_lut(&self.backrefs, lut, i)
                        {
                            // independent of the `i`th bit
                            *lut = reduced;
                            let p_inp = inp.remove(i);
                            let equiv = self.backrefs.get_val(p_inp).unwrap();
                            self.optimizer
                                .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                            self.backrefs.remove_key(p_inp).unwrap();
                            for remove in removed {
                                let equiv = self.backrefs.get_val(remove).unwrap();
                                self.optimizer
                                    .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                                self.backrefs.remove_key(remove).unwrap();
                            }
                        }
                    }
                }
                // sort inputs so that `LNode`s can be compared later
                // TODO?

                let w = NonZeroUsize::new(lut.len()).unwrap();

                // special case forwarding
                if w.get() == 1 {
                    let bit = lut[0];
                    match bit {
                        DynamicValue::ConstUnknown => {
                            let equiv = self.backrefs.get_val_mut(lnode.p_self).unwrap();
                            equiv.val = Value::ConstUnknown;
                            return Ok(true)
                        }
                        DynamicValue::Const(b) => {
                            let equiv = self.backrefs.get_val_mut(lnode.p_self).unwrap();
                            equiv.val = Value::Const(b);
                            return Ok(true)
                        }
                        DynamicValue::Dynam(bit) => {
                            lnode.kind = LNodeKind::Copy(bit);
                            self.optimizer
                                .insert(Optimization::ForwardEquiv(lnode.p_self));
                            return Ok(false)
                        }
                    }
                }

                let mut all_const_unknown = true;
                let mut all_const_known = true;
                for lut_bit in lut.iter() {
                    match lut_bit {
                        DynamicValue::ConstUnknown => {
                            all_const_known = false;
                        }
                        DynamicValue::Const(_) => {
                            all_const_unknown = false;
                        }
                        DynamicValue::Dynam(_) => {
                            all_const_unknown = false;
                            all_const_known = false;
                        }
                    }
                }

                if all_const_unknown {
                    let equiv = self.backrefs.get_val_mut(lnode.p_self).unwrap();
                    equiv.val = Value::ConstUnknown;
                    return Ok(true)
                }
                if all_const_known {
                    let mut awi_lut = Awi::zero(w);
                    for (i, lut_bit) in lut.iter().enumerate() {
                        if let DynamicValue::Const(b) = lut_bit {
                            awi_lut.set(i, *b).unwrap();
                        }
                    }
                    if (w.get() == 2) && awi_lut.get(1).unwrap() {
                        lnode.kind = LNodeKind::Copy(inp[0]);
                        self.optimizer
                            .insert(Optimization::ForwardEquiv(lnode.p_self));
                    } else {
                        let inp = mem::take(inp);
                        lnode.kind = LNodeKind::Lut(inp, awi_lut);
                    }
                }
                false
            }
        })
    }

    /// Assigns `Const` result if possible.
    /// Returns if a `Const` result was assigned.
    pub fn const_eval_tnode(&mut self, p_tnode: PTNode) -> bool {
        let tnode = self.tnodes.get(p_tnode).unwrap();
        // TODO have another parameter to enable const through some amount of delay
        if tnode.delay().is_zero() {
            let p_self = tnode.p_self;
            let p_driver = tnode.p_driver;
            let equiv = self.backrefs.get_val(p_driver).unwrap();
            if equiv.val.is_const() {
                self.backrefs.get_val_mut(p_self).unwrap().val = equiv.val;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// If there exists any equivalence with no checks applied, this should
    /// always be applied before any further optimizations are applied, so that
    /// `RemoveUnused` and `ConstPropogate` can be handled before any other
    /// optimization
    pub fn preinvestigate_equiv(&mut self, p_equiv: PBack) -> Result<(), Error> {
        let mut non_self_rc = 0usize;
        let equiv = self.backrefs.get_val(p_equiv).unwrap();
        let mut is_const = equiv.val.is_const();
        let mut possible_drivers = false;
        let mut adv = self.backrefs.advancer_surject(p_equiv);
        while let Some(p_back) = adv.advance(&self.backrefs) {
            let referent = *self.backrefs.get_key(p_back).unwrap();
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisTNode(p_tnode) => {
                    possible_drivers = true;
                    // avoid checking more if it was already determined to be constant
                    if !is_const && self.const_eval_tnode(p_tnode) {
                        is_const = true;
                    }
                }
                Referent::ThisLNode(p_lnode) => {
                    possible_drivers = true;
                    // avoid checking more if it was already determined to be constant
                    if !is_const && self.const_eval_lnode(p_lnode)? {
                        is_const = true;
                    }
                }
                Referent::ThisStateBit(p_state, _) => {
                    let state = &self.stator.states[p_state];
                    // the state bits can always be disregarded on a per-lnode basis unless they are
                    // being used externally
                    if state.extern_rc != 0 {
                        non_self_rc += 1;
                    }
                }
                Referent::Input(_) => non_self_rc += 1,
                Referent::Driver(p_driver) => {
                    // the way `Driver` networks with no real dependencies will work, is
                    // that const propogation and other simplifications will eventually result
                    // in a single node equivalence that drives itself, which we can remove
                    let p_back_driver = self.tnodes.get(p_driver).unwrap().p_self;
                    if !self.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
                        non_self_rc += 1;
                    }
                }
                Referent::ThisRNode(p_rnode) => {
                    let rnode = self.notary.rnodes().get(p_rnode).unwrap().1;
                    if !rnode.read_only() {
                        possible_drivers = true;
                    }
                    non_self_rc += 1;
                }
            }
        }

        if non_self_rc == 0 {
            self.optimizer.insert(Optimization::RemoveEquiv(p_equiv));
        } else if is_const || (!possible_drivers) {
            // if an equivalence has no possible `TNode`, `LNode`, or `RNode` drivers, the
            // value is converted to its const version
            self.optimizer.insert(Optimization::ConstifyEquiv(p_equiv));
        } else {
            self.optimizer
                .insert(Optimization::InvestigateEquiv0(p_equiv));
        }
        Ok(())
    }

    /// Does not perform the final step
    /// `ensemble.backrefs.remove(lnode.p_self).unwrap()` which is important for
    /// `Advancer`s.
    pub fn remove_state_bit_not_p_self(&mut self, p_state: PState, i_bit: usize) {
        let p_bit = self
            .stator
            .states
            .get_mut(p_state)
            .unwrap()
            .p_self_bits
            .get_mut(i_bit)
            .unwrap()
            .take()
            .unwrap();
        let p_equiv = self.backrefs.get_val(p_bit).unwrap().p_self_equiv;
        self.optimizer
            .insert(Optimization::InvestigateUsed(p_equiv));
    }

    /// Does not perform the final step
    /// `ensemble.backrefs.remove(lnode.p_self).unwrap()` which is important for
    /// `Advancer`s.
    pub fn remove_lnode_not_p_self(&mut self, p_lnode: PLNode) {
        let lnode = self.lnodes.remove(p_lnode).unwrap();
        lnode.inputs(|inp| {
            let p_equiv = self.backrefs.get_val(inp).unwrap().p_self_equiv;
            self.optimizer
                .insert(Optimization::InvestigateUsed(p_equiv));
            self.backrefs.remove_key(inp).unwrap();
        });
    }

    /// Does not perform the final step
    /// `ensemble.backrefs.remove(tnode.p_self).unwrap()` which is important for
    /// `Advancer`s.
    pub fn remove_tnode_not_p_self(&mut self, p_tnode: PTNode) {
        let tnode = self.tnodes.remove(p_tnode).unwrap();
        let p_equiv = self.backrefs.get_val(tnode.p_driver).unwrap().p_self_equiv;
        self.optimizer
            .insert(Optimization::InvestigateUsed(p_equiv));
        self.backrefs.remove_key(tnode.p_driver).unwrap();
    }

    /// Removes all states, optimizes, and shrinks allocations
    pub fn optimize_all(&mut self) -> Result<(), Error> {
        // empty current events because they will be invalidated and shrunk
        self.restart_request_phase()?;
        self.force_remove_all_states().unwrap();
        // need to preinvestigate everything before starting a priority loop
        let mut adv = self.backrefs.advancer();
        while let Some(p_back) = adv.advance(&self.backrefs) {
            if let Referent::ThisEquiv = self.backrefs.get_key(p_back).unwrap() {
                self.preinvestigate_equiv(p_back)?;
            }
        }
        while let Some(p_optimization) = self.optimizer.optimizations.min() {
            self.optimize(p_optimization)?;
        }
        self.recast_all_internal_ptrs()
    }

    pub fn optimize(&mut self, p_optimization: POpt) -> Result<(), Error> {
        let optimization = self
            .optimizer
            .optimizations
            .remove(p_optimization)
            .unwrap()
            .0;
        match optimization {
            Optimization::Preinvestigate(p_equiv) => {
                self.preinvestigate_equiv(p_equiv)?;
            }
            Optimization::RemoveEquiv(p_back) => {
                let p_equiv = if let Some(equiv) = self.backrefs.get_val(p_back) {
                    equiv.p_self_equiv
                } else {
                    return Ok(())
                };
                // remove all associated LNodes first
                let mut adv = self.backrefs.advancer_surject(p_back);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    match *self.backrefs.get_key(p_back).unwrap() {
                        Referent::ThisEquiv => (),
                        Referent::ThisStateBit(p_state, bit_i) => {
                            self.remove_state_bit_not_p_self(p_state, bit_i);
                        }
                        Referent::ThisLNode(p_lnode) => {
                            self.remove_lnode_not_p_self(p_lnode);
                        }
                        Referent::ThisTNode(p_tnode) => {
                            self.remove_tnode_not_p_self(p_tnode);
                        }
                        _ => unreachable!(),
                    }
                }
                // remove the equivalence
                self.backrefs.remove(p_equiv).unwrap();
            }
            Optimization::ForwardEquiv(p_ident) => {
                let p_source = if let Some(referent) = self.backrefs.get_key(p_ident) {
                    if let Referent::ThisLNode(p_lnode) = referent {
                        let lnode = &self.lnodes[p_lnode];
                        if let LNodeKind::Copy(inp) = lnode.kind {
                            // do not use directly, use the `p_self_equiv` since this backref will
                            // be removed when `p_ident` is process in
                            // the loop
                            self.backrefs.get_val(inp).unwrap().p_self_equiv
                        } else {
                            unreachable!()
                        }
                    } else {
                        unreachable!()
                    }
                } else {
                    return Ok(())
                };
                let mut adv = self.backrefs.advancer_surject(p_ident);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    let referent = *self.backrefs.get_key(p_back).unwrap();
                    match referent {
                        Referent::ThisEquiv => (),
                        Referent::ThisLNode(p_lnode) => {
                            self.remove_lnode_not_p_self(p_lnode);
                        }
                        Referent::ThisTNode(p_tnode) => {
                            self.remove_tnode_not_p_self(p_tnode);
                        }
                        Referent::ThisStateBit(p_state, i_bit) => {
                            let p_bit = self.stator.states[p_state].p_self_bits[i_bit]
                                .as_mut()
                                .unwrap();
                            let p_back_new = self
                                .backrefs
                                .insert_key(p_source, Referent::ThisStateBit(p_state, i_bit))
                                .unwrap();
                            *p_bit = p_back_new;
                        }
                        Referent::Input(p_input) => {
                            let lnode = self.lnodes.get_mut(p_input).unwrap();
                            let mut found = false;
                            lnode.inputs_mut(|inp| {
                                if *inp == p_back {
                                    let p_back_new = self
                                        .backrefs
                                        .insert_key(p_source, Referent::Input(p_input))
                                        .unwrap();
                                    *inp = p_back_new;
                                    found = true;
                                }
                            });
                            assert!(found);
                        }
                        Referent::Driver(p_driver) => {
                            let tnode = self.tnodes.get_mut(p_driver).unwrap();
                            debug_assert_eq!(tnode.p_driver, p_back);
                            let p_back_new = self
                                .backrefs
                                .insert_key(p_source, Referent::Driver(p_driver))
                                .unwrap();
                            tnode.p_driver = p_back_new;
                        }
                        Referent::ThisRNode(p_rnode) => {
                            let rnode = self.notary.get_rnode_by_p_rnode_mut(p_rnode).unwrap();
                            let mut found = false;
                            if let Some(bits) = rnode.bits_mut() {
                                for bit in bits {
                                    if let Some(bit) = bit {
                                        if *bit == p_back {
                                            let p_back_new = self
                                                .backrefs
                                                .insert_key(p_source, Referent::ThisRNode(p_rnode))
                                                .unwrap();
                                            *bit = p_back_new;
                                            found = true;
                                            break
                                        }
                                    }
                                }
                            }
                            assert!(found);
                        }
                    }
                }
                // remove the equivalence, since everything should be forwarded and nothing
                // depends on the identity equiv.
                self.backrefs.remove(p_ident).unwrap();
            }
            Optimization::ConstifyEquiv(p_back) => {
                if !self.backrefs.contains(p_back) {
                    return Ok(())
                };
                // for removing `ThisLNode` safely
                let mut remove = SmallVec::<[PBack; 16]>::new();
                // remove all associated LNodes
                let mut adv = self.backrefs.advancer_surject(p_back);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    match *self.backrefs.get_key(p_back).unwrap() {
                        Referent::ThisEquiv => (),
                        Referent::ThisLNode(p_lnode) => {
                            self.remove_lnode_not_p_self(p_lnode);
                            remove.push(p_back);
                        }
                        Referent::ThisTNode(p_tnode) => {
                            self.remove_tnode_not_p_self(p_tnode);
                            remove.push(p_back);
                        }
                        Referent::ThisStateBit(..) => (),
                        Referent::Input(p_inp) => {
                            self.optimizer.insert(Optimization::InvestigateConst(p_inp));
                        }
                        Referent::Driver(p_driver) => {
                            self.optimizer
                                .insert(Optimization::InvestigateDriverConst(p_driver));
                        }
                        Referent::ThisRNode(_) => (),
                    }
                }
                for p_back in remove {
                    self.backrefs.remove_key(p_back).unwrap();
                }
            }
            Optimization::RemoveLNode(p_back) => {
                if !self.backrefs.contains(p_back) {
                    return Ok(())
                }
                todo!()
            }
            Optimization::InvestigateUsed(p_back) => {
                if !self.backrefs.contains(p_back) {
                    return Ok(())
                };
                let mut found_use = false;
                let mut adv = self.backrefs.advancer_surject(p_back);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    let referent = *self.backrefs.get_key(p_back).unwrap();
                    match referent {
                        Referent::ThisEquiv => (),
                        Referent::ThisLNode(_) => (),
                        Referent::ThisTNode(_) => (),
                        Referent::ThisStateBit(p_state, _) => {
                            let state = &self.stator.states[p_state];
                            // the state bits can always be disregarded on a per-lnode basis unless
                            // they are being used externally
                            if state.extern_rc != 0 {
                                found_use = true;
                            }
                        }
                        Referent::Input(_) => {
                            found_use = true;
                            break
                        }
                        Referent::Driver(p_driver) => {
                            let p_back_driver = self.tnodes.get(p_driver).unwrap().p_self;
                            if !self.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
                                found_use = true;
                                break
                            }
                        }
                        Referent::ThisRNode(_) => {
                            found_use = true;
                            break
                        }
                    }
                }
                if !found_use {
                    self.optimizer.insert(Optimization::RemoveEquiv(p_back));
                }
            }
            Optimization::InvestigateConst(p_lnode) => {
                if !self.lnodes.contains(p_lnode) {
                    return Ok(())
                };
                if self.const_eval_lnode(p_lnode)? {
                    self.optimizer.insert(Optimization::ConstifyEquiv(
                        self.lnodes.get(p_lnode).unwrap().p_self,
                    ));
                }
            }
            Optimization::InvestigateDriverConst(p_tnode) => {
                if !self.tnodes.contains(p_tnode) {
                    return Ok(())
                };
                if self.const_eval_tnode(p_tnode) {
                    self.optimizer.insert(Optimization::ConstifyEquiv(
                        self.tnodes.get(p_tnode).unwrap().p_self,
                    ));
                }
            }
            Optimization::InvestigateEquiv0(_p_back) => {
                /*if !self.backrefs.contains(p_back) {
                    return
                };*/
                // TODO eliminate equal LNodes, combine equal equivalences etc.

                // TODO compare LNodes
                // TODO compress inverters by inverting inx table
                // TODO fusion of structures like
                // H(F(a, b), G(a, b)) definitely or any case like H(F(a, b), a)
                // with common inputs
            }
        }
        Ok(())
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}
