mod dag;
mod lower;
mod tnode;
pub use dag::*;
pub use tnode::*;
//#[cfg(feature = "debug")]
//mod debug;
//mod contract;
use triple_arena::ptr_struct;

ptr_struct!(PNote);
