use std::num::NonZeroUsize;

use awint::{
    awint_dag::{smallvec::SmallVec, triple_arena::Advancer},
    ExtAwi,
};

use crate::{
    triple_arena::{ptr_struct, OrdArena},
    PBack, PTNode, Referent, TDag, Value,
};

ptr_struct!(POpt);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct CostU8(pub u8);

/// These variants must occur generally in order of easiest and most affecting
/// to hardest and computationally expensive, so  that things like removing
/// unused nodes happens before wasting time on the harder optimizations.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Optimization {
    /// Removes an entire equivalence class because it is unused
    RemoveEquiv(PBack),
    /// If an equivalence is an identity function, any dependents should use its
    /// inputs instead. This is high priority because the principle source of a
    /// value needs to be known for various optimizations to work such as LUT
    /// input duplicates.
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
    /// The optimization state that equivalences are set to after the
    /// preinvestigation finds nothing
    InvestigateEquiv0(PBack),
    // A Lookup table equivalence that does not increase Lookup Table size
    //CompressLut(PTNode)
}

/// This struct implements a queue for simple simplifications of `TDag`s
pub struct Optimizer {
    pub gas: u64,
    pub optimizations: OrdArena<POpt, Optimization, ()>,
}

impl Optimizer {
    pub fn new(gas: u64) -> Self {
        // TODO get simplifications for all nodes.
        Self {
            gas,
            optimizations: OrdArena::new(),
        }
    }

    /// Removes all `Const` inputs and assigns `Const` result if possible.
    /// Returns if a `Const` result was assigned.
    pub fn const_eval_tnode(&mut self, t_dag: &mut TDag, p_tnode: PTNode) -> bool {
        let tnode = t_dag.tnodes.get_mut(p_tnode).unwrap();
        if let Some(original_lut) = &tnode.lut {
            let mut lut = original_lut.clone();
            // acquire LUT inputs, for every constant input reduce the LUT
            let len = tnode.inp.len();
            for i in (0..len).rev() {
                let p_inp = tnode.inp[i];
                let equiv = t_dag.backrefs.get_val(p_inp).unwrap();
                if let Value::Const(val) = equiv.val {
                    // we will be removing the input, mark it to be investigated
                    let _ = self
                        .optimizations
                        .insert(Optimization::InvestigateUsed(equiv.p_self_equiv), ());
                    t_dag.backrefs.remove_key(p_inp).unwrap();
                    tnode.inp.remove(i);

                    // reduction of the LUT
                    let next_bw = lut.bw() / 2;
                    let mut next_lut = ExtAwi::zero(NonZeroUsize::new(next_bw).unwrap());
                    let w = 1 << i;
                    let mut from = 0;
                    let mut to = 0;
                    while to < next_bw {
                        next_lut
                            .field(to, &lut, if val { from + w } else { from }, w)
                            .unwrap();
                        from += 2 * w;
                        to += w;
                    }
                    lut = next_lut;
                }
            }

            // check for inputs of the same source
            /*let len = tnode.inp.len();
            for i in (0..len).rev() {
                let p_inp = tnode.inp[i];
                let equiv = t_dag.backrefs.get_val(p_inp).unwrap();
                if let Value::Const(val) = equiv.val {
                    // we will be removing the input, mark it to be investigated
                    let _ = self
                        .optimizations
                        .insert(Optimization::InvestigateUsed(equiv.p_self_equiv), ());
                    t_dag.backrefs.remove_key(p_inp).unwrap();
                    tnode.inp.remove(i);

                    // reduction of the LUT
                    let next_bw = lut.bw() / 2;
                    let mut next_lut = ExtAwi::zero(NonZeroUsize::new(next_bw).unwrap());
                    let w = 1 << i;
                    let mut from = 0;
                    let mut to = 0;
                    while to < next_bw {
                        next_lut
                            .field(to, &lut, if val { from + w } else { from }, w)
                            .unwrap();
                        from += 2 * w;
                        to += w;
                    }
                    lut = next_lut;
                }
            }*/

            // now check for input independence, e.x. for 0101 the 2^1 bit changes nothing
            let len = tnode.inp.len();
            for i in (0..len).rev() {
                let next_bw = lut.bw() / 2;
                if let Some(nzbw) = NonZeroUsize::new(next_bw) {
                    let mut tmp0 = ExtAwi::zero(nzbw);
                    let mut tmp1 = ExtAwi::zero(nzbw);
                    let w = 1 << i;
                    // LUT if the `i`th bit were 0
                    let mut from = 0;
                    let mut to = 0;
                    while to < next_bw {
                        tmp0.field(to, &lut, from, w).unwrap();
                        from += 2 * w;
                        to += w;
                    }
                    // LUT if the `i`th bit were 1
                    from = w;
                    to = 0;
                    while to < next_bw {
                        tmp1.field(to, &lut, from, w).unwrap();
                        from += 2 * w;
                        to += w;
                    }
                    if tmp0 == tmp1 {
                        // independent of the `i`th bit
                        lut = tmp0;
                        let p_inp = tnode.inp.remove(i);
                        let equiv = t_dag.backrefs.get_val(p_inp).unwrap();
                        let _ = self
                            .optimizations
                            .insert(Optimization::InvestigateUsed(equiv.p_self_equiv), ());
                        t_dag.backrefs.remove_key(p_inp).unwrap();
                    }
                } else {
                    // LUT is 1 bit
                    break
                }
            }

            // input independence automatically reduces all zeros and all ones LUTs, so just
            // need to check if the LUT is one bit for constant generation
            if lut.bw() == 1 {
                let equiv = t_dag.backrefs.get_val_mut(tnode.p_self).unwrap();
                equiv.val = Value::Const(lut.to_bool());
                let _ = self
                    .optimizations
                    .insert(Optimization::ConstifyEquiv(equiv.p_self_equiv), ());
                // fix the `lut` to its new state, do this even if we are doing the constant
                // optimization
                tnode.lut = Some(lut);
                true
            } else {
                tnode.lut = Some(lut);
                // TODO check for identity `if lut.bw() == 2` it must be identity or invert
                false
            }
        } else if tnode.inp.len() == 1 {
            // wire propogation
            let input_equiv = t_dag.backrefs.get_val_mut(tnode.inp[0]).unwrap();
            if let Value::Const(val) = input_equiv.val {
                let equiv = t_dag.backrefs.get_val_mut(tnode.p_self).unwrap();
                equiv.val = Value::Const(val);
                let _ = self
                    .optimizations
                    .insert(Optimization::ConstifyEquiv(equiv.p_self_equiv), ());
                true
            } else {
                false
            }
        } else {
            // TODO loopbacks
            false
        }
    }

