use core::fmt;
use std::fmt::Debug;

// TODO in regular cases add errors that lazily produce a formatted output. Keep
// things using `OtherStr` and `OtherString` if they are special cases like
// improper `Epoch` management or internal failures or things like lowering that
// will be changed in the future. Conversely, add special variants for things
// users might match against

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, thiserror::Error)]
pub enum Error {
    // An operand points nowhere, so the DAG is broken
    #[error("InvalidPtr")]
    InvalidPtr,
    // Thrown if a `Literal`, `Invalid`, or `Opaque` node is attempted to be evaluated
    #[error("Unevaluatable")]
    Unevaluatable,
    // wrong bitwidths of operands
    #[error("WrongBitwidth")]
    WrongBitwidth,
    // Some other kind of brokenness, such as dependency edges not agreeing with operand edges
    #[error("{0}")]
    OtherStr(&'static str),
    #[error("{0}")]
    OtherString(String),
}

struct DisplayStr<'a>(pub &'a str);
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