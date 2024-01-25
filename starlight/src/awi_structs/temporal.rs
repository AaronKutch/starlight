use std::{borrow::Borrow, num::NonZeroUsize, ops::Deref};

use awint::{
    awint_dag::{smallvec::smallvec, Lineage, Op, PState},
    dag::{self, awi, Awi, Bits, InlAwi},
};

use crate::{epoch::get_current_epoch, lower::meta::general_mux, Error};

/// Provides a way to temporally wrap around a combinatorial circuit.
///
/// Get a `&Bits` temporal value from a `Loop` via one of the traits like
/// `Deref<Target=Bits>` or `AsRef<Bits>`, then drive the `Loop` with
/// [Loop::drive]. When [crate::Epoch::drive_loops] is run, it will evaluate the
/// value of the driver and use that to retroactively change the temporal value
/// of the loop.
///
/// ```
/// use starlight::{awi, dag::*, Epoch, EvalAwi, Loop};
/// let epoch = Epoch::new();
///
/// let looper = Loop::zero(bw(4));
/// // evaluate the value of `looper` at this point later
/// let val = EvalAwi::from(&looper);
/// let mut tmp = awi!(looper);
/// tmp.inc_(true);
/// // drive the `Loop` with itself incremented
/// looper.drive(&tmp).unwrap();
///
/// {
///     use awi::*;
///     for i in 0..16 {
///         // check that the evaluated value is equal to
///         // this loop iteration number
///         awi::assert_eq!(i, val.eval().unwrap().to_usize());
///         // every time `drive_loops` is called,
///         // the evaluated value increases by one
///         epoch.drive_loops().unwrap();
///     }
/// }
/// drop(epoch);
/// ```
// The fundamental reason for temporal asymmetry is that there needs to be a
// well defined root evaluation state and value.
#[derive(Debug)] // do not implement `Clone`, but maybe implement a `duplicate` function that
                 // explicitly duplicates drivers and loopbacks?
pub struct Loop {
    source: Awi,
}

macro_rules! loop_basic_value {
    ($($fn:ident)*) => {
        $(
            /// Creates a `Loop` with the intial temporal value and bitwidth `w`
            pub fn $fn(w: NonZeroUsize) -> Self {
                Self::from_state(Awi::$fn(w).state())
            }
        )*
    }
}

macro_rules! loop_from_impl {
    ($($fn:ident $t:ident);*;) => {
        $(
            pub fn $fn(x: dag::$t) -> Self {
                Self::from_state(x.state())
            }
        )*
    }
}

/// Functions for creating a `Loop` with the intial temporal value of `x`. The
/// value must evaluate to a constant.
impl Loop {
    loop_from_impl!(
        from_bool bool;
        from_u8 u8;
        from_i8 i8;
        from_u16 u16;
        from_i16 i16;
        from_u32 u32;
        from_i32 i32;
        from_u64 u64;
        from_i64 i64;
        from_u128 u128;
        from_i128 i128;
        from_usize usize;
        from_isize isize;
    );
}

impl Loop {
    loop_basic_value!(opaque zero umax imax imin uone);

    /// Used internally to create `Loop`s
    ///
    /// # Panics
    ///
    /// If an `Epoch` does not exist or the `PState` was pruned
    pub fn from_state(p_state: PState) -> Self {
        let w = p_state.get_nzbw();
        let source = Awi::new(w, Op::Opaque(smallvec![p_state], Some("LoopSource")));
        Self { source }
    }

