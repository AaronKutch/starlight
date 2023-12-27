use std::num::{NonZeroU128, NonZeroUsize};

use awint::awint_dag::{
    triple_arena::{ptr_struct, Arena},
    EvalError, PState,
};

use crate::{
    awi,
    ensemble::{Ensemble, PBack, Referent, Value},
    epoch::get_current_epoch,
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
    pub bits: Vec<Option<PBack>>,
}

impl RNode {
    pub fn new() -> Self {
        Self { bits: vec![] }
    }
}

/// Used for managing external references
#[derive(Debug, Clone)]
pub struct Notary {
    pub rnodes: Arena<PRNode, RNode>,
}

impl Notary {
    pub fn new() -> Self {
        Self {
            rnodes: Arena::new(),
        }
    }
}

impl Ensemble {
    #[must_use]
    pub fn make_rnode_for_pstate(&mut self, p_state: PState) -> Option<PRNode> {
        self.initialize_state_bits_if_needed(p_state)?;
        let p_rnode = self.notary.rnodes.insert(RNode::new());
        let len = self.stator.states[p_state].p_self_bits.len();
        for i in 0..len {
            let p_bit = self.stator.states[p_state].p_self_bits[i];
            if let Some(p_bit) = p_bit {
                let p_equiv = self.backrefs.get_val(p_bit)?.p_self_equiv;
                let p_back_new = self
                    .backrefs
                    .insert_key(p_equiv, Referent::ThisRNode(p_rnode))
                    .unwrap();
                self.notary.rnodes[p_rnode].bits.push(Some(p_back_new));
            } else {
                self.notary.rnodes[p_rnode].bits.push(None);
            }
        }
        Some(p_rnode)
    }

    pub fn remove_rnode(&mut self, p_rnode: PRNode) -> Result<(), EvalError> {
        if let Some(rnode) = self.notary.rnodes.remove(p_rnode) {
            for p_back in rnode.bits {
                if let Some(p_back) = p_back {
                    let referent = self.backrefs.remove_key(p_back).unwrap().0;
                    assert!(matches!(referent, Referent::ThisRNode(_)));
                }
            }
            Ok(())
        } else {
            Err(EvalError::InvalidPtr)
        }
    }

    pub fn get_thread_local_rnode_nzbw(p_rnode: PRNode) -> Result<NonZeroUsize, EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some(rnode) = ensemble.notary.rnodes.get(p_rnode) {
            Ok(NonZeroUsize::new(rnode.bits.len()).unwrap())
        } else {
            Err(EvalError::OtherStr(
                "could not find thread local `RNode`, probably an `EvalAwi` or `LazyAwi` was used \
                 outside of the `Epoch` it was created in",
            ))
        }
    }

    pub fn change_thread_local_rnode_value(
        p_rnode: PRNode,
        bits: &awi::Bits,
    ) -> Result<(), EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some(rnode) = ensemble.notary.rnodes.get(p_rnode) {
            if rnode.bits.len() != bits.bw() {
                return Err(EvalError::WrongBitwidth);
            }
        } else {
            return Err(EvalError::OtherStr(
                "could not find thread local `RNode`, probably a `LazyAwi` was used outside of \
                 the `Epoch` it was created in",
            ))
        }
        for bit_i in 0..bits.bw() {
            let p_back = ensemble.notary.rnodes[p_rnode].bits[bit_i];
            if let Some(p_back) = p_back {
                ensemble
                    .change_value(p_back, Value::Dynam(bits.get(bit_i).unwrap()))
                    .unwrap();
            }
        }
        Ok(())
    }

    pub fn calculate_thread_local_rnode_value(
        p_rnode: PRNode,
        bit_i: usize,
    ) -> Result<Value, EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        let p_back = if let Some(rnode) = ensemble.notary.rnodes.get(p_rnode) {
            if bit_i >= rnode.bits.len() {
                return Err(EvalError::OtherStr(
                    "something went wrong with rnode bitwidth",
                ));
            }
            if let Some(p_back) = rnode.bits[bit_i] {
                p_back
            } else {
                return Err(EvalError::OtherStr(
                    "something went wrong, found `RNode` for evaluator but a bit was pruned",
                ))
            }
        } else {
            return Err(EvalError::OtherStr(
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

impl Default for RNode {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for Notary {
    fn default() -> Self {
        Self::new()
    }
}
