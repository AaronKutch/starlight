use std::{borrow::Borrow, num::NonZeroUsize, ops::Deref};

use awint::{
    awint_dag::{smallvec::smallvec, Lineage, Op, PState},
    bw,
    dag::{self, awi, Awi, Bits, InlAwi},
};

use crate::{awi, epoch::get_current_epoch, lower::meta::general_mux, Delay, Error};

/// Delays the temporal value propogation of `bits` by `delay`.
///
/// For a purely combinatorial circuit that is run for an infinite time, this
/// function acts like a no-op; the effects of this function are seen in
/// temporal evaluation and in non-DAG interactions. The initial temporal value
/// `bits` is set to an opaque value, and then after `delay` it is set to the
/// value it was driven with. Any other temporal changes to the value of
/// `bits` are also delayed in their changes. Note there is no delay effect for
/// anything that used `bits` before it was used as an argument to this
/// function, only after this function is there an applied effect, as if this
/// function changes `bits` into its future value.
///
/// ```
/// use dag::*;
/// use starlight::{awi, dag, delay, Epoch, EvalAwi, LazyAwi};
/// let epoch = Epoch::new();
///
/// let mut a = awi!(0xa_u4);
/// let a_before = EvalAwi::from(&a);
/// // delay for 10 units
/// delay(&mut a, 10);
/// let a_after = EvalAwi::from(&a);
/// {
///     use awi::{assert, assert_eq, *};
///     // `a_before` with no delay in between it and the
///     // `0xa_u4` value immediately evaluates
///     assert_eq!(a_before.eval().unwrap(), awi!(0xa_u4));
///     // `a_after` starts as an `Opaque`
///     assert!(a_after.eval_is_all_unknown().unwrap());
///     // the epoch has not quiesced since
///     // there are still future events
///     assert!(!epoch.quiesced().unwrap());
///
///     epoch.run(9).unwrap();
///     assert!(a_after.eval_is_all_unknown().unwrap());
///     assert!(!epoch.quiesced().unwrap());
///
///     // only after 10 units does the
///     // value finally finish propogating
///     epoch.run(1).unwrap();
///     assert_eq!(a_after.eval().unwrap(), awi!(0xa_u4));
///     assert!(epoch.quiesced().unwrap());
/// }
///
/// let x = LazyAwi::opaque(bw(4));
/// let mut y = awi!(x);
/// delay(&mut y, 10);
/// let y = EvalAwi::from(&x);
/// {
///     use awi::{assert, assert_eq, *};
///
///     // immediate quiescence since the driver is already opaque
///     assert!(epoch.quiesced().unwrap());
///
///     x.retro_(&awi!(0xb_u4)).unwrap();
///     assert!(!epoch.quiesced().unwrap());
///     epoch.run(10).unwrap();
///     assert!(epoch.quiesced().unwrap());
///     assert_eq!(y.eval().unwrap(), awi!(0xb_u4));
///
///     x.retro_(&awi!(0xc_u4)).unwrap();
///     assert!(!epoch.quiesced().unwrap());
///     epoch.run(10).unwrap();
///     assert!(epoch.quiesced().unwrap());
///     assert_eq!(y.eval().unwrap(), awi!(0xc_u4));
/// }
/// ```
///
/// # Panics
///
/// This function is treated like a basic [awint::awint_dag] function that
/// panics internally if there is not an active epoch
#[track_caller]
pub fn delay<D: Into<Delay>>(bits: &mut Bits, delay: D) {
    // unwrap because of panic notice and because it should have panicked earlier in
    // the function
    let epoch = get_current_epoch().expect("cannot use `starlight::delay` without an active epoch");

    let mut tmp = awi::Awi::from_u128(delay.into().amount());
    let sig = tmp.sig();
    tmp.zero_resize(NonZeroUsize::new(sig).unwrap_or(bw(1)));
    if !tmp.is_zero() {
        // TODO same as the `DelayedLoopSource` case
        let delay_awi = dag::Awi::from(&tmp).state();
        let nzbw = bits.nzbw();
        bits.update_state(
            nzbw,
            Op::Opaque(smallvec![bits.state(), delay_awi], Some("Delay")),
        )
        .unwrap_at_runtime();

        // because `delay` still stays in a DAG, it was thought this wasn't needed, but
        // it turns out that for quiescence to calculate correctly (see the
        // `tnode_delay_opaque_quiesced` test), this needs to be done so an event knows
        // it has a `TNode` to cross

        let mut lock = epoch.epoch_data.borrow_mut();
        lock.ensemble.stator.states_to_lower.push(bits.state());
    }
}

