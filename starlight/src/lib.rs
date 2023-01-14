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

// TODO need the `?` helper macro

// TODO need something like an `AutoAwi` type that seamlessly interfaces with
// internally or externally running DAGs / regular Awi functions / operational
// mimick functions? Make evaluation lazy so things are not simulated until
// `AutoAwi`s are read, track write status and possible update DAGs
//
// Can RefCells and mutation be used in `AsRef`?

// TODO "regular" loop versions for completeness

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

ptr_struct!(PTNode);

// TODO use modified Lagrangians that appear different to nets with different
// requirements on critical path, plus small differencing values to prevent
// alternating constraint problems