    /// If there exists any equivalence with no checks applied, this should
    /// always be applied before any further optimizations are applied, so that
    /// `RemoveUnused` and `ConstPropogate` can be handled before any other
    /// optimization
    pub fn preinvestigate_equiv(&mut self, t_dag: &mut TDag, p_equiv: PBack) {
        let mut non_self_rc = 0usize;
        let equiv = t_dag.backrefs.get_val(p_equiv).unwrap();
        let mut is_const = matches!(equiv.val, Value::Const(_));
        let mut adv = t_dag.backrefs.advancer_surject(p_equiv);
        while let Some(p_back) = adv.advance(&t_dag.backrefs) {
            let referent = *t_dag.backrefs.get_key(p_back).unwrap();
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisTNode(p_tnode) => {
                    // avoid checking more if it was already determined to be constant
                    if !is_const && self.const_eval_tnode(t_dag, p_tnode) {
                        is_const = true;
                    }
                }
                Referent::Input(_) => non_self_rc += 1,
                Referent::LoopDriver(p_driver) => {
                    // the way `LoopDriver` networks with no real dependencies will work, is
                    // that const propogation and other simplifications will eventually result
                    // in a single node equivalence that drives itself, which we can remove
                    let p_back_driver = t_dag.tnodes.get(p_driver).unwrap().p_self;
                    if !t_dag.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
                        non_self_rc += 1;
                    }

                    // TODO check for const through loop, but there should be a
                    // parameter to enable
                }
                Referent::Note(_) => non_self_rc += 1,
            }
        }
        if non_self_rc == 0 {
            let _ = self
                .optimizations
                .insert(Optimization::RemoveEquiv(p_equiv), ());
        } else if is_const {
            let _ = self
                .optimizations
                .insert(Optimization::ConstifyEquiv(p_equiv), ());
        } else {
            let _ = self
                .optimizations
                .insert(Optimization::InvestigateEquiv0(p_equiv), ());
        }
    }

    /// Does not perform hte final step
    /// `t_dag.backrefs.remove(tnode.p_self).unwrap()` which is important for
    /// `Advancer`s.
    pub fn remove_tnode_not_p_self(&mut self, t_dag: &mut TDag, p_tnode: PTNode) {
        let tnode = t_dag.tnodes.remove(p_tnode).unwrap();
        if let Some(p_driver) = tnode.loop_driver {
            let p_equiv = t_dag.backrefs.get_val(p_driver).unwrap().p_self_equiv;
            let _ = self
                .optimizations
                .insert(Optimization::InvestigateUsed(p_equiv), ());
            t_dag.backrefs.remove_key(p_driver).unwrap();
        }
        for inp in tnode.inp {
            let p_equiv = t_dag.backrefs.get_val(inp).unwrap().p_self_equiv;
            let _ = self
                .optimizations
                .insert(Optimization::InvestigateUsed(p_equiv), ());
            t_dag.backrefs.remove_key(inp).unwrap();
        }
    }

    pub fn optimize(&mut self, t_dag: &mut TDag) {
        // need to preinvestigate everything before starting a priority loop
        let mut adv = t_dag.backrefs.advancer();
        while let Some(p_back) = adv.advance(&t_dag.backrefs) {
            if let Referent::ThisEquiv = t_dag.backrefs.get_key(p_back).unwrap() {
                self.preinvestigate_equiv(t_dag, p_back);
            }
        }
        while let Some(p_optimization) = self.optimizations.min() {
            optimize(self, t_dag, p_optimization)
        }
    }
}

