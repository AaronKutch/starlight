#![allow(clippy::needless_range_loop)]

use std::num::NonZeroUsize;

use starlight::{
    awi,
    awi::*,
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        Lineage, Op,
    },
    dag,
    ensemble::LNodeKind,
    lower::meta::create_static_lut,
    utils::StarRng,
    Epoch, EvalAwi, LazyAwi,
};

// Test static LUT simplifications, this also handles input duplication cases
#[test]
fn lut_optimization_with_dup() {
    use dag::*;
    let mut rng = StarRng::new(0);
    let mut inp_bits = 0;
    for input_w in 1usize..=8 {
        let lut_w = 1 << input_w;
        for _ in 0..100 {
            let epoch = Epoch::new();
            let mut test_input = awi::Awi::zero(bw(input_w));
            rng.next_bits(&mut test_input);
            let original_input = test_input.clone();
            let input = LazyAwi::opaque(bw(input_w));
            let mut lut_input = Awi::from(input.as_ref());
            let mut opaque_set = awi::Awi::umax(bw(input_w));
            for i in 0..input_w {
                // randomly set some bits to a constant and leave some as opaque
                if rng.next_bool() {
                    lut_input.set(i, test_input.get(i).unwrap()).unwrap();
                    opaque_set.set(i, false).unwrap();
                }
            }
            for _ in 0..input_w {
                if (rng.next_u8() % 8) == 0 {
                    let inx0 = (rng.next_u8() % (input_w as awi::u8)) as awi::usize;
                    let inx1 = (rng.next_u8() % (input_w as awi::u8)) as awi::usize;
                    if opaque_set.get(inx0).unwrap() && opaque_set.get(inx1).unwrap() {
                        // randomly make some inputs duplicates from the same source
                        let tmp = lut_input.get(inx0).unwrap();
                        lut_input.set(inx1, tmp).unwrap();
                        let tmp = test_input.get(inx0).unwrap();
                        test_input.set(inx1, tmp).unwrap();
                    }
                }
            }
            let mut lut = awi::Awi::zero(bw(lut_w));
            rng.next_bits(&mut lut);
            let mut x = awi!(0);
            x.lut_(&Awi::from(&lut), &lut_input).unwrap();

            {
                use awi::{assert, assert_eq, *};

                let opt_res = EvalAwi::from(&x);

                epoch.optimize().unwrap();

                input.retro_(&original_input).unwrap();

                // check that the value is correct
                let opt_res = opt_res.eval().unwrap();
                let res = lut.get(test_input.to_usize()).unwrap();
                let res = Awi::from_bool(res);
                if opt_res != res {
                    /*
                    println!("{:0b}", &opaque_set);
                    println!("{:0b}", &test_input);
                    println!("{:0b}", &lut);
                    */
                }
                assert_eq!(opt_res, res);

                epoch.ensemble(|ensemble| {
                    // assert that there is at most one LNode with constant inputs optimized away
                    let mut lnodes = ensemble.lnodes.vals();
                    if let Some(lnode) = lnodes.next() {
                        match &lnode.kind {
                            LNodeKind::Copy(_) => {
                                inp_bits += 1;
                            }
                            LNodeKind::Lut(inp, _) => {
                                inp_bits += inp.len();
                                assert!(inp.len() <= opaque_set.count_ones());
                            }
                            LNodeKind::DynamicLut(..) => unreachable!(),
                        }
                        assert!(lnodes.next().is_none());
                    }
                    assert!(lnodes.next().is_none());
                });
            }
        }
    }
    {
        use awi::assert_eq;
        // this should only decrease from future optimizations
        assert_eq!(inp_bits, 1386);
    }
}

// these functions need to stay the same in case the ones in the library are
// changed

/// When the `i`th input to a LUT is known to be `bit`, this will reduce the LUT
fn general_reduce_lut(lut: &Awi, i: usize, bit: bool) -> Awi {
    let next_bw = lut.bw() / 2;
    let mut next_lut = Awi::zero(NonZeroUsize::new(next_bw).unwrap());
    let w = 1 << i;
    let mut from = 0;
    let mut to = 0;
    while to < next_bw {
        next_lut
            .field(to, lut, if bit { from + w } else { from }, w)
            .unwrap();
        from += 2 * w;
        to += w;
    }
    next_lut
}

