use core::fmt;
use std::{fmt::Debug, num::NonZeroU128};

use crate::ensemble::PExternal;

// TODO in regular cases add errors that lazily produce a formatted output. Keep
// things using `OtherStr` and `OtherString` if they are special cases like
// improper `Epoch` management or internal failures or things like lowering that
// will be changed in the future. Conversely, add special variants for things
// users might match against

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, thiserror::Error)]
pub enum Error {
    /// This indicates an invalid `triple_arena::Ptr` was used
    #[error("InvalidPtr")]
    InvalidPtr,
    /// If there is an `Op` that cannot be evaluated
    #[error("Unevaluatable")]
    Unevaluatable,
    /// If an operand has a bitwidth mismatch or unexpected bitwidth
    #[error("bitwidth mismatch: lhs: {0}, rhs: {1}")]
    BitwidthMismatch(usize, usize),
    /// If something like `Out<W>` was constructed with the wrong bitwidth
    #[error("bitwidth {0} does not match the const required bitwidth {1}")]
    ConstBitwidthMismatch(usize, usize),
    /// If an operand to [crate::Drive::drive] was `None` when it should contain
    /// something
    #[error(
        "an operand to `crate::Drive::drive` was `None` or some other empty value, the other \
         operand was {0:#?}"
    )]
    DrivenValueIsNone(Option<PExternal>),
    /// If an operation that needs an active `Epoch` is called when none are
    /// active
    #[error("there is no `starlight::Epoch` that is currently active")]
    NoCurrentlyActiveEpoch,
    /// If there is an active `Epoch` but the operation needs a different one
    #[error(
        "the currently active `starlight::Epoch` is not the correct one for this operation; some \
         `Epoch` operations require that `self` is the current `Epoch`"
    )]
    WrongCurrentlyActiveEpoch,
    /// If an `RNode` was requested that cannot be found
    #[error(
        "could not find thread local `RNode` corresponding to {0:#?}, probably an `EvalAwi` or \
         `LazyAwi` was used outside of the `Epoch` it was created in"
    )]
    InvalidPExternal(PExternal),
    /// If the associated state of an `RNode` was requested but could not be
    /// found
    #[error(
        "could not find associated thread local state corresponding to {0:#?}, probably an \
         `EvalAwi` or `LazyAwi` was attempted to be used for mimicking operations but the `Epoch` \
         was already pruned or optimized"
    )]
    StatePruned(PExternal),
    /// Could not find something in a `Corresponder`
    #[error("could not find {0:#?} in the `Corresponder`")]
    CorrespondenceNotFound(PExternal),
    /// For miscellanious errors
    #[error("{0}")]
    OtherStr(&'static str),
    /// For miscellanious errors
    #[error("{0}")]
    OtherString(String),
}

pub(crate) struct DisplayStr<'a>(pub &'a str);
impl<'a> Debug for DisplayStr<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{}", self.0))
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

pub(crate) struct HexadecimalNonZeroU128(pub NonZeroU128);
impl fmt::Debug for HexadecimalNonZeroU128 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!("{:x?}", self.0))
    }
}
