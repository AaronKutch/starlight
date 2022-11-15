mod dag;
#[cfg(feature = "debug")]
mod debug;
mod lower;
mod simplify;
mod tnode;
mod toroidal;
pub use dag::*;
pub use tnode::*;
pub use toroidal::*;
use triple_arena::ptr_struct;

ptr_struct!(PNote; PTNode);
