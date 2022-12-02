use std::{
    borrow::Borrow,
    num::NonZeroUsize,
    ops::{Deref, Index, IndexMut},
};

use awint::{
    awint_dag::{dag, Lineage, PState},
    dag_prelude::{Bits, ExtAwi, InlAwi},
};

/// Returned from `Loop::drive` and `Net::drive`, implements
/// [awint::awint_dag::Lineage] so that the whole DAG can be captured. In most
/// cases, you will collect all the handles and add them to the `leaves`
/// argument of [awint::awint_dag::OpDag::new]
#[derive(Debug)]
pub struct LoopHandle {
    // just use this for now to have the non-sendability
    awi: ExtAwi,
}

impl Lineage for LoopHandle {
    fn state(&self) -> PState {
        self.awi.state()
    }
}

/// Provides a way to temporally and toroidally wrap around a combinatorial
/// circuit.
///
/// Get a `&Bits` reference from a `Loop` via the `Deref`, `Borrow<Bits>`, or
/// `AsRef<Bits>` impls
///
/// The fundamental reason for temporal asymmetry is that there needs to be a
/// well defined initial evaluation node and value.
#[derive(Debug)] // do not implement `Clone`
pub struct Loop {
    awi: ExtAwi,
}

impl Loop {
    /// Creates a `Loop` with an intial value of zero
    pub fn zero(bw: NonZeroUsize) -> Self {
        // TODO add flag on opaque for initial value
        Self {
            awi: ExtAwi::opaque(bw),
        }
    }

    // TODO pub fn opaque() umax(), etc

    /// Consumes `self`, looping back with the value of `driver` to change the
    /// `Loop`s previous value in a temporal evaluation.
    pub fn drive(mut self, driver: &Bits) -> Option<LoopHandle> {
        // TODO use id from `awi`, for now since there are only `Loops` we denote a loop
        // with a double input `Opaque`
        if self.awi.bw() != driver.bw() {
            None
        } else {
            self.awi.opaque_with_(&[driver]);
            Some(LoopHandle { awi: self.awi })
        }
    }
}

// TODO From<&Bits> and other constructions

impl Deref for Loop {
    type Target = Bits;

    fn deref(&self) -> &Self::Target {
        &self.awi
    }
}

impl Borrow<Bits> for Loop {
    fn borrow(&self) -> &Bits {
        &self.awi
    }
}

impl AsRef<Bits> for Loop {
    fn as_ref(&self) -> &Bits {
        &self.awi
    }
}

/// A reconfigurable `Net` that has a number of inputs, outputs, and is driven
/// by an possibly dynamic index that chooses one input to drive the outputs
///
/// Implements `Index` and `IndexMut` for quick port access
#[derive(Debug)]
pub struct Net {
    driver: Loop,
    ports: Vec<ExtAwi>,
}

impl Net {
    pub fn zero(bw: NonZeroUsize) -> Self {
        Self {
            driver: Loop::zero(bw),
            ports: vec![],
        }
    }

    /// Returns the current number of ports
    pub fn len(&self) -> usize {
        self.ports.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    /// Returns the bitwidth of the ports
    pub fn nzbw(&self) -> NonZeroUsize {
        self.ports[0].nzbw()
    }

    pub fn bw(&self) -> usize {
        self.nzbw().get()
    }

    /// Pushes on a new port that is initialized to a zero value, and returns a
    /// mutable reference to that value.
    pub fn push_zero(&mut self) -> &mut Bits {
        self.ports.push(ExtAwi::from(self.driver.as_ref()));
        self.ports.last_mut().unwrap()
    }

    /// Returns a reference to the `i`th port
    pub fn get(&self, i: usize) -> Option<&Bits> {
        self.ports.get(i).map(|x| x.as_ref())
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Bits> {
        self.ports.get_mut(i).map(|x| x.as_mut())
    }

    /// Drives all the ports with the `inx`th port. Note that `inx` can be from
    /// a dynamic `dag::usize`.
    ///
    /// If `inx` is out of range, the zeroeth port is driven. If `self.len()` is
    /// 0, the `LoopHandle` points to an opaque loop driving itself.
    pub fn drive(mut self, inx: impl Into<dag::usize>) -> LoopHandle {
        // I feel like there is no need to return a `None`, nothing has been read or
        // written
        if self.is_empty() {
            // TODO make opaque
            self.push_zero();
        }

        // zero the index if it is out of range
        let mut inx = InlAwi::from_usize(inx);
        let ge = inx.uge(&InlAwi::from_usize(self.len())).unwrap();
        inx.mux_(&InlAwi::from_usize(0), ge).unwrap();

        let mut selector = ExtAwi::uone(NonZeroUsize::new(self.len()).unwrap());
        selector.shl_(inx.to_usize()).unwrap();
        let mut tmp = ExtAwi::zero(self.ports[0].nzbw());
        for i in 0..self.len() {
            tmp.mux_(&self[i], selector.get(i).unwrap()).unwrap();
        }
        self.driver.drive(&tmp).unwrap()
    }
}

impl<B: Borrow<usize>> Index<B> for Net {
    type Output = Bits;

    fn index(&self, i: B) -> &Bits {
        self.get(*i.borrow()).unwrap()
    }
}

impl<B: Borrow<usize>> IndexMut<B> for Net {
    fn index_mut(&mut self, i: B) -> &mut Bits {
        self.get_mut(*i.borrow()).unwrap()
    }
}
