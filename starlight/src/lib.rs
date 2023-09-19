#[cfg(feature = "debug")]
mod debug;
mod lower;
//mod simplify;
mod rng;
mod t_dag;
mod temporal;
mod tnode;
#[cfg(feature = "debug")]
pub use awint::awint_dag::triple_arena_render;
pub use awint::{self, awint_dag, awint_dag::triple_arena};
pub use t_dag::*;
pub use temporal::*;
pub use tnode::*;
mod optimize;
pub use optimize::*;
pub use rng::StarRng;

// TODO need something like an `AutoAwi` type that seamlessly interfaces with
// internally or externally running DAGs / regular Awi functions / operational
// mimick functions? Make evaluation lazy so things are not simulated until
// `AutoAwi`s are read, track write status and possible update DAGs
//
// Can RefCells and mutation be used in `AsRef`?

/// Reexports all the regular arbitrary width integer structs, macros, common
/// enums, and most of `core::primitive::*`. This is useful for glob importing
/// everything or for when using the regular items in a context with structs
/// imported from `awint_dag`.
pub mod awi {
    pub use awint::awi::*;
    pub use Option::{None, Some};
    pub use Result::{Err, Ok};
}

/// Reexports all the mimicking versions of `awi` items
pub mod dag {
    pub use awint::dag::{
        Option::{None, Some},
        Result::{Err, Ok},
        *,
    };

    pub use crate::{Loop, LoopHandle, Net};
}

// TODO use modified Lagrangians that appear different to nets with different
// requirements on critical path, plus small differencing values to prevent
// alternating constraint problems
