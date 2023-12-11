use std::{borrow::Borrow, num::NonZeroUsize, ops::Deref};

use awint::{
    awint_dag::{smallvec::smallvec, Lineage, Op},
    dag::{self, awi, Awi, Bits, InlAwi},
};

use crate::{epoch::get_current_epoch, lower::meta::selector};

/// Provides a way to temporally wrap around a combinatorial circuit.
///
/// Get a `&Bits` temporal value from a `Loop` via one of the traits like
/// `Deref<Target=Bits>` or `AsRef<Bits>`, then drive the `Loop` with
/// [Loop::drive]. When [crate::Epoch::drive_loops] is run, it will evaluate the
/// value of the driver and use that to retroactively change the temporal value
/// of the loop.
///
/// The fundamental reason for temporal asymmetry is that there needs to be a
/// well defined root evaluation state and value.
#[derive(Debug)] // do not implement `Clone`, but maybe implement a `duplicate` function that
                 // explicitly duplicates drivers and loopbacks?
pub struct Loop {
    source: Awi,
}

impl Loop {
    /// Creates a `Loop` with an intial temporal value of zero and bitwidth `w`
    pub fn zero(w: NonZeroUsize) -> Self {
        let source = Awi::new(w, Op::Opaque(smallvec![], Some("LoopSource")));
        Self { source }
    }

    // TODO pub fn opaque(), umax(), From<&Bits>, etc. What we could do is have an
    // extra input to "LoopSource" that designates the initial value, but there are
    // many questions to be resolved

    /// Returns the bitwidth of `self` as a `NonZeroUsize`
    #[must_use]
    pub fn nzbw(&self) -> NonZeroUsize {
        self.source.nzbw()
    }

    /// Returns the bitwidth of `self` as a `usize`
    #[must_use]
    pub fn bw(&self) -> usize {
        self.source.bw()
    }

    /// Consumes `self`, looping back with the value of `driver` to change the
    /// `Loop`s temporal value in a iterative temporal evaluation. Returns
    /// `None` if `self.bw() != driver.bw()`.
    #[must_use]
    pub fn drive(self, driver: &Bits) -> Option<()> {
        if self.source.bw() != driver.bw() {
            None
        } else {
            let epoch = get_current_epoch().unwrap();
            let mut lock = epoch.epoch_data.borrow_mut();
            lock.ensemble
                .stator
                .states
                .get_mut(self.source.state())
                .unwrap()
                .op = Op::Opaque(smallvec![driver.state()], Some("LoopSource"));
            lock.ensemble
                .stator
                .states
                .get_mut(driver.state())
                .unwrap()
                .inc_rc();
            Some(())
        }
    }
}

impl Deref for Loop {
    type Target = Bits;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl Borrow<Bits> for Loop {
    fn borrow(&self) -> &Bits {
        &self.source
    }
}

impl AsRef<Bits> for Loop {
    fn as_ref(&self) -> &Bits {
        &self.source
    }
}

/// A reconfigurable `Net` that is a `Vec`-like vector of "ports" that are
/// multiplexed to drive an internal `Loop`. First, use a trait like
/// `Deref<Target=Bits>` or `AsRef<Bits>` to get the temporal value. Second,
/// [Net::push] and [Net::get_mut] can be used to write values to each of the
/// ports. Third, [Net::drive] takes a possibly dynamic index that multiplexes
/// one of the values of the ports to drive the temporal value across
/// [crate::Epoch::drive_loops] calls.
#[derive(Debug)]
pub struct Net {
    source: Loop,
    ports: Vec<Awi>,
}

impl Net {
    /// Create a `Net` with an initial temporal value of zero and bitwidth `w`
    pub fn zero(w: NonZeroUsize) -> Self {
        Self {
            source: Loop::zero(w),
            ports: vec![],
        }
    }

