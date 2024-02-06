use std::cmp::max;

use crate::{route::Router, Error};

pub(crate) fn route(router: &mut Router) -> Result<(), Error> {
    // see cnode.rs for the overall idea

    // property: if a program CNode is embedded in a certain target CNode, the
    // supernodes of the program CNode should be embedded somewhere in the
    // supernode chain of the target CNode including itself. Node and edge
    // embeddings should be in a ladder like ordering

    // Embed all supernodes of the absolute embeddings in the common `CNode`, and
    // make the paths between them all

    // TODO ?

    // in order to program a target CEdge, the incidents of a base level program
    // CEdge must be compatible with their embedded incidents in the target.
    // Only then is it known to be possible to embed an edge (for bulk edges the
    // substructure might not allow it when we then try to dilute, the only thing we
    // can tell for sure is that a given embedding is not possible if the incidents
    // are not compatible).

    // If a program `CEdge` currently has all of its incidents already embedded, it
    // should be embedded now and conflicts resolved (requiring in general that
    // dilution happen until the `CEdge` and its incidents are embedded together in
    // one target `CNode`). We started with embedding some `CNode`s only because we
    // knew they were absolutely required, but after this we want to orient mainly
    // around embedding `CEdge`s, because they introduce the most constraints first
    // and should be resolved first.

    // TODO

    // Way of viewing hyperpaths: the paths can be ordered in order of which ones
    // stay on the same path the longest. The first and second path stay together
    // the longest before diverging, then the third diverges earlier, etc.
    // A straightforward optimization then is to start from any endpoint and see if
    // there is a shorter overall path to another, rebasing the divergence at that
    // point. If it was close to breakeven by some measure, then do a finding
    // triangle median like thing where different points in the triangle are
    // branched off from, then finding a center close to those.
    // With the hierarchy, we can try a new kind of hyperpath finding that is
    // perhaps based purely on finding the immediate best local routing in each
    // dilution.

    // Note: I suspect we need 4 "colors" of Lagrangian pressure in order to do a
    // constraint violation cleanup

    let mut max_lvl = 0;
    for q_cnode in router.target_channeler().top_level_cnodes.keys() {
        let cnode = router.target_channeler().cnodes.get_val(*q_cnode).unwrap();
        max_lvl = max(max_lvl, cnode.lvl);
    }

    // on every iteration of this outer loop we reduce the maximum level of
    // hyperpaths
    let mut gas = 100u64;
    loop {
        if max_lvl == 0 {
            break
        }
        gas = gas.saturating_sub(1);
        if gas == 0 {
            return Err(Error::OtherStr("ran out of gas while routing"));
        }
    }

    Ok(())
}