fn optimize(opt: &mut Optimizer, t_dag: &mut TDag, p_optimization: POpt) {
    let optimization = opt.optimizations.remove(p_optimization).unwrap().0;
    match optimization {
        Optimization::RemoveEquiv(p_back) => {
            let p_equiv = if let Some(equiv) = t_dag.backrefs.get_val(p_back) {
                equiv.p_self_equiv
            } else {
                return
            };
            // remove all associated TNodes first
            let mut adv = t_dag.backrefs.advancer_surject(p_back);
            while let Some(p_back) = adv.advance(&t_dag.backrefs) {
                match t_dag.backrefs.get_key(p_back).unwrap() {
                    Referent::ThisEquiv => (),
                    Referent::ThisTNode(p_tnode) => {
                        opt.remove_tnode_not_p_self(t_dag, *p_tnode);
                    }
                    // TODO check self reference case
                    Referent::LoopDriver(_) => (),
                    _ => unreachable!(),
                }
            }
            // remove the equivalence
            t_dag.backrefs.remove(p_equiv).unwrap();
        }
        Optimization::ForwardEquiv(p_back) => {
            todo!()
        }
        Optimization::ConstifyEquiv(p_back) => {
            if !t_dag.backrefs.contains(p_back) {
                return
            };
            // for removing `ThisTNode` safely
            let mut remove = SmallVec::<[PBack; 16]>::new();
            // remove all associated TNodes
            let mut adv = t_dag.backrefs.advancer_surject(p_back);
            while let Some(p_back) = adv.advance(&t_dag.backrefs) {
                match t_dag.backrefs.get_key(p_back).unwrap() {
                    Referent::ThisEquiv => (),
                    Referent::ThisTNode(p_tnode) => {
                        opt.remove_tnode_not_p_self(t_dag, *p_tnode);
                        remove.push(p_back);
                    }
                    Referent::Input(p_inp) => {
                        let _ = opt
                            .optimizations
                            .insert(Optimization::InvestigateConst(*p_inp), ());
                    }
                    Referent::LoopDriver(p_driver) => {
                        let _ = opt
                            .optimizations
                            .insert(Optimization::InvestigateConst(*p_driver), ());
                    }
                    Referent::Note(_) => (),
                }
            }
            for p_back in remove {
                t_dag.backrefs.remove_key(p_back).unwrap();
            }
        }
        Optimization::RemoveTNode(p_back) => {
            if !t_dag.backrefs.contains(p_back) {
                return
            }
            todo!()
        }
        Optimization::InvestigateUsed(p_back) => {
            if !t_dag.backrefs.contains(p_back) {
                return
            };
            let mut found_use = false;
            let mut adv = t_dag.backrefs.advancer_surject(p_back);
            while let Some(p_back) = adv.advance(&t_dag.backrefs) {
                let referent = *t_dag.backrefs.get_key(p_back).unwrap();
                match referent {
                    Referent::ThisEquiv => (),
                    Referent::ThisTNode(_) => (),
                    Referent::Input(_) => {
                        found_use = true;
                        break
                    }
                    Referent::LoopDriver(p_driver) => {
                        let p_back_driver = t_dag.tnodes.get(p_driver).unwrap().p_self;
                        if !t_dag.backrefs.in_same_set(p_back, p_back_driver).unwrap() {
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
                let _ = opt
                    .optimizations
                    .insert(Optimization::RemoveEquiv(p_back), ());
            }
        }
        Optimization::InvestigateConst(p_tnode) => {
            if !t_dag.tnodes.contains(p_tnode) {
                return
            };
            if opt.const_eval_tnode(t_dag, p_tnode) {
                let _ = opt.optimizations.insert(
                    Optimization::ConstifyEquiv(t_dag.tnodes.get(p_tnode).unwrap().p_self),
                    (),
                );
            }
        }
        Optimization::InvestigateEquiv0(_) => (),
    }
}
