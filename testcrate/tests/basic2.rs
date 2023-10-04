use starlight::{awi, dag::*, Epoch};

#[test]
fn auto() -> Option<()> {
    let epoch0 = Epoch::new();

    /*
    // starts as an opaque, but when lazy eval happens it uses zero to start out
    let mut x = AutoAwi::zero(bw(1));
    // cannot get &mut Bits from x, only &Bits which autoevals each time it is called
    let mut a = awi!(x);
    a.not_();

    let y = AutoAwi::from(&a);
    // starts epoch optimization and reevaluates
    awi::assert_eq!(y.to_extawi(), awi!(1));

    // retroactively change the value that `x` was assigned with
    x.retro_(&awi!(1)).unwrap();

    awi::assert_eq!(y.eval(), &awi!(0));
    */

    // cleans up everything not still used by `AutoAwi`s, `AutoAwi`s deregister
    // notes when dropped
    drop(epoch0);

    Some(())
}
