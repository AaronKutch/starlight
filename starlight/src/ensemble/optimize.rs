use std::num::NonZeroUsize;

use awint::{
    awint_dag::{
        smallvec::SmallVec,
        triple_arena::{Advancer, Ptr},
        PState,
    },
    Awi, InlAwi,
};

use crate::{
    ensemble::{Ensemble, PBack, PLNode, PTNode, Referent, TNode, Value},
    triple_arena::{ptr_struct, OrdArena},
    SmallMap,
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
    /// This needs to point to the `Referent::ThisTNode` of the identity
    /// `TNode`. If an equivalence is an identity function, any referents should
    /// use its inputs instead. This is high priority because the principle
    /// source of a value needs to be known for various optimizations
    /// involving deduplication to work (such as early LUT simplification), and
    /// also because it eliminates useless identities early.
    ForwardEquiv(PBack),
    /// Removes all `TNode`s from an equivalence that has had a constant
    /// assigned to it, and notifies all referents.
    ConstifyEquiv(PBack),
    /// Removes a `TNode` because there is at least one other `TNode` in the
    /// equivalence that is stricly better
    RemoveTNode(PBack),
    /// If a backref is removed, investigate this equivalence. Note that
    /// `InvestigateUsed`s overwrite each other when multiple ones are fired on
    /// the same equivalence.
    // TODO should this one be moved up? Needs to be benchmarked.
    InvestigateUsed(PBack),
    /// If an input was constified
    InvestigateConst(PTNode),
    /// If a driver was constified
    InvestigateLoopDriverConst(PLNode),
    /// The optimization state that equivalences are set to after the
    /// preinvestigation finds nothing
    InvestigateEquiv0(PBack),
    //InvertInput
    // (?) not sure if fusion + ordinary `const_eval_tnode` handles all cases cleanly,
    // might only do fission for routing
    //Fission
    // A fusion involving the number of inputs that will result
    //Fusion(u8, PBack)
}

#[derive(Debug, Clone)]
pub struct Optimizer {
    pub gas: u64,
    pub optimizations: OrdArena<POpt, Optimization, ()>,
}

impl Optimizer {
    pub fn new() -> Self {
        Self {
            gas: 0,
            optimizations: OrdArena::new(),
        }
    }

    pub fn insert(&mut self, optimization: Optimization) {
        let _ = self.optimizations.insert(optimization, ());
    }
}

