mod dag;
#[cfg(feature = "debug")]
mod debug;
mod lower;
mod simplify;
mod tnode;
mod toroidal;
#[cfg(feature = "debug")]
pub use awint::awint_dag::triple_arena_render;
pub use awint::{self, awint_dag, awint_dag::triple_arena};
pub use dag::*;
pub use tnode::*;
pub use toroidal::*;
use triple_arena::ptr_struct;

ptr_struct!(PNote; PTNode);

// TODO use modified Lagrangians that appear different to nets with different
// requirements on critical path, plus small differencing values to prevent
// alternating constraint problems
