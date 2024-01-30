use std::{
    cmp::{Ordering, Reverse},
    collections::BinaryHeap,
    num::{NonZeroU64, NonZeroUsize},
};

use awint::{awi::*, awint_dag::triple_arena::Advancer};

use crate::{
    ensemble::{Ensemble, PBack, PLNode, PTNode, Referent},
    Error,
};

#[derive(Debug, Clone, Copy)]
pub enum BasicValueKind {
    Opaque,
    Zero,
    Umax,
    Imax,
    Imin,
    Uone,
}

/// Used when we need to pass an argument that can multiplex over the basic
/// initial values
#[derive(Debug, Clone, Copy)]
pub struct BasicValue {
    pub kind: BasicValueKind,
    pub nzbw: NonZeroUsize,
}

impl BasicValue {
    pub fn nzbw(&self) -> NonZeroUsize {
        self.nzbw
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn get(&self, inx: usize) -> Option<Option<bool>> {
        if inx >= self.bw() {
            None
        } else {
            Some(match self.kind {
                BasicValueKind::Opaque => None,
                BasicValueKind::Zero => Some(false),
                BasicValueKind::Umax => Some(true),
                BasicValueKind::Imax => Some(inx != (self.bw() - 1)),
                BasicValueKind::Imin => Some(inx == (self.bw() - 1)),
                BasicValueKind::Uone => Some(inx == 0),
            })
        }
    }
}

/// Used when we need to pass an argument that can multiplex over common initial
/// values
#[derive(Debug, Clone)]
pub enum CommonValue<'a> {
    Bits(&'a Bits),
    Basic(BasicValue),
}

impl<'a> CommonValue<'a> {
    pub fn nzbw(&self) -> NonZeroUsize {
        match self {
            CommonValue::Bits(x) => x.nzbw(),
            CommonValue::Basic(basic) => basic.nzbw(),
        }
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    pub fn get(&self, inx: usize) -> Option<Option<bool>> {
        match self {
            CommonValue::Bits(bits) => bits.get(inx).map(Some),
            CommonValue::Basic(basic) => basic.get(inx),
        }
    }
}

/// The value of a multistate boolean
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Value {
    /// The value is permanently unknown
    ConstUnknown,
    /// The value is simply unknown, or a circuit is undriven
    Unknown,
    /// The value is a known constant that is guaranteed to not change under any
    /// condition
    Const(bool),
    /// The value is known, but may be dynamically changed
    Dynam(bool),
}

impl Value {
    pub fn known_value(self) -> Option<bool> {
        match self {
            Value::ConstUnknown => None,
            Value::Unknown => None,
            Value::Const(b) => Some(b),
            Value::Dynam(b) => Some(b),
        }
    }

    pub fn is_known(self) -> bool {
        match self {
            Value::ConstUnknown | Value::Unknown => false,
            Value::Const(_) | Value::Dynam(_) => true,
        }
    }

    pub fn is_const(self) -> bool {
        match self {
            Value::Unknown | Value::Dynam(_) => false,
            Value::ConstUnknown | Value::Const(_) => true,
        }
    }

