use starlight::{
    awi,
    dag::{self, *},
    Epoch, EvalAwi, LazyAwi, StarRng,
};

#[test]
fn lazy_awi() -> Option<()> {
    let epoch0 = Epoch::new();

    let x = LazyAwi::opaque(bw(1));
    let mut a = awi!(x);
    a.not_();
    let y = EvalAwi::from(a);

    {
        use awi::*;

        // TODO the solution is to use the `bits` macro in these places
        x.retro_(&awi!(0)).unwrap();

        epoch0.ensemble().verify_integrity().unwrap();
        awi::assert_eq!(y.eval().unwrap(), awi!(1));
        epoch0.ensemble().verify_integrity().unwrap();

        x.retro_(&awi!(1)).unwrap();

        awi::assert_eq!(y.eval().unwrap(), awi!(0));
        epoch0.ensemble().verify_integrity().unwrap();
    }

    // cleans up everything not still used by `LazyAwi`s, `LazyAwi`s deregister
    // notes when dropped
    drop(epoch0);

    Some(())
}

#[test]
fn invert_twice() {
    let epoch0 = Epoch::new();
    let x = LazyAwi::opaque(bw(1));
    let mut a = awi!(x);
    a.not_();
    let a_copy = a.clone();
    a.lut_(&inlawi!(10), &a_copy).unwrap();
    a.not_();
    let y = EvalAwi::from(a);

    {
        use awi::{assert_eq, *};

        x.retro_(&awi!(0)).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(0));
        epoch0.ensemble().verify_integrity().unwrap();
        x.retro_(&awi!(1)).unwrap();
        assert_eq!(y.eval().unwrap(), awi!(1));
    }
    drop(epoch0);
}

#[test]
fn multiplier() {
    let epoch0 = Epoch::new();
    let input_a = LazyAwi::opaque(bw(16));
    let input_b = LazyAwi::opaque(bw(16));
    let mut output = inlawi!(zero: ..32);
    output.arb_umul_add_(&input_a, &input_b);
    let output = EvalAwi::from(output);

    {
        use awi::*;

        input_a.retro_(&awi!(123u16)).unwrap();
        input_b.retro_(&awi!(77u16)).unwrap();
        std::assert_eq!(output.eval().unwrap(), awi!(9471u32));

        epoch0.optimize().unwrap();

        input_a.retro_(&awi!(10u16)).unwrap();
        std::assert_eq!(output.eval().unwrap(), awi!(770u32));
    }
    drop(epoch0);
}

// test LUT simplifications
#[test]
fn luts() {
    let mut rng = StarRng::new(0);
    let mut inp_bits = 0;
    for input_w in 1usize..=8 {
        let lut_w = 1 << input_w;
        for _ in 0..100 {
            let epoch0 = Epoch::new();
            let mut test_input = awi::Awi::zero(bw(input_w));
            rng.next_bits(&mut test_input);
            let original_input = test_input.clone();
            let input = LazyAwi::opaque(bw(input_w));
            let mut lut_input = dag::Awi::from(input.as_ref());
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

                epoch0.optimize().unwrap();

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

                let ensemble = epoch0.ensemble();

                // assert that there is at most one LNode with constant inputs optimized away
                let mut lnodes = ensemble.lnodes.vals();
                if let Some(lnode) = lnodes.next() {
                    inp_bits += lnode.inp.len();
                    assert!(lnode.inp.len() <= opaque_set.count_ones());
                    assert!(lnodes.next().is_none());
                }
                assert!(lnodes.next().is_none());
            }
        }
    }
    {
        use awi::assert_eq;
        // this should only decrease from future optimizations
        assert_eq!(inp_bits, 1386);
    }
}
