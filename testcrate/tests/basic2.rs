use std::path::PathBuf;

use starlight::{awi, awint_dag::EvalError, dag::*, Epoch, EvalAwi, LazyAwi};

fn _render(epoch: &Epoch) -> awi::Result<(), EvalError> {
    epoch.render_to_svg_file(PathBuf::from("./ensemble.svg".to_owned()))
}

#[test]
fn lazy_awi() -> Option<()> {
    let epoch0 = Epoch::new();

    // starts as an opaque, but when lazy eval happens it uses zero to start out
    let mut x = LazyAwi::zero(bw(1));
    // cannot get &mut Bits from x, only &Bits which prevents the overwriting
    // problem.
    let mut a = awi!(x);
    a.not_();

    {
        use awi::*;
        // have an interfacing opaque
        let mut y = EvalAwi::from(a.as_ref());

        //y._internal_init();
        //let _ = y.eval();
        //_render(&epoch0).unwrap();
        //dbg!(epoch0.to_debug());
        // starts epoch optimization and reevaluates
        awi::assert_eq!(y.eval().unwrap(), awi!(1));

        // retroactively change the value that `x` was assigned with
        x.retro_(&awi!(1)).unwrap();

        awi::assert_eq!(y.eval().unwrap(), awi!(0));
    }

    // cleans up everything not still used by `LazyAwi`s, `LazyAwi`s deregister
    // notes when dropped
    drop(epoch0);

    Some(())
}
