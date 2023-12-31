use starlight::{awi, dag::*, ensemble::LNodeKind, Epoch, EvalAwi, LazyAwi, StarRng};

// Test static LUT simplifications
#[test]
fn luts_optimization() {
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