impl Ensemble {
    /// Removes all `Const` inputs and assigns `Const` result if possible.
    /// Returns if a `Const` result was assigned (`Optimization::ConstifyEquiv`
    /// needs to be run by the caller).
    pub fn const_eval_tnode(&mut self, p_tnode: PTNode) -> bool {
        let tnode = self.tnodes.get_mut(p_tnode).unwrap();
        if let Some(original_lut) = &tnode.lut {
            let mut lut = original_lut.clone();
            // acquire LUT inputs, for every constant input reduce the LUT
            let len = usize::from(u8::try_from(tnode.inp.len()).unwrap());
            for i in (0..len).rev() {
                let p_inp = tnode.inp[i];
                let equiv = self.backrefs.get_val(p_inp).unwrap();
                if let Value::Const(val) = equiv.val {
                    // we will be removing the input, mark it to be investigated
                    self.optimizer
                        .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                    self.backrefs.remove_key(p_inp).unwrap();
                    tnode.inp.remove(i);

                    lut = TNode::reduce_lut(&lut, i, val);
                }
            }

            // check for duplicate inputs of the same source
            'outer: loop {
                // we have to reset every time because the removals can mess up any range of
                // indexes
                let mut set = SmallMap::new();
                for i in 0..tnode.inp.len() {
                    let p_inp = tnode.inp[i];
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
                            self.backrefs.remove_key(tnode.inp[j]).unwrap();
                            tnode.inp.remove(j);
                            lut = next_lut;
                            continue 'outer
                        }
                    }
                }
                break
            }

            // now check for input independence, e.x. for 0101 the 2^1 bit changes nothing
            let len = tnode.inp.len();
            for i in (0..len).rev() {
                if lut.bw() > 1 {
                    if let Some(reduced) = TNode::reduce_independent_lut(&lut, i) {
                        // independent of the `i`th bit
                        lut = reduced;
                        let p_inp = tnode.inp.remove(i);
                        let equiv = self.backrefs.get_val(p_inp).unwrap();
                        self.optimizer
                            .insert(Optimization::InvestigateUsed(equiv.p_self_equiv));
                        self.backrefs.remove_key(p_inp).unwrap();
                    }
                }
            }
            // sort inputs so that `TNode`s can be compared later
            // TODO?

            // input independence automatically reduces all zeros and all ones LUTs, so just
            // need to check if the LUT is one bit for constant generation
            if lut.bw() == 1 {
                let equiv = self.backrefs.get_val_mut(tnode.p_self).unwrap();
                equiv.val = Value::Const(lut.to_bool());
                // fix the `lut` to its new state, do this even if we are doing the constant
                // optimization
                tnode.lut = Some(lut);
                true
            } else if (lut.bw() == 2) && lut.get(1).unwrap() {
                // the only `lut.bw() == 2` cases that survive independence removal is identity
                // and inversion. If it is identity, register this for forwarding
                tnode.lut = None;
                self.optimizer
                    .insert(Optimization::ForwardEquiv(tnode.p_self));
                false
            } else {
                tnode.lut = Some(lut);
                false
            }
        } else if tnode.inp.len() == 1 {
            // wire propogation
            let input_equiv = self.backrefs.get_val_mut(tnode.inp[0]).unwrap();
            if let Value::Const(val) = input_equiv.val {
                let equiv = self.backrefs.get_val_mut(tnode.p_self).unwrap();
                equiv.val = Value::Const(val);
                self.optimizer
                    .insert(Optimization::ConstifyEquiv(equiv.p_self_equiv));
                true
            } else {
                self.optimizer
                    .insert(Optimization::ForwardEquiv(tnode.p_self));
                false
            }
        } else {
            false
        }
    }

    /// Assigns `Const` result if possible.
    /// Returns if a `Const` result was assigned.
    pub fn const_eval_lnode(&mut self, p_lnode: PLNode) -> bool {
        let lnode = self.lnodes.get(p_lnode).unwrap();
        let p_self = lnode.p_self;
        let p_driver = lnode.p_driver;
        let equiv = self.backrefs.get_val(p_driver).unwrap();
        if let Value::Const(val) = equiv.val {
            self.backrefs.get_val_mut(p_self).unwrap().val = Value::Const(val);
            true
        } else {
            false
        }
    }

    /// If there exists any equivalence with no checks applied, this should
    /// always be applied before any further optimizations are applied, so that
    /// `RemoveUnused` and `ConstPropogate` can be handled before any other
    /// optimization
    pub fn preinvestigate_equiv(&mut self, p_equiv: PBack) {
        let mut non_self_rc = 0usize;
        let equiv = self.backrefs.get_val(p_equiv).unwrap();
        let mut is_const = matches!(equiv.val, Value::Const(_));
        let mut adv = self.backrefs.advancer_surject(p_equiv);
        while let Some(p_back) = adv.advance(&self.backrefs) {
            let referent = *self.backrefs.get_key(p_back).unwrap();
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisLNode(p_lnode) => {
                    // avoid checking more if it was already determined to be constant
                    if !is_const && self.const_eval_lnode(p_lnode) {
                        is_const = true;
                    }
                }
                Referent::ThisTNode(p_tnode) => {
                    // avoid checking more if it was already determined to be constant
                    if !is_const && self.const_eval_tnode(p_tnode) {
                        is_const = true;
                    }
                }
                Referent::ThisStateBit(p_state, _) => {
                    let state = &self.stator.states[p_state];
                    // the state bits can always be disregarded on a per-tnode basis unless they are
                    // being used externally
                    if state.extern_rc != 0 {
                        non_self_rc += 1;
                    }
                }
                Referent::Input(_) => non_self_rc += 1,
                Referent::LoopDriver(p_driver) => {
                    // the way `LoopDriver` networks with no real dependencies will work, is
                    // that const propogation and other simplifications will eventually result
                    // in a single node equivalence that drives itself, which we can remove
                    let p_back_driver = self.lnodes.get(p_driver).unwrap().p_self;
                    if !self.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
                        non_self_rc += 1;
                    }

                    // TODO check for const through loop, but there should be a
                    // parameter to enable
                }
                Referent::Note(_) => non_self_rc += 1,
            }
        }
        if non_self_rc == 0 {
            self.optimizer.insert(Optimization::RemoveEquiv(p_equiv));
        } else if is_const {
            self.optimizer.insert(Optimization::ConstifyEquiv(p_equiv));
        } else {
            self.optimizer
                .insert(Optimization::InvestigateEquiv0(p_equiv));
        }
    }

    /// Does not perform the final step
    /// `ensemble.backrefs.remove(tnode.p_self).unwrap()` which is important for
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
    /// `ensemble.backrefs.remove(tnode.p_self).unwrap()` which is important for
    /// `Advancer`s.
    pub fn remove_tnode_not_p_self(&mut self, p_tnode: PTNode) {
        let tnode = self.tnodes.remove(p_tnode).unwrap();
        for inp in tnode.inp {
            let p_equiv = self.backrefs.get_val(inp).unwrap().p_self_equiv;
            self.optimizer
                .insert(Optimization::InvestigateUsed(p_equiv));
            self.backrefs.remove_key(inp).unwrap();
        }
    }

    /// Does not perform the final step
    /// `ensemble.backrefs.remove(lnode.p_self).unwrap()` which is important for
    /// `Advancer`s.
    pub fn remove_lnode_not_p_self(&mut self, p_lnode: PLNode) {
        let lnode = self.lnodes.remove(p_lnode).unwrap();
        let p_equiv = self.backrefs.get_val(lnode.p_driver).unwrap().p_self_equiv;
        self.optimizer
            .insert(Optimization::InvestigateUsed(p_equiv));
        self.backrefs.remove_key(lnode.p_driver).unwrap();
    }

    pub fn optimize_all(&mut self) {
        // need to preinvestigate everything before starting a priority loop
        let mut adv = self.backrefs.advancer();
        while let Some(p_back) = adv.advance(&self.backrefs) {
            if let Referent::ThisEquiv = self.backrefs.get_key(p_back).unwrap() {
                self.preinvestigate_equiv(p_back);
            }
        }
        while let Some(p_optimization) = self.optimizer.optimizations.min() {
            self.optimize(p_optimization);
        }
    }

    pub fn optimize(&mut self, p_optimization: POpt) {
        let optimization = self
            .optimizer
            .optimizations
            .remove(p_optimization)
            .unwrap()
            .0;
        match optimization {
            Optimization::Preinvestigate(p_equiv) => {
                self.preinvestigate_equiv(p_equiv);
            }
            Optimization::RemoveEquiv(p_back) => {
                let p_equiv = if let Some(equiv) = self.backrefs.get_val(p_back) {
                    equiv.p_self_equiv
                } else {
                    return
                };
                // remove all associated TNodes first
                let mut adv = self.backrefs.advancer_surject(p_back);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    match self.backrefs.get_key(p_back).unwrap() {
                        Referent::ThisEquiv => (),
                        Referent::ThisStateBit(p_state, bit_i) => {
                            self.remove_state_bit_not_p_self(*p_state, *bit_i);
                        }
                        Referent::ThisTNode(p_tnode) => {
                            self.remove_tnode_not_p_self(*p_tnode);
                        }
                        Referent::ThisLNode(p_lnode) => {
                            self.remove_lnode_not_p_self(*p_lnode);
                        }
                        _ => unreachable!(),
                    }
                }
                // remove the equivalence
                self.backrefs.remove(p_equiv).unwrap();
            }
            Optimization::ForwardEquiv(p_ident) => {
                let p_source = if let Some(referent) = self.backrefs.get_key(p_ident) {
                    if let Referent::ThisTNode(p_tnode) = referent {
                        let tnode = &self.tnodes[p_tnode];
                        assert_eq!(tnode.inp.len(), 1);
                        // do not use directly, use the `p_self_equiv` since this backref will be
                        // removed when `p_ident` is process in the loop
                        let p_back = tnode.inp[0];
                        self.backrefs.get_val(p_back).unwrap().p_self_equiv
                    } else {
                        unreachable!()
                    }
                } else {
                    return
                };
                let mut adv = self.backrefs.advancer_surject(p_ident);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    let referent = *self.backrefs.get_key(p_back).unwrap();
                    match referent {
                        Referent::ThisEquiv => (),
                        Referent::ThisTNode(p_tnode) => {
                            self.remove_tnode_not_p_self(p_tnode);
                        }
                        Referent::ThisLNode(p_lnode) => {
                            self.remove_lnode_not_p_self(p_lnode);
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
                            let tnode = self.tnodes.get_mut(p_input).unwrap();
                            let mut found = false;
                            for inp in &mut tnode.inp {
                                if *inp == p_back {
                                    let p_back_new = self
                                        .backrefs
                                        .insert_key(p_source, Referent::Input(p_input))
                                        .unwrap();
                                    *inp = p_back_new;
                                    found = true;
                                    break
                                }
                            }
                            assert!(found);
                        }
                        Referent::LoopDriver(p_driver) => {
                            let lnode = self.lnodes.get_mut(p_driver).unwrap();
                            assert_eq!(lnode.p_driver, p_back);
                            let p_back_new = self
                                .backrefs
                                .insert_key(p_source, Referent::LoopDriver(p_driver))
                                .unwrap();
                            lnode.p_driver = p_back_new;
                        }
                        Referent::Note(p_note) => {
                            // here we see a major advantage of the backref system
                            let note = self.notes.get_mut(p_note).unwrap();
                            let mut found = false;
                            for bit in &mut note.bits {
                                if let Some(bit) = bit {
                                    if *bit == p_back {
                                        let p_back_new = self
                                            .backrefs
                                            .insert_key(p_source, Referent::Note(p_note))
                                            .unwrap();
                                        *bit = p_back_new;
                                        found = true;
                                        break
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
                    return
                };
                // for removing `ThisTNode` safely
                let mut remove = SmallVec::<[PBack; 16]>::new();
                // remove all associated TNodes
                let mut adv = self.backrefs.advancer_surject(p_back);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    match self.backrefs.get_key(p_back).unwrap() {
                        Referent::ThisEquiv => (),
                        Referent::ThisTNode(p_tnode) => {
                            self.remove_tnode_not_p_self(*p_tnode);
                            remove.push(p_back);
                        }
                        Referent::ThisLNode(p_lnode) => {
                            self.remove_lnode_not_p_self(*p_lnode);
                            remove.push(p_back);
                        }
                        Referent::ThisStateBit(..) => (),
                        Referent::Input(p_inp) => {
                            self.optimizer
                                .insert(Optimization::InvestigateConst(*p_inp));
                        }
                        Referent::LoopDriver(p_driver) => {
                            self.optimizer
                                .insert(Optimization::InvestigateLoopDriverConst(*p_driver));
                        }
                        Referent::Note(_) => (),
                    }
                }
                for p_back in remove {
                    self.backrefs.remove_key(p_back).unwrap();
                }
            }
            Optimization::RemoveTNode(p_back) => {
                if !self.backrefs.contains(p_back) {
                    return
                }
                todo!()
            }
            Optimization::InvestigateUsed(p_back) => {
                if !self.backrefs.contains(p_back) {
                    return
                };
                let mut found_use = false;
                let mut adv = self.backrefs.advancer_surject(p_back);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    let referent = *self.backrefs.get_key(p_back).unwrap();
                    match referent {
                        Referent::ThisEquiv => (),
                        Referent::ThisTNode(_) => (),
                        Referent::ThisLNode(_) => (),
                        Referent::ThisStateBit(p_state, _) => {
                            let state = &self.stator.states[p_state];
                            // the state bits can always be disregarded on a per-tnode basis unless
                            // they are being used externally
                            if state.extern_rc != 0 {
                                found_use = true;
                            }
                        }
                        Referent::Input(_) => {
                            found_use = true;
                            break
                        }
                        Referent::LoopDriver(p_driver) => {
                            let p_back_driver = self.lnodes.get(p_driver).unwrap().p_self;
                            if !self.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
                                found_use = true;
                                break
                            }
                        }
                        Referent::Note(_) => {
                            found_use = true;
                            break
                        }
                    }
                }
                if !found_use {
                    self.optimizer.insert(Optimization::RemoveEquiv(p_back));
                }
            }
            Optimization::InvestigateConst(p_tnode) => {
                if !self.tnodes.contains(p_tnode) {
                    return
                };
                if self.const_eval_tnode(p_tnode) {
                    self.optimizer.insert(Optimization::ConstifyEquiv(
                        self.tnodes.get(p_tnode).unwrap().p_self,
                    ));
                }
            }
            Optimization::InvestigateLoopDriverConst(p_lnode) => {
                if !self.lnodes.contains(p_lnode) {
                    return
                };
                if self.const_eval_lnode(p_lnode) {
                    self.optimizer.insert(Optimization::ConstifyEquiv(
                        self.lnodes.get(p_lnode).unwrap().p_self,
                    ));
                }
            }
            Optimization::InvestigateEquiv0(_p_back) => {
                /*if !self.backrefs.contains(p_back) {
                    return
                };*/
                // TODO eliminate equal TNodes, combine equal equivalences etc.

                // TODO compare TNodes
                // TODO compress inverters by inverting inx table
                // TODO fusion of structures like
                // H(F(a, b), G(a, b)) definitely or any case like H(F(a, b), a)
                // with common inputs
            }
        }
    }
}

impl Default for Optimizer {
    fn default() -> Self {
        Self::new()
    }
}
