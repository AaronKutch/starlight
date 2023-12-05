use starlight::{dag::*, Epoch};

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
    let epoch0 = Epoch::new();
    drop(epoch0);
    let _x: inlawi_ty!(1) = InlAwi::zero();
}

#[test]
fn epoch_nested() {
    let epoch0 = Epoch::new();
    let epoch1 = Epoch::new();
    drop(epoch1);
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
fn epoch_shared() {
    let epoch0 = Epoch::new();
    let epoch1 = Epoch::shared_with(&epoch0);
    drop(epoch1);
    drop(epoch0);
}
