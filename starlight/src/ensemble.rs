mod correspond;
#[cfg(feature = "debug")]
mod debug;
mod lnode;
mod optimize;
#[cfg(feature = "debug")]
pub mod render;
mod rnode;
mod state;
mod tnode;
mod together;
mod value;

#[allow(unused)]
use std::num::NonZeroU32;

use awint::awint_dag::triple_arena::ptr_struct;
pub use correspond::Corresponder;
pub use lnode::{LNode, LNodeKind};
pub use optimize::Optimizer;
pub use rnode::{Notary, PExternal, RNode};
pub use state::{State, Stator};
pub use tnode::{Delay, Delayer, TNode};
pub use together::{Ensemble, Equiv, Referent};
pub use value::{
    BasicValue, BasicValueKind, ChangeKind, CommonValue, DynamicValue, EvalPhase, Evaluator, Event,
    Value,
};

#[cfg(any(
    debug_assertions,
    all(feature = "gen_counters", not(feature = "u32_ptrs")),
))]
ptr_struct!(PBack; PLNode; PTNode; PRNode);

#[cfg(all(
    not(debug_assertions),
    not(feature = "gen_counters"),
    not(feature = "u32_ptrs"),
))]
ptr_struct!(PBack(); PLNode(); PTNode(); PRNode());

#[cfg(all(not(debug_assertions), feature = "gen_counters", feature = "u32_ptrs",))]
ptr_struct!(
    PBack[NonZeroU32](NonZeroU32);
    PLNode[NonZeroU32](NonZeroU32);
    PTNode[NonZeroU32](NonZeroU32);
    PRNode[NonZeroU32](NonZeroU32)
);

#[cfg(all(
    not(debug_assertions),
    not(feature = "gen_counters"),
    feature = "u32_ptrs",
))]
ptr_struct!(PBack[NonZeroU32](); PLNode[NonZeroU32](); PTNode[NonZeroU32](); PRNode[NonZeroU32]());

// these are completely internal and so can always go without gen counters
#[cfg(any(debug_assertions, not(feature = "u32_ptrs")))]
ptr_struct!(PSimEvent(); POpt(); PMeta(); PCorrespond());

#[cfg(all(not(debug_assertions), feature = "u32_ptrs"))]
ptr_struct!(PSimEvent[NonZeroU32](); POpt[NonZeroU32](); PMeta[NonZeroU32](); PCorrespond[NonZeroU32]());
