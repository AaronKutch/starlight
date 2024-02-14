use starlight::{
    awi,
    dag::{self, *},
    Epoch, Error, EvalAwi, LazyAwi,
};

#[test]
#[should_panic]
fn epoch_unregistered0() {
    let _x = Awi::zero(bw(1));
}

#[test]
#[should_panic]
fn epoch_unregistered1() {
    let _x: u8 = 7.into();
}

#[test]
#[should_panic]
fn epoch_unregistered2() {
    let epoch = Epoch::new();
    drop(epoch);
    let _x: inlawi_ty!(1) = InlAwi::zero();
}

// generates some mimick ops and assertions
fn ex() -> (LazyAwi, EvalAwi) {
    let lazy_x = LazyAwi::opaque(bw(2));
    let mut y = awi!(01);
    y.shl_(lazy_x.to_usize()).unwrap();
    let eval_x = EvalAwi::from(&y);
    (lazy_x, eval_x)
}

#[test]
fn epoch_nested0() {
    let epoch0 = Epoch::new();
    let (lazy0, eval0) = ex();
    let epoch1 = Epoch::new();
    {
        use awi::*;
        awi::assert!(lazy0.retro_(&awi!(01)).is_err());
        awi::assert!(eval0.eval().is_err());
    }
    drop(epoch1);
    drop(epoch0);
}

#[test]
fn epoch_nested1() {
    let epoch0 = Epoch::new();
    let (lazy0, eval0) = ex();
    let epoch1 = Epoch::new();
    drop(epoch1);
    {
        use awi::*;
        lazy0.retro_(&awi!(01)).unwrap();
        awi::assert_eq!(eval0.eval().unwrap(), awi!(10));
        epoch0.assert_assertions(true).unwrap();
    }
    drop(epoch0);
}

#[test]
#[should_panic]
fn epoch_nested_fail() {
    let epoch0 = Epoch::new();
    let epoch1 = Epoch::new();
    drop(epoch0);
    drop(epoch1);
}

#[test]
fn epoch_shared0() {
    // checking assertions
    let epoch0 = Epoch::new();
    let (lazy0, eval0) = ex();
    let epoch1 = Epoch::shared_with(&epoch0);
    awi::assert_eq!(
        epoch0.ensemble(|ensemble| ensemble.notary.rnodes().len()),
        3
    );
    awi::assert_eq!(
        epoch1.ensemble(|ensemble| ensemble.notary.rnodes().len()),
        3
    );
    awi::assert_eq!(epoch0.assertions().bits.len(), 1);
    awi::assert!(epoch1.assertions().bits.is_empty());
    drop(lazy0);
    drop(eval0);
    awi::assert_eq!(
        epoch0.ensemble(|ensemble| ensemble.notary.rnodes().len()),
        1
    );
    awi::assert_eq!(
        epoch1.ensemble(|ensemble| ensemble.notary.rnodes().len()),
        1
    );
    awi::assert_eq!(epoch0.assertions().bits.len(), 1);
    drop(epoch0);
    awi::assert!(epoch1.assertions().bits.is_empty());
    awi::assert!(epoch1.ensemble(|ensemble| ensemble.notary.rnodes().is_empty()));
    epoch1.prune_unused_states().unwrap();
    awi::assert!(epoch1.ensemble(|ensemble| ensemble.stator.states.is_empty()));
    drop(epoch1);
}

#[test]
fn epoch_shared1() {
    let epoch0 = Epoch::new();
    let (lazy0, eval0) = ex();
    let epoch1 = Epoch::shared_with(&epoch0);
    {
        use awi::*;
        lazy0.retro_(&awi!(01)).unwrap();
        awi::assert_eq!(eval0.eval().unwrap(), awi!(10));
        epoch0.assert_assertions(true).unwrap();
    }
    drop(lazy0);
    drop(eval0);
    drop(epoch0);
    epoch1.assert_assertions(true).unwrap();
    epoch1.prune_unused_states().unwrap();
    awi::assert!(epoch1.ensemble(|ensemble| ensemble.notary.rnodes().is_empty()));
    awi::assert!(epoch1.assertions().bits.is_empty());
    awi::assert!(epoch1.ensemble(|ensemble| ensemble.stator.states.is_empty()));
    drop(epoch1);
}

#[test]
fn epoch_shared2() {
    let epoch0 = Epoch::new();
    let epoch1 = Epoch::shared_with(&epoch0);
    let (lazy1, eval1) = ex();
    drop(epoch0);
    epoch1.optimize().unwrap();
    drop(lazy1);
    drop(eval1);
    drop(epoch1);
}