    pub fn constified(self) -> Self {
        match self {
            Value::ConstUnknown => self,
            Value::Unknown => Value::ConstUnknown,
            Value::Const(_) => self,
            Value::Dynam(b) => Value::Const(b),
        }
    }
}

/// Used for dealing with mixed values and dynamics
#[derive(Debug, Clone, Copy)]
pub enum DynamicValue {
    /// Corresponds with `Value::Unknown`
    ConstUnknown,
    /// Corresponds with `Value::Const`
    Const(bool),
    Dynam(PBack),
}

// Here are some of the reasons why we have chosen this somewhat convoluted
// evaluator strategy. We want to prevent a situation where we receive a command
// to change an equivalence value, then propogate changes as far as they will go
// down potentially most of the DAG, then do that whole cascade for every change
// made. Changes made to equivalences can stay in place until the point where a
// request for a downstream value is made. A secondary goal is to avoid
// unneccessary calculations from change propogations if they don't actually
// lead to a request.

// What we most want to avoid is globally requesting `TNode` drivers when most
// of them are not actually being changed. We color regions connected by
// `LNode`s, and have a surject arena over equivalences, and the `Delayer` does
// its event processing on that level.

// Within each region, we could use the cascade front strategy that starts from
// the roots or from the immediate precalculated descendants of the roots,
// progressing when counters associated with the inputs of each `LNode` reach
// zero. However, if the region source tree is large and only one small part has
// been changed, there is a lot of wasted computation. Instead of the front
// strategy or an intermediate change-request strategy that had issues of still
// needing to request the whole thing, we have a modified event propogation
// strategy that avoids the overwriting waste problem. Now that we have the
// extra surjection level with known DAGs, what we do is assign partially
// ordered integers over the DAG, such that an equivalence's number must be
// greater than the maximum number of any of its dependencies. The event
// propogation is calculated in order from least to greatest numbered. So, an
// equivalence will not be calculated until its dependencies are.

// However, to allow any changes to the equivalence graph we need more referents
// and a lot of development time. The current strategy is an approximation of
// the above, where we calculate the partial ordering on the fly. After the
// first few cascades, the existing partial orderings will help prevent both DAG
// region overwrites and more complicated
// multi-DAG-connected-by-zero-delay-length overwrites that the above strategy
// would require even more to handle.

// I have torn out an old change/request bifurcation strategy because it is
// probably practically useless for e.g. Island FPGA simulation which strongly
// favors change only event cascading, but for other purposes I envision we may
// want to revisit it

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvalPhase {
    Change,
    Request,
}

#[derive(Debug, Clone, Copy)]
pub enum ChangeKind {
    LNode(PLNode),
    TNode(PTNode),
}

/// Note that the `Eq`, `Ord`, etc traits are implemented to only order on
/// `partial_ord_num`
#[derive(Debug, Clone, Copy)]
pub struct Event {
    pub partial_ord_num: NonZeroU64,
    pub change_kind: ChangeKind,
}

impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.partial_ord_num == other.partial_ord_num
    }
}

impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.partial_ord_num.cmp(&other.partial_ord_num))
    }
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_ord_num.cmp(&other.partial_ord_num)
    }
}

#[derive(Debug, Clone)]
pub struct Evaluator {
    phase: EvalPhase,
    /// Events that can accumulate during `Change` phase, but must all be
    /// processed before `Request` phase can start
    events: BinaryHeap<Reverse<Event>>,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            phase: EvalPhase::Change,
            events: BinaryHeap::new(),
        }
    }

    /// Checks that there are no remaining events, then shrinks allocations
    pub fn check_clear(&mut self) -> Result<(), Error> {
        if !self.events.is_empty() {
            return Err(Error::OtherStr("events need to be empty"));
        }
        self.events.clear();
        self.events.shrink_to_fit();
        Ok(())
    }

    pub fn are_events_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn push_event(&mut self, event: Event) {
        self.events.push(Reverse(event))
    }

    pub fn pop_event(&mut self) -> Option<Event> {
        self.events.pop().map(|e| e.0)
    }
}

impl Ensemble {
    /// Switches to change phase if not already in that phase
    pub fn switch_to_change_phase(&mut self) {
        if self.evaluator.phase != EvalPhase::Change {
            self.evaluator.phase = EvalPhase::Change;
        }
    }

    /// `switch_to_request_phase` will do nothing if the phase is already
    /// `Request`, this will always run the event clearing
    pub fn restart_request_phase(&mut self) -> Result<(), Error> {
        // TODO think more about this, handle redundant change cases
        let mut event_gas = self.backrefs.len_keys();
        while let Some(event) = self.evaluator.pop_event() {
            let res = self.handle_event(event);
            if res.is_err() {
                // need to reinsert
                self.evaluator.push_event(event)
            }
            res?;
            if let Some(x) = event_gas.checked_sub(1) {
                event_gas = x;
            } else {
                return Err(Error::OtherStr("ran out of event gas"));
            }
        }

        // handle_event will keep in change phase, only afterwards do we switch
        self.evaluator.phase = EvalPhase::Request;
        Ok(())
    }

    /// Switches to request phase if not already in that phase, clears events
    pub fn switch_to_request_phase(&mut self) -> Result<(), Error> {
        if self.evaluator.phase != EvalPhase::Request {
            self.restart_request_phase()
        } else {
            Ok(())
        }
    }

