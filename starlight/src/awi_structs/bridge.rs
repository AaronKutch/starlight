use crate::{Error, EvalAwi, LazyAwi};

pub trait Drive<Rhs> {
    fn drive(&mut self, rhs: &Rhs) -> Result<(), Error>;
}

impl<E: std::borrow::Borrow<EvalAwi>, L: Into<LazyAwi>> Drive<E> for Option<L> {
    fn drive(&mut self, rhs: &E) -> Result<(), Error> {
        let rhs = rhs.borrow();
        self.take()
            .ok_or(Error::DrivenValueIsNone(Some(rhs.p_external())))?
            .into()
            .drive(rhs)
    }
}

impl<E: std::borrow::Borrow<EvalAwi>, L: Into<LazyAwi>> Drive<Option<E>> for Option<L> {
    fn drive(&mut self, rhs: &Option<E>) -> Result<(), Error> {
        match (self.take(), rhs) {
            (None, None) => Err(Error::DrivenValueIsNone(None)),
            (Some(lhs), None) => Err(Error::DrivenValueIsNone(Some(lhs.into().p_external()))),
            (None, Some(rhs)) => Err(Error::DrivenValueIsNone(Some(rhs.borrow().p_external()))),
            (Some(lhs), Some(rhs)) => lhs.into().drive(rhs.borrow()),
        }
    }
}
