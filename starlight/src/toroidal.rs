use std::{borrow::Borrow, num::NonZeroUsize, ops::Deref};

use awint::{
    awint_dag::{dag, Lineage, PState},
    dag_prelude::{Bits, ExtAwi, InlAwi},
};

/// Returned from `Loop::drive` and other structures like `Net::drive` that use
/// `Loop`s internally, implements [awint::awint_dag::Lineage] so that the whole
/// DAG can be captured. In most cases, you will collect all the handles and add
/// them to the `leaves` argument of [awint::awint_dag::OpDag::new]
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

/// Provides a way to temporally wrap around a combinatorial circuit.
///
/// Get a `&Bits` reference from a `Loop` via the `Deref`, `Borrow<Bits>`, or
/// `AsRef<Bits>` impls, then consume the `Loop` with [Loop::drive].
///
/// The fundamental reason for temporal asymmetry is that there needs to be a
/// well defined root evaluation node and value.
#[derive(Debug)] // do not implement `Clone`
pub struct Loop {
    awi: ExtAwi,
}

impl Loop {
    /// Creates a `Loop` with an intial temporal value of zero and bitwidth `w`
    pub fn zero(w: NonZeroUsize) -> Self {
        // TODO add flag on opaque for initial value, and a way to notify if the
        // `LoopHandle` is not included in the graph
        Self {
            awi: ExtAwi::opaque(w),
        }
    }

    // TODO pub fn opaque() umax(), etc

    /// Returns the bitwidth of `self` as a `NonZeroUsize`
    pub fn nzbw(&self) -> NonZeroUsize {
        self.awi.nzbw()
    }

    /// Returns the bitwidth of `self` as a `usize`
    pub fn bw(&self) -> usize {
        self.awi.bw()
    }

    /// Get the driven value. This can conveniently be obtained by the `Deref`,
    /// `Borrow<Bits>`, and `AsRef<Bits>` impls on `Loop`.
    pub fn get(&self) -> &Bits {
        &self.awi
    }

    /// Consumes `self`, looping back with the value of `driver` to change the
    /// `Loop`s temporal value in a iterative temporal evaluation. Returns a
    /// `LoopHandle`. Returns `None` if `self.bw() != driver.bw()`.
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
        self.get()
    }
}

impl Borrow<Bits> for Loop {
    fn borrow(&self) -> &Bits {
        self.get()
    }
}

impl AsRef<Bits> for Loop {
    fn as_ref(&self) -> &Bits {
        self.get()
    }
}

/// A reconfigurable `Net` that is a `Vec`-like vector of "ports" that are
/// multiplexed to drive an internal `Loop`. First, [Net::get] or the trait
/// impls can be used to get the temporal value. Second, `Net::push_*` and
/// [Net::get_mut] can be write values to each of the ports. Third, [Net::drive]
/// takes a possibly dynamic index that multiplexes one of the values of the
/// ports to drive the temporal value.
#[derive(Debug)]
pub struct Net {
    driver: Loop,
    initial: ExtAwi,
    ports: Vec<ExtAwi>,
}

impl Net {
    /// Create a `Net` with an initial value of zero and bitwidth `w`
    pub fn zero(w: NonZeroUsize) -> Self {
        Self {
            driver: Loop::zero(w),
            initial: ExtAwi::zero(w),
            ports: vec![],
        }
    }

    /// Returns the current number of ports
    pub fn len(&self) -> usize {
        self.ports.len()
    }

    /// Returns if there are no ports on this `Net`
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    /// Returns the bitwidth of `self` as a `NonZeroUsize`
    pub fn nzbw(&self) -> NonZeroUsize {
        self.driver.nzbw()
    }

    /// Returns the bitwidth of `self` as a `usize`
    pub fn bw(&self) -> usize {
        self.driver.bw()
    }

    /// Pushes on a new port that is initially set to the initial value this
    /// `Net` was constructed with (and not the temporal value). If nothing is
    /// done to the port, and this port is selected as the driver, then the
    /// driven value will be the initial value this `Net` was originally
    /// constructed with. Returns a mutable reference to the port for
    /// immediate use (or the port can be accessed later by `get_mut`).
    pub fn push(&mut self) -> &mut Bits {
        self.ports.push(self.initial.clone());
        self.ports.last_mut().unwrap()
    }

    /// Get the temporal value. This can conveniently be obtained by the
    /// `Deref`, `Borrow<Bits>`, and `AsRef<Bits>` impls on `Net`.
    pub fn get(&self) -> &Bits {
        &self.driver
    }

    /// Gets the port at index `i`. Returns `None` if `i >= self.len()`.
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Bits> {
        self.ports.get_mut(i).map(|x| x.as_mut())
    }

    /// Drives with the value of the `inx`th port. Note that `inx` can be from
    /// a dynamic `dag::usize`.
    ///
    /// If `inx` is out of range, the initial value is driven (and _not_ the
    /// current temporal value). If `self.is_empty()`, the `LoopHandle` points
    /// to a loop being driven with the initial value.
    pub fn drive(mut self, inx: impl Into<dag::usize>) -> LoopHandle {
        let last = InlAwi::from_usize(self.len());
        // this elegantly handles the `self.is_empty()` case in addition to the out of
        // range case
        self.push();

        // set the index to `last` if it is out of range
        let mut inx = InlAwi::from_usize(inx);
        let gt = inx.ugt(&last).unwrap();
        inx.mux_(&last, gt).unwrap();

        // TODO need an optimized selector from `awint_dag`
        let mut selector = ExtAwi::uone(NonZeroUsize::new(self.len()).unwrap());
        selector.shl_(inx.to_usize()).unwrap();
        let mut tmp = ExtAwi::zero(self.nzbw());
        for i in 0..self.len() {
            tmp.mux_(&self.get_mut(i).unwrap(), selector.get(i).unwrap())
                .unwrap();
        }
        self.driver.drive(&tmp).unwrap()
    }
}

impl Deref for Net {
    type Target = Bits;

    fn deref(&self) -> &Self::Target {
        self.get()
    }
}

impl Borrow<Bits> for Net {
    fn borrow(&self) -> &Bits {
        self.get()
    }
}

impl AsRef<Bits> for Net {
    fn as_ref(&self) -> &Bits {
        self.get()
    }
}

// don't use `Index` and `IndexMut`, `IndexMut` requires `Index` and we do not
// want to introduce confusion
