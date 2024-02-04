pub mod epoch;
mod eval_awi;
mod lazy_awi;
mod temporal;

pub use epoch::{Assertions, Epoch, SuspendedEpoch};
pub use eval_awi::EvalAwi;
pub use lazy_awi::LazyAwi;
pub use temporal::{delay, Loop, Net};

// TODO `In<BW>`, `Out<W>`, `InOut<W>` (?) shorthand wrappers