    /// If the new `value` would actually change the existing value at `p_back`,
    /// this will change it and push new events for dependent equivalences.
    ///
    /// # Errors
    ///
    /// If an error is returned, no events have been created and any events that
    /// caused `change_value` need to be reinserted
    pub fn change_value(
        &mut self,
        p_back: PBack,
        value: Value,
        source_partial_ord_num: NonZeroU64,
    ) -> Result<(), Error> {
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if equiv.val == value {
                // no change needed
                return Ok(())
            }
            if equiv.val.is_const() && (equiv.val != value) {
                return Err(Error::OtherStr(
                    "tried to change a constant (probably, `retro_const_*` was used followed by a \
                     contradicting `retro_*`, or some invariant was broken)",
                ))
            }
            equiv.val = value;
            if equiv.evaluator_partial_order <= source_partial_ord_num {
                equiv.evaluator_partial_order = source_partial_ord_num.checked_add(1).unwrap();
            }
            let equiv_partial_ord_num = equiv.evaluator_partial_order;
            // switch to change phase if not already
            self.switch_to_change_phase();

            // create any needed events
            let mut adv = self.backrefs.advancer_surject(p_back);
            while let Some(p_back) = adv.advance(&self.backrefs) {
                let referent = *self.backrefs.get_key(p_back).unwrap();
                match referent {
                    Referent::ThisEquiv
                    | Referent::ThisLNode(_)
                    | Referent::ThisTNode(_)
                    | Referent::ThisStateBit(..) => (),
                    Referent::Input(p_lnode) => {
                        self.evaluator.push_event(Event {
                            partial_ord_num: equiv_partial_ord_num,
                            change_kind: ChangeKind::LNode(p_lnode),
                        });
                    }
                    Referent::Driver(p_tnode) => {
                        self.evaluator.push_event(Event {
                            partial_ord_num: equiv_partial_ord_num,
                            change_kind: ChangeKind::TNode(p_tnode),
                        });
                    }
                    Referent::ThisRNode(_) => (),
                }
            }
            Ok(())
        } else {
            Err(Error::InvalidPtr)
        }
    }

    /// Note that if an error is returned, the event needs to be reinserted.
    fn handle_event(&mut self, event: Event) -> Result<(), Error> {
        match event.change_kind {
            ChangeKind::LNode(p_lnode) => self.eval_lnode(p_lnode),
            ChangeKind::TNode(p_tnode) => self.eval_tnode(p_tnode),
        }
    }

    /// Evaluates the `LNode` and pushes new events as needed. Note that any
    /// events that cause this need to be reinserted if this returns an error.
    pub fn eval_lnode(&mut self, p_lnode: PLNode) -> Result<(), Error> {
        let p_back = self.lnodes.get(p_lnode).unwrap().p_self;
        let (val, partial_ord_num) = self.calculate_lnode_value(p_lnode)?;
        self.change_value(p_back, val, partial_ord_num)
    }

    /// Evaluates the `TNode` and pushes new events or delayed events as needed.
    /// Note that any events that cause this need to be reinserted if this
    /// returns an error.
    pub fn eval_tnode(&mut self, p_tnode: PTNode) -> Result<(), Error> {
        let tnode = self.tnodes.get(p_tnode).unwrap();
        if tnode.delay().is_zero() {
            let p_driver = tnode.p_driver;
            let equiv = self.backrefs.get_val(p_driver).unwrap();
            let partial_ord_num = equiv.evaluator_partial_order;
            self.change_value(tnode.p_self, equiv.val, partial_ord_num)
        } else {
            self.delayer
                .insert_delayed_tnode_event(p_tnode, tnode.delay());
            Ok(())
        }
    }

    pub fn request_value(&mut self, p_back: PBack) -> Result<Value, Error> {
        if let Some(equiv) = self.backrefs.get_val_mut(p_back) {
            if equiv.val.is_const() {
                return Ok(equiv.val)
            }
            self.switch_to_request_phase()?;
            Ok(self.backrefs.get_val(p_back).unwrap().val)
        } else {
            Err(Error::InvalidPtr)
        }
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}