/// When a LUT's output is determined to be independent of the `i`th bit, this
/// will reduce it and return true
fn general_reduce_independent_lut(lut: &mut Awi, i: usize) -> bool {
    let nzbw = lut.nzbw();
    debug_assert!(nzbw.get().is_power_of_two());
    let next_bw = nzbw.get() / 2;
    let next_nzbw = NonZeroUsize::new(next_bw).unwrap();
    let mut tmp0 = Awi::zero(next_nzbw);
    let mut tmp1 = Awi::zero(next_nzbw);
    let w = 1 << i;
    // LUT if the `i`th bit were 0
    let mut from = 0;
    let mut to = 0;
    while to < next_bw {
        tmp0.field(to, lut, from, w).unwrap();
        from += 2 * w;
        to += w;
    }
    // LUT if the `i`th bit were 1
    from = w;
    to = 0;
    while to < next_bw {
        tmp1.field(to, lut, from, w).unwrap();
        from += 2 * w;
        to += w;
    }
    if tmp0 == tmp1 {
        *lut = tmp0;
        true
    } else {
        false
    }
}

#[derive(Debug)]
enum DynamicBool {
    Bool(bool),
    Lazy(LazyAwi),
}
use DynamicBool::*;

/// Test that various types of LUT simplification work, not using duplication
/// cases
#[test]
fn lut_optimization() {
    // The first number is the base number of iterations, the others are counters to
    // make sure the rng isn't broken
    const N: (u64, u64, u64) = if cfg!(debug_assertions) {
        (16, 193536, 14778)
    } else {
        (128, 1548288, 107245)
    };
    let mut rng = StarRng::new(0);
    let mut num_lut_bits = 0u64;
    let mut num_simplified_lut_bits = 0u64;
    let mut expected_output = awi!(0);
    // LUTs with input sizes between 1 and 12 (the higher end is needed for some
    // beyond 64 width cases)
    for w in 1..=12 {
        let n = if w > 6 { N.0 } else { N.0 * 32 };
        let lut_w = NonZeroUsize::new(1 << w).unwrap();
        let w = NonZeroUsize::new(w).unwrap();
        let mut lut_input = Awi::zero(w);
        let mut known_inputs = Awi::zero(w);
        let mut lut = Awi::zero(lut_w);
        let mut pad = lut.clone();

        for _ in 0..n {
            num_lut_bits += lut.bw() as u64;
            // Some bits will be known in some way to the epoch
            rng.next_bits(&mut known_inputs);
            rng.next_bits(&mut lut_input);
            //rng.next_bits(&mut lut);
            rng.linear_fuzz_step(&mut lut, &mut pad);
            expected_output.lut_(&lut, &lut_input).unwrap();
            let mut expected_lut = lut.clone();
            let mut remaining_inp_len = w.get();
            for i in (0..w.get()).rev() {
                if known_inputs.get(i).unwrap() {
                    expected_lut = general_reduce_lut(&expected_lut, i, lut_input.get(i).unwrap());
                    remaining_inp_len -= 1;
                }
            }
            for i in (0..remaining_inp_len).rev() {
                if expected_lut.bw() == 1 {
                    break
                }
                general_reduce_independent_lut(&mut expected_lut, i);
            }
            num_simplified_lut_bits += expected_lut.bw() as u64;

            {
                let epoch = Epoch::new();
                use dag::*;
                // prepare inputs for the subtests
                let mut inputs: SmallVec<[DynamicBool; 12]> = smallvec![];
                for i in 0..w.get() {
                    if known_inputs.get(i).unwrap() {
                        inputs.push(Bool(lut_input.get(i).unwrap()))
                    } else {
                        inputs.push(Lazy(LazyAwi::opaque(bw(1))));
                    }
                }
                let mut total = Awi::zero(w);
                for (i, input) in inputs.iter().enumerate() {
                    match input {
                        Bool(b) => total.set(i, *b).unwrap(),
                        Lazy(b) => total.set(i, b.to_bool()).unwrap(),
                    }
                }
                let mut p_state_inputs = smallvec![];
                for input in inputs.iter() {
                    match input {
                        Bool(b) => p_state_inputs.push(Awi::from_bool(*b).state()),
                        Lazy(b) => p_state_inputs.push(b.try_get_p_state().unwrap()),
                    }
                }

                // for the first subtest, we make sure that the metalowerer correctly creates
                // simplified LUTs in its subroutines, we will compare this with what the
                // `LNode` optimizer path does
                let meta_res = create_static_lut(p_state_inputs, lut.clone());

                // for the second subtest create a static LUT that will be optimized in the
                // `LNode` optimization path
                let mut output = Awi::zero(bw(1));
                output.lut_(&Awi::from(&lut), &total).unwrap();
                let output = EvalAwi::from(&output);
                epoch.optimize().unwrap();

                {
                    use awi::*;
                    epoch.ensemble(|ensemble| {
                        match &meta_res {
                            Ok(op) => {
                                match op {
                                    Op::StaticLut(_, lut) => {
                                        // get the sole `LNode` that should exist by this point
                                        let mut tmp = ensemble.lnodes.vals();
                                        let lnode = tmp.next().unwrap();
                                        awi::assert!(tmp.next().is_none());
                                        match &lnode.kind {
                                            LNodeKind::Lut(_, lnode_lut) => {
                                                awi::assert_eq!(lnode_lut, lut);
                                                awi::assert_eq!(expected_lut, *lut);
                                            }
                                            _ => unreachable!(),
                                        }
                                    }
                                    Op::Literal(_) => {
                                        // there should be no `LNode` since it was optimized to a
                                        // constant
                                        let mut tmp = ensemble.lnodes.vals();
                                        awi::assert!(tmp.next().is_none());
                                        awi::assert_eq!(expected_lut.bw(), 1);
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            Err(_) => {
                                // it results in a copy of an input bit, there
                                // should be no `LNode` since any equivalence should
                                // be merged
                                let mut tmp = ensemble.lnodes.vals();
                                awi::assert!(tmp.next().is_none());
                                awi::assert_eq!(expected_lut.bw(), 2);
                            }
                        }
                    });
                }

                // set unknown inputs
                for (i, input) in inputs.iter().enumerate() {
                    if let Lazy(b) = input {
                        b.retro_bool_(lut_input.get(i).unwrap()).unwrap();
                    }
                }
                awi::assert_eq!(output.eval_bool().unwrap(), expected_output.to_bool());
                drop(epoch);
            }

            // subtest 3: make sure evaluation can handle dynamically unknown inputs in
            // several cases
            {
                let epoch = Epoch::new();
                use dag::*;
                // here, "known" will mean what bits are set to dynamically known values
                let mut total = Awi::zero(w);
                let mut inputs: SmallVec<[LazyAwi; 12]> = smallvec![];
                for i in 0..w.get() {
                    let tmp = LazyAwi::opaque(bw(1));
                    total.set(i, tmp.to_bool()).unwrap();
                    inputs.push(tmp);
                }

                let mut output = Awi::zero(bw(1));
                output.lut_(&Awi::from(&lut), &total).unwrap();
                let output = EvalAwi::from(&output);
                epoch.optimize().unwrap();

                for i in 0..w.get() {
                    if known_inputs.get(i).unwrap() {
                        inputs[i].retro_bool_(lut_input.get(i).unwrap()).unwrap();
                    }
                }
                if expected_lut.bw() == 1 {
                    // evaluation should produce a known value
                    awi::assert_eq!(output.eval_bool().unwrap(), expected_output.to_bool());
                } else {
                    // evaluation fails
                    awi::assert!(output.eval().is_err());
                }

                drop(epoch);
            }
        }
    }
    assert_eq!((num_lut_bits, num_simplified_lut_bits), (N.1, N.2));
}

/// Test dynamic LUT optimizations
#[test]
fn lut_dynamic_optimization() {
    // The first number is the base number of iterations, the others are counters to
    // make sure the rng isn't broken
    const N: (u64, u64, u64) = if cfg!(debug_assertions) {
        (32, 1984, 690)
    } else {
        (512, 31744, 9575)
    };
    let mut rng = StarRng::new(0);
    let mut num_lut_bits = 0u64;
    let mut num_simplified_lut_bits = 0u64;
    let mut expected_output = awi!(0);
    for w in 1..=5 {
        let n = N.0;
        let lut_w = NonZeroUsize::new(1 << w).unwrap();
        let w = NonZeroUsize::new(w).unwrap();
        let mut lut_input = Awi::zero(w);
        let mut known_inputs = Awi::zero(w);
        let mut lut = Awi::zero(lut_w);
        let mut known_lut_bits = Awi::zero(lut_w);
        let mut pad = lut.clone();
        let mut lut_pad = known_lut_bits.clone();

        for _ in 0..n {
            num_lut_bits += lut.bw() as u64;
            rng.next_bits(&mut known_inputs);
            rng.next_bits(&mut lut_input);
            rng.linear_fuzz_step(&mut lut, &mut pad);
            // now only some bits of the LUT might be known
            rng.linear_fuzz_step(&mut known_lut_bits, &mut lut_pad);
            let mut known_lut_bits_reduced = known_lut_bits.clone();
            expected_output.lut_(&lut, &lut_input).unwrap();
            let mut expected_lut = lut.clone();
            let mut remaining_inp_len = w.get();
            for i in (0..w.get()).rev() {
                if known_inputs.get(i).unwrap() {
                    expected_lut = general_reduce_lut(&expected_lut, i, lut_input.get(i).unwrap());
                    known_lut_bits_reduced =
                        general_reduce_lut(&known_lut_bits_reduced, i, lut_input.get(i).unwrap());
                    remaining_inp_len -= 1;
                }
            }
            if known_lut_bits_reduced.is_umax() {
                for i in (0..remaining_inp_len).rev() {
                    if expected_lut.bw() == 1 {
                        break
                    }
                    if general_reduce_independent_lut(&mut expected_lut, i) {
                        known_lut_bits_reduced = general_reduce_lut(
                            &known_lut_bits_reduced,
                            i,
                            lut_input.get(i).unwrap(),
                        );
                    }
                }
            }
            num_simplified_lut_bits += expected_lut.bw() as u64;

            {
                let epoch = Epoch::new();
                use dag::*;
                // prepare inputs for the subtests
                let mut inputs: SmallVec<[DynamicBool; 12]> = smallvec![];
                for i in 0..w.get() {
                    if known_inputs.get(i).unwrap() {
                        inputs.push(Bool(lut_input.get(i).unwrap()))
                    } else {
                        inputs.push(Lazy(LazyAwi::opaque(bw(1))));
                    }
                }
                let mut lut_bits = vec![];
                for i in 0..lut.bw() {
                    if known_lut_bits.get(i).unwrap() {
                        lut_bits.push(Bool(lut.get(i).unwrap()))
                    } else {
                        lut_bits.push(Lazy(LazyAwi::opaque(bw(1))));
                    }
                }
                let mut total = Awi::zero(w);
                for (i, input) in inputs.iter().enumerate() {
                    match input {
                        Bool(b) => total.set(i, *b).unwrap(),
                        Lazy(b) => total.set(i, b.to_bool()).unwrap(),
                    }
                }
                let mut total_lut_bits = Awi::zero(lut.nzbw());
                for (i, input) in lut_bits.iter().enumerate() {
                    match input {
                        Bool(b) => total_lut_bits.set(i, *b).unwrap(),
                        Lazy(b) => total_lut_bits.set(i, b.to_bool()).unwrap(),
                    }
                }

                let mut output = Awi::zero(bw(1));
                output.lut_(&total_lut_bits, &total).unwrap();
                let output = EvalAwi::from(&output);
                epoch.optimize().unwrap();

                {
                    epoch.ensemble(|ensemble| {
                        if known_lut_bits_reduced.bw() == 1 {
                            // there should be no `LNode` since it was optimized to a
                            // constant or forwarded
                            let mut tmp = ensemble.lnodes.vals();
                            awi::assert!(tmp.next().is_none());
                            awi::assert_eq!(expected_lut.bw(), 1);
                        } else if known_lut_bits_reduced.is_umax() {
                            if (expected_lut.bw() == 1)
                                || ((expected_lut.bw() == 2) && expected_lut.get(1).unwrap())
                            {
                                let mut tmp = ensemble.lnodes.vals();
                                awi::assert!(tmp.next().is_none());
                            } else {
                                // there should be one static LUT `LNode`
                                let mut tmp = ensemble.lnodes.vals();
                                let lnode = tmp.next().unwrap();
                                awi::assert!(tmp.next().is_none());
                                match &lnode.kind {
                                    LNodeKind::Lut(_, lnode_lut) => {
                                        awi::assert_eq!(*lnode_lut, expected_lut);
                                    }
                                    _ => unreachable!(),
                                }
                            }
                        } else {
                            // there should be one dynamic LUT `LNode`
                            let mut tmp = ensemble.lnodes.vals();
                            let lnode = tmp.next().unwrap();
                            awi::assert!(tmp.next().is_none());
                            match &lnode.kind {
                                LNodeKind::DynamicLut(_, lnode_lut) => {
                                    awi::assert_eq!(lnode_lut.len(), expected_lut.bw());
                                }
                                _ => unreachable!(),
                            }
                        }
                    });
                }

                // set unknown inputs
                for (i, input) in inputs.iter().enumerate() {
                    if let Lazy(b) = input {
                        b.retro_bool_(lut_input.get(i).unwrap()).unwrap();
                    }
                }
                for (i, input) in lut_bits.iter().enumerate() {
                    if let Lazy(b) = input {
                        b.retro_bool_(lut.get(i).unwrap()).unwrap();
                    }
                }
                awi::assert_eq!(output.eval_bool().unwrap(), expected_output.to_bool());
                epoch.verify_integrity().unwrap();
                drop(epoch);
            }

            // subtest to make sure evaluation can handle dynamically unknown inputs in
            // several cases
            {
                let epoch = Epoch::new();
                use dag::*;
                // here, "known" will mean what bits are set to dynamically known values
                let mut total = Awi::zero(w);
                let mut inputs: SmallVec<[LazyAwi; 12]> = smallvec![];
                for i in 0..w.get() {
                    let tmp = LazyAwi::opaque(bw(1));
                    total.set(i, tmp.to_bool()).unwrap();
                    inputs.push(tmp);
                }
                let mut total_lut_bits = Awi::zero(lut.nzbw());
                let mut lut_bits = vec![];
                for i in 0..lut.bw() {
                    let tmp = LazyAwi::opaque(bw(1));
                    total_lut_bits.set(i, tmp.to_bool()).unwrap();
                    lut_bits.push(tmp);
                }

                let mut output = Awi::zero(bw(1));
                output.lut_(&total_lut_bits, &total).unwrap();
                let output = EvalAwi::from(&output);
                epoch.optimize().unwrap();

                for i in 0..w.get() {
                    if known_inputs.get(i).unwrap() {
                        inputs[i].retro_bool_(lut_input.get(i).unwrap()).unwrap();
                    }
                }
                for i in 0..lut_w.get() {
                    if known_lut_bits.get(i).unwrap() {
                        lut_bits[i].retro_bool_(lut.get(i).unwrap()).unwrap();
                    }
                }
                if (expected_lut.bw() == 1) && (known_lut_bits_reduced.is_umax()) {
                    // evaluation should produce a known value
                    awi::assert_eq!(output.eval_bool().unwrap(), expected_output.to_bool());
                } else {
                    // evaluation fails
                    awi::assert!(output.eval().is_err());
                }

                drop(epoch);
            }
        }
    }
    assert_eq!((num_lut_bits, num_simplified_lut_bits), (N.1, N.2));
}
