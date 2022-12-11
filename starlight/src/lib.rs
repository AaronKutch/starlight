#[cfg(feature = "debug")]
mod debug;
mod lower;
mod simplify;
mod t_dag;
mod tnode;
mod toroidal;
#[cfg(feature = "debug")]
pub use awint::awint_dag::triple_arena_render;
pub use awint::{self, awint_dag, awint_dag::triple_arena};
pub use t_dag::*;
pub use tnode::*;
pub use toroidal::*;
use triple_arena::ptr_struct;

// TODO "regular" loop versions for completeness

pub mod prelude {
    pub use awint::prelude::*;
}

pub mod awi {
    pub use awint::awi::*;
}

pub mod dag_prelude {
    pub use awint::dag_prelude::*;

    pub use crate::{Loop, LoopHandle, Net};
}

pub mod dag {
    pub use awint::dag::*;

    pub use crate::{Loop, LoopHandle, Net};
}

ptr_struct!(PNote; PTNode);

// TODO use modified Lagrangians that appear different to nets with different
// requirements on critical path, plus small differencing values to prevent
// alternating constraint problems
