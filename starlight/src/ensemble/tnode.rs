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
            self.change_value(p_source, init_val).unwrap();
        }
        Some(p_tnode)
    }

    pub fn run(&mut self, time: Delay) -> Result<(), Error> {
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            let p_driver = tnode.p_driver;
            self.calculate_value(p_driver)?;
        }
        // second do all loopback changes
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            let val = self.backrefs.get_val(tnode.p_driver).unwrap().val;
            let p_self = tnode.p_self;
            self.change_value(p_self, val).unwrap();
        }
        Ok(())
    }
}
