mod dag;
#[cfg(feature = "debug")]
mod debug;
mod lower;
mod simplify;
mod tnode;
pub use dag::*;
pub use tnode::*;
use triple_arena::ptr_struct;

ptr_struct!(PNote; PTNode);
