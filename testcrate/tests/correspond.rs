use starlight::{awi, dag, ensemble::Corresponder, Epoch, Error, EvalAwi, In, LazyAwi, Out};

#[test]
fn correspond_clone() {
    use dag::*;
    let epoch = Epoch::new();

    let x = LazyAwi::opaque(bw(8));
    let mut y = awi!(00110110);
    y.add_(&x).unwrap();
    let y = EvalAwi::from(&y);
    let x_clone = x.try_clone().unwrap();

    {
        use awi::*;

        x_clone.retro_(&awi!(3u8)).unwrap();
        awi::assert_eq!(y.eval_u8().unwrap(), 57);
        x.retro_(&awi!(10u8)).unwrap();
        awi::assert_eq!(y.eval_u8().unwrap(), 64);
    }

    drop(epoch);
}

#[test]
fn correspond_within() {
    use dag::*;
    let epoch = Epoch::new();

    let target_x = In::<8>::opaque();
    let target_y = In::<8>::opaque();
    let mut a = awi!(0u8);
    let mut lhs = awi!(target_x);
    let mut rhs = awi!(target_y);
    lhs.neg_(true);
    rhs.neg_(true);
    a.mul_add_(&lhs, &rhs).unwrap();
    let target_z = Out::from_u8(a.to_u8());

    let program_x = LazyAwi::opaque(bw(8));
    let program_z = EvalAwi::opaque(bw(8));
    let mismatch_a = LazyAwi::opaque(bw(4));
    let mismatch_b = EvalAwi::opaque(bw(4));

    {
        use awi::*;
        let mut corresponder = Corresponder::new();
        corresponder.correspond_lazy(&program_x, &target_x).unwrap();
        corresponder.correspond_eval(&target_z, &program_z).unwrap();
        awi::assert!(matches!(
            corresponder.correspond_lazy(&mismatch_a, &program_x),
            Err(Error::BitwidthMismatch(_, _))
        ));
        awi::assert!(matches!(
            corresponder.correspond_eval(&mismatch_b, &program_z),
            Err(Error::BitwidthMismatch(_, _))
        ));

        corresponder
            .transpose_lazy(&program_x)
            .unwrap()
            .retro_(&awi!(7u8))
            .unwrap();
        target_y.retro_(&awi!(6u8)).unwrap();
        awi::assert_eq!(
            corresponder
                .transpose_eval(&program_z)
                .unwrap()
                .eval()
                .unwrap(),
            awi!(42u8)
        );
        awi::assert_eq!(target_z.eval().unwrap(), awi!(42u8));
    }
    drop(epoch);
}

#[test]
fn correspond_inbetween() {
    use dag::*;
    let target_epoch = Epoch::new();

    let target_x = In::<8>::opaque();
    let target_y = In::<8>::opaque();
    let mut a = awi!(0u8);
    let mut lhs = awi!(target_x);
    let mut rhs = awi!(target_y);
    lhs.neg_(true);
    rhs.neg_(true);
    a.mul_add_(&lhs, &rhs).unwrap();
    let target_z = Out::from_u8(a.to_u8());

    let target_epoch = target_epoch.suspend();

    let program_epoch = Epoch::new();

    let program_x = LazyAwi::opaque(bw(8));
    let program_y = LazyAwi::opaque(bw(8));
    let mut a = awi!(0u8);
    a.mul_add_(&program_x, &program_y).unwrap();
    a.neg_(true);
    let program_z = EvalAwi::from_u8(a.to_u8());

    let program_epoch = program_epoch.suspend();

    {
        use awi::*;
        let mut corresponder = Corresponder::new();
        corresponder.correspond_lazy(&program_x, &target_x).unwrap();
        corresponder.correspond_lazy(&target_y, &program_y).unwrap();
        corresponder.correspond_eval(&target_z, &program_z).unwrap();

        let target_epoch = target_epoch.resume();

        awi::assert!(matches!(
            corresponder.transpose_lazy(&target_x),
            Err(Error::CorrespondenceEmpty(_))
        ));
        awi::assert!(matches!(
            corresponder.transpose_eval(&target_z),
            Err(Error::CorrespondenceEmpty(_))
        ));

        corresponder
            .transpose_lazy(&program_x)
            .unwrap()
            .retro_(&awi!(7u8))
            .unwrap();
        corresponder
            .transpose_lazy(&program_y)
            .unwrap()
            .retro_(&awi!(6u8))
            .unwrap();
        awi::assert_eq!(
            corresponder
                .transpose_eval(&program_z)
                .unwrap()
                .eval()
                .unwrap(),
            awi!(42u8)
        );
        awi::assert_eq!(target_z.eval().unwrap(), awi!(42u8));

        let target_epoch = target_epoch.suspend();

        let program_epoch = program_epoch.resume();

        awi::assert!(matches!(
            corresponder.transpose_lazy(&program_x),
            Err(Error::CorrespondenceEmpty(_))
        ));
        awi::assert!(matches!(
            corresponder.transpose_eval(&program_z),
            Err(Error::CorrespondenceEmpty(_))
        ));

        corresponder
            .transpose_lazy(&target_x)
            .unwrap()
            .retro_(&awi!(16u8))
            .unwrap();
        corresponder
            .transpose_lazy(&target_y)
            .unwrap()
            .retro_(&awi!(8u8))
            .unwrap();
        awi::assert_eq!(
            corresponder
                .transpose_eval(&target_z)
                .unwrap()
                .eval()
                .unwrap(),
            awi!(128u8)
        );
        awi::assert_eq!(program_z.eval().unwrap(), awi!(128u8));

        let program_epoch = program_epoch.suspend();

        drop(target_epoch);
        drop(program_epoch);
    }
}