#[test]
fn epoch_suspension0() {
    let epoch0 = Epoch::new();
    let (lazy0, eval0) = ex();
    let epoch0 = epoch0.suspend();
    let epoch1 = Epoch::new();
    {
        use awi::*;
        awi::assert!(lazy0.retro_(&awi!(01)).is_err());
        awi::assert!(eval0.eval().is_err());
    }
    let (lazy1, eval1) = ex();
    // TODO probably should create `RNode` and `PState` arenas with generations
    // starting at random offsets to help prevent collisions
    /*{
        use awi::*;
        lazy1.retro_(&awi!(01)).unwrap();
        awi::assert_eq!(eval1.eval().unwrap(), awi!(10));
        epoch1.assert_assertions(true).unwrap();
    }*/
    {
        use awi::*;
        lazy1.retro_(&awi!(01)).unwrap();
        awi::assert_eq!(eval1.eval().unwrap(), awi!(10));
        epoch1.assert_assertions(true).unwrap();
    }
    drop(epoch1);
    let epoch0 = epoch0.resume();
    {
        use awi::*;
        lazy0.retro_(&awi!(01)).unwrap();
        awi::assert_eq!(eval0.eval().unwrap(), awi!(10));
        epoch0.assert_assertions(true).unwrap();
    }
    drop(epoch0);
}

#[test]
#[should_panic]
fn epoch_suspension1() {
    let epoch0 = Epoch::new();
    let epoch0 = epoch0.suspend();
    let epoch1 = Epoch::new();
    drop(epoch0);
    drop(epoch1);
}

#[test]
fn fallible_epoch_inactive_errors() {
    let epoch = Epoch::new();
    let x = LazyAwi::opaque(bw(1));
    let b = awi!(x);
    dag::assert!(b.lsb());
    let y = EvalAwi::from(&b);
    let z0 = LazyAwi::opaque(bw(2));
    let z1 = LazyAwi::opaque(bw(2));
    let l0 = Loop::opaque(bw(1));
    let l1 = Loop::opaque(bw(1));

    // when things are from the outer epoch
    let epoch1 = Epoch::new();
    {
        use awi::{assert, assert_eq, *};
        assert!(matches!(
            x.retro_(&awi!(0)),
            Err(Error::InvalidPExternal(_))
        ));
        assert!(matches!(
            x.retro_bool_(false),
            Err(Error::InvalidPExternal(_))
        ));
        assert!(matches!(x.retro_u8_(0), Err(Error::InvalidPExternal(_))));
        assert!(matches!(y.eval(), Err(Error::InvalidPExternal(_))));
        assert!(matches!(y.eval_bool(), Err(Error::InvalidPExternal(_))));
        assert!(matches!(y.eval_u8(), Err(Error::InvalidPExternal(_))));
        assert!(matches!(
            z0.drive_with_delay(&y, 0),
            Err(Error::InvalidPExternal(_))
        ));
        // this might be an issue, but I think this should be like a normal mimick
        //assert!(matches!(.unwrap(), Err(Error::InvalidPExternal(_))));

        epoch.verify_integrity().unwrap();
        assert_eq!(
            epoch.assert_assertions(true),
            Err(Error::WrongCurrentlyActiveEpoch)
        );
        assert_eq!(
            epoch.prune_unused_states(),
            Err(Error::WrongCurrentlyActiveEpoch)
        );
        assert_eq!(epoch.lower(), Err(Error::WrongCurrentlyActiveEpoch));
        assert_eq!(
            epoch.lower_and_prune(),
            Err(Error::WrongCurrentlyActiveEpoch)
        );
        assert_eq!(epoch.optimize(), Err(Error::WrongCurrentlyActiveEpoch));
        assert_eq!(epoch.run(0), Err(Error::WrongCurrentlyActiveEpoch));
        assert_eq!(epoch.quiesced(), Err(Error::WrongCurrentlyActiveEpoch));
    }
    drop(epoch1);

    let epoch = epoch.suspend();

    // when there is no active epoch
    {
        use awi::{assert_eq, *};
        assert_eq!(x.retro_(&awi!(0)), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(x.retro_bool_(false), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(x.retro_u8_(0), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(y.eval(), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(y.eval_bool(), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(y.eval_u8(), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(
            z1.drive_with_delay(&y, 0),
            Err(Error::NoCurrentlyActiveEpoch)
        );
        assert_eq!(l0.drive(&b), Err(Error::NoCurrentlyActiveEpoch));
        assert_eq!(
            l1.drive_with_delay(&b, 0),
            Err(Error::NoCurrentlyActiveEpoch)
        );
    }
    drop(epoch);
}