/// Provides a way to temporally wrap around a combinatorial circuit.
///
/// Get a `&Bits` temporal value from a `Loop` via one of the traits like
/// `Deref<Target=Bits>` or `AsRef<Bits>`, then drive the `Loop` with
/// [Loop::drive]. Evaluation will find the value of the driver and use that to
/// retroactively change the temporal value of the loop.
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
/// // drive the `Loop` with itself incremented, and
/// // with a delay to prevent an instant infinite loop
/// looper.drive_with_delay(&tmp, 1).unwrap();
///
/// {
///     use awi::*;
///     for i in 0..16 {
///         // check that the evaluated value is equal to
///         // this loop iteration number
///         awi::assert_eq!(i, val.eval().unwrap().to_usize());
///         // run simulation for 1 delay step
///         epoch.run(1).unwrap();
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
        let source = Awi::new(
            w,
            Op::Opaque(smallvec![p_state], Some("UndrivenLoopSource")),
        );
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
    /// `Loop`s temporal value. There is no delay with this method, so
    /// configuration must form a DAG overall or else a nontermination error can
    /// be thrown later. Returns an error if `self.bw() != driver.bw()`.
    pub fn drive(self, driver: &Bits) -> Result<(), Error> {
        let epoch = get_current_epoch()?;
        if self.source.bw() != driver.bw() {
            Err(Error::WrongBitwidth)
        } else {
            let mut lock = epoch.epoch_data.borrow_mut();
            // add the driver to the loop source
            let op = &mut lock
                .ensemble
                .stator
                .states
                .get_mut(self.source.state())
                .unwrap()
                .op;
            if let Op::Opaque(v, name) = op {
                assert_eq!(*name, Some("UndrivenLoopSource"));
                assert_eq!(v.len(), 1);
                v.push(driver.state());
                *name = Some("LoopSource");
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

    /// Consumes `self`, looping back with the value of `driver` to change the
    /// `Loop`s temporal value in a iterative temporal evaluation. Includes a
    /// delay `delay`. Returns an error if `self.bw() != driver.bw()`.
    pub fn drive_with_delay<D: Into<Delay>>(self, driver: &Bits, delay: D) -> Result<(), Error> {
        let delay = delay.into();
        if delay.is_zero() {
            self.drive(driver)
        } else {
            let epoch = get_current_epoch()?;
            if self.source.bw() != driver.bw() {
                return Err(Error::WrongBitwidth)
            }

            // TODO perhaps just lower, but the plan is to base incremental compilation on
            // states. Not sure if we ever want dynamic delay.
            let mut tmp = awi::Awi::from_u128(delay.amount());
            let sig = tmp.sig();
            tmp.zero_resize(NonZeroUsize::new(sig).unwrap_or(bw(1)));
            let delay_awi = dag::Awi::from(&tmp).state();

            let mut lock = epoch.epoch_data.borrow_mut();
            // add the driver to the loop source
            let op = &mut lock
                .ensemble
                .stator
                .states
                .get_mut(self.source.state())
                .unwrap()
                .op;
            if let Op::Opaque(v, name) = op {
                assert_eq!(*name, Some("UndrivenLoopSource"));
                assert_eq!(v.len(), 1);
                v.push(driver.state());
                v.push(delay_awi);
                *name = Some("DelayedLoopSource");
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
            lock.ensemble
                .stator
                .states
                .get_mut(delay_awi)
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
/// one of the values of the ports to drive the temporal value.
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
