use core::fmt;
use std::num::NonZeroUsize;

use awint::awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena, SurjectArena};

use crate::{ensemble::PExternal, Error, EvalAwi, LazyAwi};

ptr_struct!(PMeta(); PCorrespond());

/// Provides a controlled way to correspond `LazyAwi`s and `EvalAwi`s in and
/// between different `Epoch`s.
pub struct Corresponder {
    a: OrdArena<PMeta, PExternal, PCorrespond>,
    c: SurjectArena<PCorrespond, PMeta, NonZeroUsize>,
}

impl Clone for Corresponder {
    fn clone(&self) -> Self {
        Self {
            a: self.a.clone(),
            c: self.c.clone(),
        }
    }
}

impl fmt::Debug for Corresponder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Corresponder")
            .field("a", &self.a)
            .field("c", &self.c)
            .finish()
    }
}

impl Corresponder {
    pub fn new() -> Self {
        Self {
            a: OrdArena::new(),
            c: SurjectArena::new(),
        }
    }

    fn get_or_insert_lazy<L: std::borrow::Borrow<LazyAwi>>(
        &mut self,
        l: &L,
    ) -> (PCorrespond, NonZeroUsize) {
        let l = l.borrow();
        let p = l.p_external();
        let w = l.nzbw();
        (
            if let Some(p_meta) = self.a.find_key(&p) {
                *self.a.get_val(p_meta).unwrap()
            } else {
                self.c.insert_with(|p_c| (self.a.insert(p, p_c).0, w))
            },
            w,
        )
    }

    /// Corresponds `l0` with `l1`. This relationship is bidirectional, and if
    /// something is corresponded more than once, everything ever corresponded
    /// with it will all have a correspondence together.
    pub fn correspond_lazy<L0: std::borrow::Borrow<LazyAwi>, L1: std::borrow::Borrow<LazyAwi>>(
        &mut self,
        l0: &L0,
        l1: &L1,
    ) -> Result<(), Error> {
        let (p_c0, w0) = self.get_or_insert_lazy(l0);
        let (p_c1, w1) = self.get_or_insert_lazy(l1);
        if w0 != w1 {
            Err(Error::BitwidthMismatch(w0.get(), w1.get()))
        } else {
            // not insert_key because this could be two sets getting corresponded
            let _ = self.c.union(p_c0, p_c1);
            Ok(())
        }
    }

    fn get_or_insert_eval<E: std::borrow::Borrow<EvalAwi>>(
        &mut self,
        e: &E,
    ) -> (PCorrespond, NonZeroUsize) {
        let e = e.borrow();
        let p = e.p_external();
        let w = e.nzbw();
        (
            if let Some(p_meta) = self.a.find_key(&p) {
                *self.a.get_val(p_meta).unwrap()
            } else {
                self.c.insert_with(|p_c| (self.a.insert(p, p_c).0, w))
            },
            w,
        )
    }

    /// Corresponds `e0` with `e1`. This relationship is bidirectional, and if
    /// something is corresponded more than once, everything ever corresponded
    /// with it will all have a correspondence together
    pub fn correspond_eval<E0: std::borrow::Borrow<EvalAwi>, E1: std::borrow::Borrow<EvalAwi>>(
        &mut self,
        e0: &E0,
        e1: &E1,
    ) -> Result<(), Error> {
        let (p_c0, w0) = self.get_or_insert_eval(e0);
        let (p_c1, w1) = self.get_or_insert_eval(e1);
        if w0 != w1 {
            Err(Error::BitwidthMismatch(w0.get(), w1.get()))
        } else {
            let _ = self.c.union(p_c0, p_c1);
            Ok(())
        }
    }

    /// Returns a vector of `LazyAwi`s for everything that was
    /// corresponded with `l` and is usable with the currently active `Epoch`.
    pub fn correspondences_lazy<L: std::borrow::Borrow<LazyAwi>>(
        &self,
        l: &L,
    ) -> Result<Vec<LazyAwi>, Error> {
        let l = l.borrow();
        let p = l.p_external();
        if let Some(p_meta) = self.a.find_key(&p) {
            let p_start = *self.a.get_val(p_meta).unwrap();
            let mut adv = self.c.advancer_surject(p_start);
            let mut v = vec![];
            while let Some(p_correspond) = adv.advance(&self.c) {
                let p_meta = *self.c.get_key(p_correspond).unwrap();
                let p_external = *self.a.get_key(p_meta).unwrap();
                if p_external != p {
                    if let Ok(l) = LazyAwi::try_clone_from(p_external, None) {
                        v.push(l);
                    }
                }
            }
            Ok(v)
        } else {
            Err(Error::CorrespondenceNotFound(p))
        }
    }

    /// If `l` has been corresponded with exactly one other `LazyAwi` valid in
    /// the currently active `Epoch`, this will return a reference the
    /// corresponding `LazyAwi`.
    pub fn transpose_lazy<L: std::borrow::Borrow<LazyAwi>>(&self, l: &L) -> Result<LazyAwi, Error> {
        let tmp = l.borrow();
        let p = tmp.p_external();
        let mut v = self.correspondences_lazy(&tmp)?;
        if v.is_empty() {
            return Err(Error::CorrespondenceEmpty(p))
        }
        if v.len() == 1 {
            Ok(v.pop().unwrap())
        } else {
            Err(Error::CorrespondenceNotATranspose(tmp.p_external()))
        }
    }

    /// Returns a vector of `EvalAwi`s for everything that was
    /// corresponded with `l` and is usable with the currently active `Epoch`.
    pub fn correspondences_eval<E: std::borrow::Borrow<EvalAwi>>(
        &self,
        e: &E,
    ) -> Result<Vec<EvalAwi>, Error> {
        let e = e.borrow();
        let p = e.p_external();
        if let Some(p_meta) = self.a.find_key(&p) {
            let p_start = *self.a.get_val(p_meta).unwrap();
            let mut adv = self.c.advancer_surject(p_start);
            let mut v = vec![];
            while let Some(p_correspond) = adv.advance(&self.c) {
                let p_meta = *self.c.get_key(p_correspond).unwrap();
                let p_external = *self.a.get_key(p_meta).unwrap();
                if p_external != p {
                    if let Ok(l) = EvalAwi::try_clone_from(p_external) {
                        v.push(l);
                    }
                }
            }
            Ok(v)
        } else {
            Err(Error::CorrespondenceNotFound(p))
        }
    }

    /// If `l` has been corresponded with exactly one other `EvalAwi` valid in
    /// the currently active `Epoch`, this will return a reference the
    /// corresponding `EvalAwi`.
    pub fn transpose_eval<E: std::borrow::Borrow<EvalAwi>>(&self, e: &E) -> Result<EvalAwi, Error> {
        let tmp = e.borrow();
        let p = tmp.p_external();
        let mut v = self.correspondences_eval(&tmp)?;
        if v.is_empty() {
            return Err(Error::CorrespondenceEmpty(p))
        }
        if v.len() == 1 {
            Ok(v.pop().unwrap())
        } else {
            Err(Error::CorrespondenceNotATranspose(tmp.p_external()))
        }
    }
}

impl Default for Corresponder {
    fn default() -> Self {
        Self::new()
    }
}
