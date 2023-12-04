//! This is a WIP Hardware design language that works as a Rust program.
//! Currently, just combinational logic is well supported. The temporal structs
//! need more development.
//!
//! ```
//! use std::num::NonZeroUsize;
//! use starlight::{awi, dag, lazy_inlawi_ty, Epoch, EvalAwi, LazyInlAwi};
//!
//! // in the scope where this is glob imported, all arbitrary width types, some primitives, and
//! // the mechanisms in the macros will use mimicking types and be lazily evaluated in general.
//! use dag::*;
//!
//! // This is just some arbitrary example I coded up, note that you can use
//! // almost all of Rust's features that you can use on the normal types
//! struct StateMachine {
//!     data: inlawi_ty!(16),
//!     counter: Awi,
//! }
//!
//! impl StateMachine {
//!     pub fn new(w: NonZeroUsize) -> Self {
//!         Self {
//!             data: inlawi!(0u16),
//!             counter: Awi::zero(w),
//!         }
//!     }
//!
//!     pub fn update(&mut self, input: inlawi_ty!(4)) -> Option<()> {
//!         self.counter.inc_(true);
//!
//!         let mut s0 = inlawi!(0u4);
//!         let mut s1 = inlawi!(0u4);
//!         let mut s2 = inlawi!(0u4);
//!         let mut s3 = inlawi!(0u4);
//!         cc!(self.data; s3, s2, s1, s0)?;
//!         s2.xor_(&s0)?;
//!         s3.xor_(&s1)?;
//!         s1.xor_(&s2)?;
//!         s0.xor_(&s3)?;
//!         s3.rotl_(1)?;
//!         s2.mux_(&input, input.get(0)?)?;
//!         cc!(s3, s2, s1, s0; self.data)?;
//!         Some(())
//!     }
//! }
//!
//! // First, create an epoch, this will live until this struct is dropped. The
//! // epoch needs to live until all mimicking operations are done and states are
//! // lowered. Manually drop it with the `drop` function to avoid mistakes.
//! let epoch0 = Epoch::new();
//!
//! let mut m = StateMachine::new(bw(4));
//!
//! // this is initially an opaque value that cannot be eagerly evaluated
//! let input: lazy_inlawi_ty!(4) = LazyInlAwi::opaque();
//! // if we later retroactively assign this to an unequal value, the
//! // `assert_assertions_strict` call will error and show the location of the
//! // assertion that errored
//! dag::assert_eq!(*input, inlawi!(0101));
//!
//! // step the state machine forward
//! m.update(*input).unwrap();
//! m.update(inlawi!(0110)).unwrap();
//! m.update(inlawi!(0110)).unwrap();
//!
//! // use `EvalAwi`s to evaluate the resulting values
//! let output_counter = EvalAwi::from(m.counter);
//! let output_data = EvalAwi::from(m.data);
//!
//! {
//!     // switch back to normal structs
//!     use awi::*;
//!
//!     // lower into purely static bit movements and lookup tables.
//!     epoch0.lower().unwrap();
//!     epoch0.optimize().unwrap();
//!
//!     // Now the combinational logic is described in a DAG of lookup tables that we
//!     // could use for various purposes
//!     for state in epoch0.ensemble().stator.states.vals() {
//!         awi::assert!(state.lowered_to_tnodes);
//!     }
//!
//!     // "retroactively" assign the input with a non-opaque value
//!     input.retro_(&awi!(0101)).unwrap();
//!     // check assertions (all `dag::assert*` functions and dynamic `unwrap`s done
//!     // during the current `Epoch`)
//!     epoch0.assert_assertions_strict().unwrap();
//!     // evaluate the outputs
//!     awi::assert_eq!(output_counter.eval().unwrap(), awi!(0011));
//!     awi::assert_eq!(output_data.eval().unwrap(), awi!(0xa505_u16));
//!
//!     // reassign and reevaluate
//!     input.retro_(&awi!(1011)).unwrap();
//!     awi::assert!(epoch0.assert_assertions().is_err());
//!     awi::assert_eq!(output_data.eval().unwrap(), awi!(0x7b0b_u16));
//! }
//! drop(epoch0);
//! ```

#![allow(clippy::needless_range_loop)]
#![allow(clippy::manual_flatten)]

mod awi_structs;
pub mod ensemble;
mod misc;
pub use awi_structs::{
    epoch, Assertions, Epoch, EvalAwi, LazyAwi, LazyInlAwi, Loop, LoopHandle, Net,
};
#[cfg(feature = "debug")]
pub use awint::awint_dag::triple_arena_render;
pub use awint::{self, awint_dag, awint_dag::triple_arena};
pub use misc::{SmallMap, StarRng};

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