    /// Creates a `Net` with [Net::zero] and pushes on `num_ports` ports
    /// initialized to zero.
    pub fn zero_with_ports(w: NonZeroUsize, num_ports: usize) -> Self {
        Self {
            source: Loop::zero(w),
            ports: vec![Awi::zero(w); num_ports],
        }
    }

    /// Returns the current number of ports
    #[must_use]
    pub fn len(&self) -> usize {
        self.ports.len()
    }

    /// Returns if there are no ports on this `Net`
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ports.is_empty()
    }

    /// Returns the bitwidth of `self` as a `NonZeroUsize`
    #[must_use]
    pub fn nzbw(&self) -> NonZeroUsize {
        self.source.nzbw()
    }

    /// Returns the bitwidth of `self` as a `usize`
    #[must_use]
    pub fn bw(&self) -> usize {
        self.source.bw()
    }

    /// Pushes on a new port. Returns `None` if the bitwidth mismatches the
    /// width that this `Net` was created with
    #[must_use]
    pub fn push(&mut self, port: &Bits) -> Option<()> {
        if port.bw() != self.bw() {
            None
        } else {
            self.ports.push(Awi::from(port));
            Some(())
        }
    }

    /// Gets a mutable reference to the port at index `i`. Returns `None` if `i
    /// >= self.len()`.
    #[must_use]
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Bits> {
        self.ports.get_mut(i).map(|x| x.as_mut())
    }

    /// Adds a port to `self` and `other` that use each other's temporal values
    /// as inputs. Returns `None` if bitwidths mismatch
    #[must_use]
    pub fn exchange(&mut self, rhs: &mut Self) -> Option<()> {
        if self.bw() != rhs.bw() {
            None
        } else {
            self.ports.push(Awi::from(rhs.as_ref()));
            rhs.ports.push(Awi::from(self.as_ref()));
            Some(())
        }
    }

    /// Drives with the value of the `inx`th port. Note that `inx` can be from
    /// a dynamic `dag::usize`.
    ///
    /// If `inx` is out of range, the return value is a runtime or dynamic
    /// `None`.
    #[must_use]
    pub fn drive(self, inx: impl Into<dag::usize>) -> dag::Option<()> {
        if self.is_empty() {
            return dag::Option::None;
        }
        let max_inx = self.len() - 1;
        let max_inx_bits = max_inx.next_power_of_two().trailing_zeros() as usize;
        let inx = InlAwi::from_usize(inx.into());
        // we detect overflow by seeing if any of these bits are nonzero or if the rest
        // of the index is greater than the expected max bits (only needed if the
        // self.len() is not a power of two)
        let should_stay_zero = awi!(inx[max_inx_bits..]).unwrap();
        let mut in_range = should_stay_zero.is_zero();
        let inx = awi!(inx[..max_inx_bits]).unwrap();
        let signals = selector(&inx, None);
        if !self.len().is_power_of_two() {
            let le = inx.ule(&InlAwi::from_usize(max_inx)).unwrap();
            in_range &= le;
        }

        let mut tmp = Awi::zero(self.nzbw());
        for i in 0..self.len() {
            tmp.mux_(&self.ports[i], signals[i].to_bool()).unwrap();
        }
        self.source.drive(&tmp).unwrap();
        dag::Option::some_at_dagtime((), in_range)
    }

    // TODO we can do this
    // Drives with a one-hot vector of selectors.
    //pub fn drive_priority(mut self, inx: impl Into<dag::usize>) {
    //pub fn drive_onehot(mut self, onehot)
}

impl Deref for Net {
    type Target = Bits;

    fn deref(&self) -> &Self::Target {
        &self.source
    }
}

impl Borrow<Bits> for Net {
    fn borrow(&self) -> &Bits {
        &self.source
    }
}

impl AsRef<Bits> for Net {
    fn as_ref(&self) -> &Bits {
        &self.source
    }
}

// don't use `Index` and `IndexMut`, `IndexMut` requires `Index` and we do not
// want to introduce confusion
