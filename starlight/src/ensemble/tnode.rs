use awint::awint_dag::triple_arena::{ptr_struct, Advancer, Recast, Recaster};

use crate::{
    ensemble::{Ensemble, PBack, Referent, Value},
    Error,
};

// We use this because our algorithms depend on generation counters
ptr_struct!(PTNode);

#[derive(Debug, Clone)]
pub struct Delay {
    pub amount: u128,
}

impl Delay {
    pub fn zero() -> Self {
        Self { amount: 0 }
    }

    pub fn from_amount(amount: u128) -> Self {
        Self { amount }
    }

    pub fn is_zero(&self) -> bool {
        self.amount == 0
    }

    pub fn amount(&self) -> u128 {
        self.amount
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

    pub fn delay(&self) -> &Delay {
        &self.delay
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

#[derive(Debug, Clone)]
pub struct Delayer {}

impl Ensemble {
    /// Sets up a `TNode` source driven by a driver
    #[must_use]
    pub fn make_tnode(
        &mut self,
        p_source: PBack,
        p_driver: PBack,
        init_val: Option<Value>,
        delay: Delay,
    ) -> Option<PTNode> {
        let partial_ord_num = self
            .backrefs
            .get_val(p_driver)
            .unwrap()
            .evaluator_partial_order;
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
        if let Some(init_val) = init_val {
            // in order for the value to register correctly
            self.change_value(p_source, init_val, partial_ord_num.checked_add(1).unwrap())
                .unwrap();
        }
        Some(p_tnode)
    }

    pub fn run(&mut self, time: Delay) -> Result<(), Error> {
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            let p_driver = tnode.p_driver;
            self.request_value(p_driver)?;
        }
        // second do all loopback changes
        // TODO
        /*let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            let val = self.backrefs.get_val(tnode.p_driver).unwrap().val;
            let p_self = tnode.p_self;
            self.change_value(p_self, val).unwrap();
        }*/
        Ok(())
    }
}
