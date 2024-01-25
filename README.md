# Starlight

 This is a DSL (Domain Specific Language) that can describe combinational
 logic and temporal logic. This allows RTL (Register Transfer Level)
 descriptions in ordinary Rust code with all the features that Rust provides.

 This crate still has a considerable amount of WIP stuff needed to evolve
 into a proper HDL (Hardware Description Language).

 See the documentation of `awint`/`awint_dag` which is used as the backend
 for this. `awint` is the base library that operations are modeled off of.
 `awint_dag` allows for recording a DAG of arbitrary bitwidth integer
 operations. `starlight` lowers high level operations down into a DAG of
 simple lookup tables, and also adds on temporal structs like `Loop`s. It can
 optimize, evaluate, and retroactively change values in the `DAG` for various
 purposes.

 ```rust
 use std::num::NonZeroUsize;
 use starlight::{awi, dag, Epoch, EvalAwi, LazyAwi};

 // in the scope where this is glob imported, all arbitrary width types, some primitives, and
 // the mechanisms in the macros will use mimicking types and be lazily evaluated in general.
 use dag::*;

 // This is just some arbitrary example I coded up, note that you can use
 // almost all of Rust's features that you can use on the normal types
 struct StateMachine {
     data: inlawi_ty!(16),
     counter: Awi,
 }

 impl StateMachine {
     pub fn new(w: NonZeroUsize) -> Self {
         Self {
             data: inlawi!(0u16),
             counter: Awi::zero(w),
         }
     }

     pub fn update(&mut self, input: &Bits) -> Option<()> {
         self.counter.inc_(true);

         let mut s0 = inlawi!(0u4);
         let mut s1 = inlawi!(0u4);
         let mut s2 = inlawi!(0u4);
         let mut s3 = inlawi!(0u4);
         cc!(self.data; s3, s2, s1, s0)?;
         s2.xor_(&s0)?;
         s3.xor_(&s1)?;
         s1.xor_(&s2)?;
         s0.xor_(&s3)?;
         s3.rotl_(1)?;
         s2.mux_(&input, input.get(0)?)?;
         cc!(s3, s2, s1, s0; self.data)?;
         Some(())
     }
 }

 // First, create an epoch, this will live until this struct is dropped. The
 // epoch needs to live until all mimicking operations are done and states are
 // lowered. Manually drop it with the `drop` function to avoid mistakes.
 let epoch = Epoch::new();

 let mut m = StateMachine::new(bw(4));

 // this is initially an opaque value that cannot be eagerly evaluated
 let input = LazyAwi::opaque(bw(4));

 // if we later retroactively assign this to an unequal value, the
 // `assert_assertions_strict` call will error and show the location of the
 // assertion that errored
 dag::assert_eq!(Awi::from(&input), awi!(0101));

 // step the state machine forward
 m.update(&input).unwrap();
 m.update(&awi!(0110)).unwrap();
 m.update(&awi!(0110)).unwrap();

 // use `EvalAwi`s to evaluate the resulting values
 let output_counter = EvalAwi::from(m.counter);
 let output_data = EvalAwi::from(m.data);

 {
     // switch back to normal structs
     use awi::*;

     // discard all unused mimicking states so the render is cleaner
     epoch.prune_unused_states().unwrap();

     // See the mimicking state DAG before it is lowered
     epoch
         .render_to_svgs_in_dir(std::path::PathBuf::from("./".to_owned()))
         .unwrap();

     // lower into purely static bit movements and lookup tables and optimize
     epoch.optimize().unwrap();

     // Now the combinational logic is described in a DAG of lookup tables that we
     // could use for various purposes
     epoch.ensemble(|ensemble| {
         for state in ensemble.stator.states.vals() {
             awi::assert!(state.lowered_to_lnodes);
         }
     });

     // "retroactively" assign the input with a non-opaque value
     input.retro_(&awi!(0101)).unwrap();
     // check assertions (all `dag::assert*` functions and dynamic `unwrap`s done
     // during the current `Epoch`)
     epoch.assert_assertions(true).unwrap();
     // evaluate the outputs
     awi::assert_eq!(output_counter.eval().unwrap(), awi!(0011));
     awi::assert_eq!(output_data.eval().unwrap(), awi!(0xa505_u16));

     // reassign and reevaluate
     input.retro_(&awi!(1011)).unwrap();
     awi::assert!(epoch.assert_assertions(true).is_err());
     awi::assert_eq!(output_data.eval().unwrap(), awi!(0x7b0b_u16));
 }
 drop(epoch);
 ```

 ```rust
 use starlight::{dag, awi, Epoch, EvalAwi};
 use dag::*;

 let epoch = Epoch::new();

 let mut lhs = inlawi!(zero: ..8);
 let rhs = inlawi!(umax: ..8);
 let x = inlawi!(10101010);
 let y = InlAwi::from_u64(4);

 let mut output = inlawi!(0xffu8);

 // error: expected `bool`, found struct `bool`
 //if lhs.ult(&rhs).unwrap() {
 //    output.xor_(&x).unwrap();
 //} else {
 //    output.lshr_(y.to_usize()).unwrap();
 //};

 // A little more cumbersome, but we get to use all the features of
 // normal Rust in metaprogramming and don't have to support an entire DSL.
 // In the future we will have more macros to help with this.

 let lt = lhs.ult(&rhs).unwrap();

 let mut tmp0 = output;
 tmp0.xor_(&x).unwrap();
 output.mux_(&tmp0, lt).unwrap();

 let mut tmp1 = output;
 tmp1.lshr_(y.to_usize()).unwrap();
 output.mux_(&tmp1, !lt).unwrap();

 let output_eval = EvalAwi::from(&output);

 {
     use awi::*;
     awi::assert_eq!(output_eval.eval().unwrap(), awi!(01010101));
 }
 drop(epoch);
 ```