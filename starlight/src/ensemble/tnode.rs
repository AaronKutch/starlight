use std::num::NonZeroU64;

use awint::awint_dag::triple_arena::{ptr_struct, OrdArena, Recast, Recaster};

use crate::{
    ensemble::{Ensemble, PBack, Referent},
    Error,
};

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode; PSimEvent);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Delay {
    amount: u128,
}

impl Delay {
    pub fn zero() -> Self {
        Self { amount: 0 }
    }

    pub fn from_amount(amount: u128) -> Self {
        Self { amount }
    }

    pub fn is_zero(self) -> bool {
        self.amount == 0
    }

    pub fn amount(self) -> u128 {
        self.amount
    }

    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        self.amount.checked_add(rhs.amount).map(Delay::from_amount)
    }
}

impl From<u128> for Delay {
    fn from(value: u128) -> Self {
        Self::from_amount(value)
    }
}

/// A temporal node, currently just used for loopbacks
#[derive(Debug, Clone)]
pub struct TNode {
    pub p_self: PBack,
    pub p_driver: PBack,
    pub delay: Delay,
}

impl Recast<PBack> for TNode {
    fn recast<R: Recaster<Item = PBack>>(
        &mut self,
        recaster: &R,
    ) -> Result<(), <R as Recaster>::Item> {
        self.p_self.recast(recaster)?;
        self.p_driver.recast(recaster)
    }
}

impl TNode {
    pub fn new(p_self: PBack, p_driver: PBack, delay: Delay) -> Self {
        Self {
            p_self,
            p_driver,
            delay,
        }
    }

    pub fn delay(&self) -> Delay {
        self.delay
    }
}

// We have separated the `Evaluator` from what we call the `Delayer` which
// manages the actual temporal functionality. The evaluator and `LNode`s are
// only made to handle a DAG ordering, while the delayer and `TNode`s bridge
// different DAGs to each other and themselves. The idea is that we call the
// evaluator to `calculate_value`s of the drivers, and then `change_value`s
// according to scheduling done by the `Delayer`

// The other thing to understand is that there are target descriptions that
// practically need `TNode` based `Net` structures to avoid blowups even if only
// combinational programs are being routed. Consider an island-style FPGA. If we
// want to be able to route any input to any output on the outside edges, and be
// able to do it in a fractal way for subsets of sufficient size, then `Net`s
// are the only way that avoid full trees for every output from every input. The
// setup can produce cycles in general and must be combinational according to
// the configuration in order to be well defined if there are no delays. There
// are many cases where we want no delays. So, we need special handling around
// zero delay edges that cause a region of `LNode`s and `TNode`s to act as
// combinational, and detect when they are not.

// Consider a zero delay `TNode` driving itself through a sequence of two
// inverters, so that the same value should be stored.

#[derive(Debug, Clone)]
pub struct SimultaneousEvents {
    pub tnode_drives: Vec<PTNode>,
}

#[derive(Debug, Clone)]
pub struct Delayer {
    /// Current time as measured by the delay between `Delayer` creation and now
    pub current_time: Delay,
    pub delayed_events: OrdArena<PSimEvent, Delay, SimultaneousEvents>,
}

impl Delayer {
    pub fn new() -> Self {
        Self {
            current_time: Delay::zero(),
            delayed_events: OrdArena::new(),
        }
    }

    pub fn compress(&mut self) {
        self.delayed_events.compress_and_shrink();
    }

    /// Inserts an event that will be delayed by `delay` from the current time
    pub fn insert_delayed_tnode_event(&mut self, p_tnode: PTNode, delay: Delay) {
        let future_time = self.current_time.checked_add(delay).unwrap();
        if let Some((p, order)) = self.delayed_events.find_similar_key(&future_time) {
            if order.is_eq() {
                self.delayed_events
                    .get_val_mut(p)
                    .unwrap()
                    .tnode_drives
                    .push(p_tnode);
            } else {
                let _ = self
                    .delayed_events
                    .insert_linear(p, 2, future_time, SimultaneousEvents {
                        tnode_drives: vec![p_tnode],
                    });
            }
        } else {
            self.delayed_events
                .insert_empty(future_time, SimultaneousEvents {
                    tnode_drives: vec![p_tnode],
                })
                .unwrap();
        }
    }

    pub fn are_delayed_events_empty(&self) -> bool {
        self.delayed_events.is_empty()
    }

    pub fn peek_next_event_time(&self) -> Option<Delay> {
        let p_min = self.delayed_events.min()?;
        self.delayed_events.get_key(p_min).copied()
    }

    pub fn pop_next_simultaneous_events(&mut self) -> Option<(Delay, SimultaneousEvents)> {
        let p_min = self.delayed_events.min()?;
        self.delayed_events.remove(p_min)
    }
}

impl Ensemble {
    /// Sets up a `TNode` source driven by a driver. Driving events need to be
    /// handled by the caller.
    #[must_use]
    pub fn make_tnode(&mut self, p_source: PBack, p_driver: PBack, delay: Delay) -> Option<PTNode> {
        let p_tnode = self.tnodes.insert_with(|p_tnode| {
            let p_driver = self
                .backrefs
                .insert_key(p_driver, Referent::Driver(p_tnode))
                .unwrap();
            let p_self = self
                .backrefs
                .insert_key(p_source, Referent::ThisTNode(p_tnode))
                .unwrap();
            TNode::new(p_self, p_driver, delay)
        });
        Some(p_tnode)
    }

    /// Runs temporal evaluation until `delay` has passed since the current time
    pub fn run(&mut self, delay: Delay) -> Result<(), Error> {
        let final_time = self.delayer.current_time.checked_add(delay).unwrap();
        while let Some(next_time) = self.delayer.peek_next_event_time() {
            if next_time > final_time {
                break
            }
            let (time, events) = self.delayer.pop_next_simultaneous_events().unwrap();
            self.delayer.current_time = time;
            for p_tnode in &events.tnode_drives {
                // this is conditional because some optimizations can remove tnodes
                if let Some(tnode) = self.tnodes.get(*p_tnode) {
                    let p_driver = tnode.p_driver;
                    self.request_value(p_driver)?;
                }
            }
            for p_tnode in &events.tnode_drives {
                if let Some(tnode) = self.tnodes.get(*p_tnode) {
                    let val = self.backrefs.get_val(tnode.p_driver).unwrap().val;
                    let p_self = tnode.p_self;
                    // TODO if we don't unwrap, we need to reregister events
                    self.change_value(p_self, val, NonZeroU64::new(1).unwrap())
                        .unwrap();
                }
            }
        }
        self.delayer.current_time = final_time;
        // this needs to be done in case the last events would lead to infinite loops,
        // it is `restart_request_phase` instead of `switch_to_request_phase` to handle
        // any order of infinite loop detection
        self.restart_request_phase()
    }
}

impl Default for Delayer {
    fn default() -> Self {
        Self::new()
    }
}
