use core::fmt;
use std::fmt::Debug;

// TODO in regular cases add errors that lazily produce a formatted output. Keep
// things using `OtherStr` and `OtherString` if they are special cases like
// improper `Epoch` management or internal failures or things like lowering that
// will be changed in the future. Conversely, add special variants for things
// users might match against

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, thiserror::Error)]
pub enum Error {
    /// This indicates an invalid [triple_arena::Ptr] was used
    #[error("InvalidPtr")]
    InvalidPtr,
    /// If there is an `Op` that cannot be evaluated
    #[error("Unevaluatable")]
    Unevaluatable,
    /// If an operand has a bitwidth mismatch or unexpected bitwidth
    #[error("WrongBitwidth")]
    WrongBitwidth,
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
        match self {
            Self::InvalidPtr => write!(f, "InvalidPtr"),
            Self::Unevaluatable => write!(f, "Unevaluatable"),
            Self::WrongBitwidth => write!(f, "WrongBitwidth"),
            Self::OtherStr(arg0) => f.debug_tuple("OtherStr").field(&DisplayStr(arg0)).finish(),
            Self::OtherString(arg0) => f
                .debug_tuple("OtherString")
                .field(&DisplayStr(arg0))
                .finish(),
        }
    }
}