    /// Creates a `Loop` with the intial temporal value of `bits`. The value
    /// must evaluate to a constant.
    pub fn from_bits(bits: &dag::Bits) -> Self {
        Self::from_state(bits.state())
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

    /// Consumes `self`, looping back with the value of `driver` to change the
    /// `Loop`s temporal value in a iterative temporal evaluation. Returns
    /// an error if `self.bw() != driver.bw()`.
    pub fn drive(self, driver: &Bits) -> Result<(), Error> {
        if self.source.bw() != driver.bw() {
            Err(Error::WrongBitwidth)
        } else {
            let epoch = get_current_epoch().unwrap();
            let mut lock = epoch.epoch_data.borrow_mut();
            // add the driver to the loop source
            let op = &mut lock
                .ensemble
                .stator
                .states
                .get_mut(self.source.state())
                .unwrap()
                .op;
            if let Op::Opaque(v, Some("LoopSource")) = op {
                assert_eq!(v.len(), 1);
                v.push(driver.state());
            } else {
                unreachable!()
            }
            // increment the reference count on the driver
            lock.ensemble
                .stator
                .states
                .get_mut(driver.state())
                .unwrap()
                .inc_rc();
            // in order for loop driving to always work we need to do this (otherwise
            // `drive_loops` would have to search all states, or we would need the old loop
            // handle strategy which was horrible to use)
            lock.ensemble
                .stator
                .states_to_lower
                .push(self.source.state());
            Ok(())
        }
    }

    // TODO FP<B> is violating the Hash, Eq, Ord requirements of `Borrow`, but
    // `AsRef` does not have the reflexive blanket impl, perhaps we need a
    // `BorrowBits` trait that also handles the primitives, and several signatures
    // like this `drive` could be written with <B: BorrowBits>. We also need to find
    // a way around the movement problem, possibly by requiring `&B` always or maybe
    // inventing some other kind of trait, there are also cases where we do want to
    // move.
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
///
/// Note: In most HDL oriented cases, you will want to create `Net`s with
/// `Net::opaque` to simulate a net starting with an undefined value that must
/// be driven with a definite value from outside.
#[derive(Debug)]
pub struct Net {
    source: Loop,
    ports: Vec<Awi>,
}

macro_rules! net_basic_value {
    ($($fn:ident)*) => {
        $(
            /// Creates a `Net` with the intial temporal value and port bitwidth `w`
            pub fn $fn(w: NonZeroUsize) -> Self {
                Self::from_state(Awi::$fn(w).state())
            }
        )*
    }
}

macro_rules! net_from_impl {
    ($($fn:ident $t:ident);*;) => {
        $(
            pub fn $fn(x: dag::$t) -> Self {
                Self::from_state(x.state())
            }
        )*
    }
}

macro_rules! net_push_impl {
    ($($fn:ident $t:ident);*;) => {
        $(
            #[must_use]
            pub fn $fn(&mut self, port: dag::$t) -> Option<()> {
                self.push_state(port.state())
            }
        )*
    }
}

/// Functions for creating a `Net` with the intial temporal value of `x`. The
/// value must evaluate to a constant.
impl Net {
    net_from_impl!(
        from_bool bool;
        from_u8 u8;
        from_i8 i8;
        from_u16 u16;
        from_i16 i16;
        from_u32 u32;
        from_i32 i32;
        from_u64 u64;
        from_i64 i64;
        from_u128 u128;
        from_i128 i128;
        from_usize usize;
        from_isize isize;
    );
}

/// Pushes on a new port. Returns `None` if the bitwidth mismatches the
/// width that this `Net` was created with.
impl Net {
    net_push_impl!(
        push_bool bool;
        push_u8 u8;
        push_i8 i8;
        push_u16 u16;
        push_i16 i16;
        push_u32 u32;
        push_i32 i32;
        push_u64 u64;
        push_i64 i64;
        push_u128 u128;
        push_i128 i128;
        push_usize usize;
        push_isize isize;
    );
}

impl Net {
    net_basic_value!(opaque zero umax imax imin uone);

    /// Used internally to create `Net`s
    ///
    /// # Panics
    ///
    /// If an `Epoch` does not exist or the `PState` was pruned
    pub fn from_state(p_state: PState) -> Self {
        Self {
            source: Loop::from_state(p_state),
            ports: vec![],
        }
    }

    /// Creates a `Net` with the intial temporal value of `bits`. The value
    /// must evaluate to a constant.
    pub fn from_bits(bits: &dag::Bits) -> Self {
        Self::from_state(bits.state())
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

    /// Internal function for pushing on a new port. Returns `None` if the
    /// bitwidth mismatches the width that this `Net` was created with.
    #[must_use]
    pub fn push_state(&mut self, port: PState) -> Option<()> {
        if port.get_nzbw() != self.nzbw() {
            None
        } else {
            self.ports.push(Awi::from_state(port));
            Some(())
        }
    }

    /// Pushes on a new port. Returns `None` if the bitwidth mismatches the
    /// width that this `Net` was created with.
    #[must_use]
    pub fn push(&mut self, port: &Bits) -> Option<()> {
        self.push_state(port.state())
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
    /// `None`. The source value if the `inx` is out of range is not specified,
    /// and it may result in an undriven `Loop` in some cases, so the return
    /// `Option` should probably be `unwrap`ed.
    #[must_use]
    pub fn drive(self, inx: &Bits) -> dag::Option<()> {
        if self.is_empty() {
            return dag::Option::None;
        }
        if self.len() == 1 {
            self.source.drive(&self.ports[0]).unwrap();
            return dag::Option::some_at_dagtime((), inx.is_zero());
        }
        let max_inx = self.len() - 1;
        let max_inx_bits = Bits::nontrivial_bits(max_inx).unwrap().get();

        // use this instead of `efficient_ule` because here we can avoid many cases if
        // `max_inx_bits >= inx.bw()`
        let should_stay_zero = if max_inx_bits < inx.bw() {
            awi!(inx[max_inx_bits..]).unwrap()
        } else {
            awi!(0)
        };
        let mut in_range = should_stay_zero.is_zero();
        if (!self.len().is_power_of_two()) && (inx.bw() >= max_inx_bits) {
            // dance to avoid stuff that can get lowered into a full `BITS` sized comparison
            let mut max = Awi::zero(inx.nzbw());
            max.usize_(max_inx);
            let le = inx.ule(&max).unwrap();
            in_range &= le;
        }

        let small_inx = if max_inx_bits < inx.bw() {
            awi!(inx[..max_inx_bits]).unwrap()
        } else if max_inx_bits > inx.bw() {
            awi!(zero: .., inx; ..max_inx_bits).unwrap()
        } else {
            Awi::from(inx)
        };
        let tmp = general_mux(&self.ports, &small_inx);
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
