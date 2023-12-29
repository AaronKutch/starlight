use starlight::{awi, dag::*, Epoch, EvalAwi, LazyAwi};

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
    epoch1.prune_unused_states().unwrap();
    awi::assert!(epoch1.ensemble(|ensemble| ensemble.stator.states.is_empty()));
    drop(epoch1);
}

#[test]
fn epoch_suspension0() {
    let epoch0 = Epoch::new();
    let (lazy0, eval0) = ex();
    let epoch0 = epoch0.suspend().unwrap();
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
    let epoch0 = epoch0.suspend().unwrap();
    let epoch1 = Epoch::new();
    drop(epoch0);
    drop(epoch1);
}
