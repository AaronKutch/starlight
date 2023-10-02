use starlight::{dag::*, Epoch};

#[test]
#[should_panic]
fn state_epoch_unregistered0() {
    let _x = ExtAwi::zero(bw(1));
}

#[test]
#[should_panic]
fn state_epoch_unregistered1() {
    let _x: u8 = 7.into();
}

#[test]
#[should_panic]
fn state_epoch_unregistered2() {
    let epoch0 = Epoch::new();
    drop(epoch0);
    let _x: inlawi_ty!(1) = InlAwi::zero();
}

#[test]
#[should_panic]
fn state_epoch_fail() {
    let epoch0 = Epoch::new();
    let epoch1 = Epoch::new();
    drop(epoch0);
    drop(epoch1);
}
