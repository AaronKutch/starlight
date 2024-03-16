use std::cmp::max;

use crate::{
    route::{dilute_level, Router},
    Error,
};

pub(crate) fn route(router: &mut Router) -> Result<(), Error> {
    // see cnode.rs for the overall idea

    // property: if a program CNode is embedded in a certain target CNode, the
    // supernodes of the program CNode should be embedded somewhere in the
    // supernode chain of the target CNode including itself. Node and edge
    // embeddings should be in a ladder like ordering

    // the `initialize_embeddings` call before this function establishes all
    // neccessary initial embeddings implied by the mappings.

    // in order for a `CEdge` embedding to occur, we require it to be possible
    // within channel width constraints and available LUT bits (ignoring the sum of
    // other `CEdge`s, the lagrangians will help to drive them apart)

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

    let mut max_lvl = 0;
    for cnode in router.target_channeler().cnodes.vals() {
        max_lvl = max(max_lvl, cnode.lvl);
    }

    // on every iteration of this outer loop we reduce the maximum level of
    // hyperpaths
    loop {
        if max_lvl == 0 {
            break
        }
        max_lvl = max_lvl.checked_sub(1).unwrap();
        dilute_level(router, max_lvl)?;

        // TODO after each dilution step, then we have a separate set of
        // lagrangian adjustment routines run for a number of iterations or
        // until there are sufficiently few violations

        // things we may need to consider:

        // - something analogous to adaboost at first, but adaboost deals with
        //   probabilistic things that don't need to be exact, and we need the
        //   strict absence of violations to have a successful routing. Towards
        //   the end there will probably be a small fraction of things with
        //   violations, and will need an explicit router.

        // - Granularity will lead to situations like fitting 3 * 1/3 program
        //   edges into 2 * 1/2 target edges at the bulk level, and in general
        //   we would be forced to dilute all the way before resolving a
        //   possible routing, or in other words a valid routing may necessarily
        //   have violations at the concentrated level. I think the way to
        //   resolve this is to propogate congestion sums up the tree and only
        //   fire overall violation when the routing at the level cannot resolve
        //   the higher discovered violations. Same level violations are allowed
        //   until the base level, we use the Lagrangians to promote embedding
        //   in the right average places. We have to bias it so that we do not
        //   end up in routing hell from making things too constrained early on,
        //   but not so violation free early on that we end up unnecessarily
        //   spread out later.

        // TODO or is the above true?

        // - we may want something more sophisticated that allows the
        //   Lagrangians to work on multiple levels. Or, if there is an
        //   overdensity maybe we just reconcentrate everything, adjust weights,
        //   and retry.
    }

    // the embeddings should form a valid routing now

    Ok(())
}
