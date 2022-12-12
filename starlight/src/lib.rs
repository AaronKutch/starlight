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

// TODO need mimicking `Option` and a thread local panic handler equivalent
// assertion bit

// TODO need the `?` helper macro

// TODO need something like an `AutoAwi` type that seamlessly interfaces with
// internally or externally running DAGs / regular Awi functions / operational
// mimick functions? Make evaluation lazy so things are not simulated until
// `AutoAwi`s are read, track write status and possible update DAGs

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
