/// An epoch management struct used for tests and examples.
use std::{cell::RefCell, mem, num::NonZeroUsize, thread::panicking};

use awint::{
    awint_dag::{
        epoch::{EpochCallback, EpochKey},
        Lineage, Location, Op, PState,
    },
    bw, dag,
};

use crate::TDag;

#[derive(Debug, Clone)]
pub struct Assertions {
    pub bits: Vec<PState>,
}

impl Assertions {
    pub fn new() -> Self {
        Self { bits: vec![] }
    }
}

impl Default for Assertions {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct EpochData {
    key: EpochKey,
    assertions: Assertions,
    /// All states associated with this epoch
    states: Vec<PState>,
}

struct TopEpochData {
    tdag: TDag,
    /// The top level `EpochData`
    data: EpochData,
    /// If the top level is active
    active: bool,
}

impl TopEpochData {
    pub fn new() -> Self {
        Self {
            tdag: TDag::new(),
            data: EpochData::default(),
            active: false,
        }
    }
}

thread_local!(
    /// The `TopEpochData`. We have this separate from `EPOCH_DATA_STACK` in the
    /// first place to minimize the assembly needed to access the data.
    static EPOCH_DATA_TOP: RefCell<TopEpochData> = RefCell::new(TopEpochData::new());

    /// Stores data for epochs lower than the current one
    static EPOCH_DATA_STACK: RefCell<Vec<EpochData>> = RefCell::new(vec![]);
);

#[doc(hidden)]
pub fn _callback() -> EpochCallback {
    fn new_pstate(nzbw: NonZeroUsize, op: Op<PState>, location: Option<Location>) -> PState {
        EPOCH_DATA_TOP.with(|top| {
            let mut top = top.borrow_mut();
            let p_state = top.tdag.make_state(nzbw, op, location);
            top.data.states.push(p_state);
            p_state
        })
    }
    fn register_assertion_bit(bit: dag::bool, location: Location) {
        // need a new bit to attach location data to
        let new_bit = new_pstate(bw(1), Op::Copy([bit.state()]), Some(location));
        EPOCH_DATA_TOP.with(|top| {
            let mut top = top.borrow_mut();
            top.data.assertions.bits.push(new_bit);
        })
    }
    fn get_nzbw(p_state: PState) -> NonZeroUsize {
        EPOCH_DATA_TOP.with(|top| {
            let top = top.borrow();
            top.tdag.states.get(p_state).unwrap().nzbw
        })
    }
    fn get_op(p_state: PState) -> Op<PState> {
        EPOCH_DATA_TOP.with(|top| {
            let top = top.borrow();
            top.tdag.states.get(p_state).unwrap().op.clone()
        })
    }
    EpochCallback {
        new_pstate,
        register_assertion_bit,
        get_nzbw,
        get_op,
    }
}

#[derive(Debug)]
pub struct Epoch {
    key: EpochKey,
}

impl Drop for Epoch {
    fn drop(&mut self) {
        // prevent invoking recursive panics and a buffer overrun
        if !panicking() {
            // unregister callback
            self.key.pop_off_epoch_stack();
            EPOCH_DATA_TOP.with(|top| {
                let mut top = top.borrow_mut();
                // remove all the states associated with this epoch
                for _p_state in top.data.states.iter() {
                    // TODO
                    //top.tdag.states.remove(*p_state).unwrap();
                }
                top.tdag = TDag::new();
                // move the top of the stack to the new top
                let new_top = EPOCH_DATA_STACK.with(|stack| {
                    let mut stack = stack.borrow_mut();
                    stack.pop()
                });
                if let Some(new_data) = new_top {
                    top.data = new_data;
                } else {
                    top.active = false;
                    top.data = EpochData::default();
                    // TODO capacity clearing?
                }
            });
        }
    }
}

impl Epoch {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let key = _callback().push_on_epoch_stack();
        EPOCH_DATA_TOP.with(|top| {
            let mut top = top.borrow_mut();
            if top.active {
                // move old top to the stack
                EPOCH_DATA_STACK.with(|stack| {
                    let mut stack = stack.borrow_mut();
                    let new_top = EpochData {
                        key,
                        ..Default::default()
                    };
                    let old_top = mem::replace(&mut top.data, new_top);
                    stack.push(old_top);
                })
            } else {
                top.active = true;
                top.data.key = key;
                // do not have to do anything else, defaults are set at the
                // beginning and during dropping
            }
        });
        Self { key }
    }

    /// Gets the assertions associated with this Epoch (not including assertions
    /// from when sub-epochs are alive or from before the this Epoch was
    /// created)
    pub fn assertions(&self) -> Assertions {
        let mut res = Assertions::new();
        let mut found = false;
        EPOCH_DATA_TOP.with(|top| {
            let top = top.borrow();
            if top.data.key == self.key {
                res = top.data.assertions.clone();
                found = true;
            }
        });
        if !found {
            EPOCH_DATA_STACK.with(|stack| {
                let stack = stack.borrow();
                for (i, layer) in stack.iter().enumerate().rev() {
                    if layer.key == self.key {
                        res = layer.assertions.clone();
                        break
                    }
                    if i == 0 {
                        // shouldn't be reachable even with leaks
                        unreachable!();
                    }
                }
            });
        }
        res
    }
}