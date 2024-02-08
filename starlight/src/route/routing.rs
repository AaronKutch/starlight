use std::{
    cmp::{max, Reverse},
    collections::BinaryHeap,
};

use awint::awint_dag::triple_arena::Advancer;

use crate::{
    route::{EdgeKind, EmbeddingKind, PEmbedding, Referent, Router},
    Error,
};

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
    loop {
        if max_lvl == 0 {
            break
        }
        max_lvl = max_lvl.checked_sub(1).unwrap();
        route_level(router, max_lvl)?;
    }

    Ok(())
}

fn route_level(router: &mut Router, max_lvl: u16) -> Result<(), Error> {
    // things we may need to consider:

    // - something analogous to adaboost at first, but adaboost deals with
    //   probabilistic things that don't need to be exact, and we need the strict
    //   absence of violations to have a successful routing. Towards the end there
    //   will probably be a small fraction of things with violations.

    // - Granularity will lead to situations like fitting 3 * 1/3 program edges into
    //   2 * 1/2 target edges at the bulk level, and in general we would be forced
    //   to dilute all the way before resolving a possible routing, or in other
    //   words a valid routing may necessarily have violations at the concentrated
    //   level. I think the way to resolve this is to propogate congestion sums up
    //   the tree and only fire overall violation when the routing at the level
    //   cannot resolve the higher discovered violations. Same level violations are
    //   allowed until the base level, we use the Lagrangians to promote embedding
    //   in the right average places. We have to bias it so that we do not end up in
    //   routing hell from making things too constrained early on, but not so
    //   violation free early on that we end up unnecessarily spread out later.

    // - Currently, `route_embedding` relies on there being

    let mut max_loops = 1u64;
    for _ in 0..max_loops {
        let mut violations = false;

        let mut adv = router.embeddings().advancer();
        while let Some(p_embedding) = adv.advance(&router.embeddings()) {
            route_embedding(router, max_lvl, p_embedding)?;
        }

        if !violations {
            break
        }
    }
    Ok(())
}

// currently assumes that hyperpath paths are like "trapezoids", one sequence of
// `Concentrate` followed by zero or more traversals, followed by a sequence
// `Dilute`s. Also assumes there is just one level of the trapezoid to dilute
fn route_embedding(
    router: &mut Router,
    max_lvl: u16,
    p_embedding: PEmbedding,
) -> Result<(), Error> {
    // as a consequence of the hierarchy generation rules, the subnodes of a node
    // are no farther than two edge traversals apart from each other

    // Current idea: color the path nodes of the top `max_lvl + 1` of the trapezoid,
    // and then do a Dijkstra search on level `max_lvl` that is constrained to only
    // search in nodes that have the colored nodes as supernodes

    let embedding = router.embeddings.get(p_embedding).unwrap();
    match embedding.program {
        EmbeddingKind::Edge(_) => todo!(),
        EmbeddingKind::Node(_) => {
            let q_source = embedding.target_hyperpath.source();
            let source = router.target_channeler().cnodes.get_val(q_source).unwrap();
            let source_lvl = source.lvl;
            assert!(source_lvl <= max_lvl);
            // TODO when doing the Steiner tree optimization generalize the two front
            // priority queue to be like a Voronoi front with speed increase for critical
            // fronts. When two cells touch each other, they should continue one while
            // recording the best intersection point, then later find the best points or
            // triple points.
            for (path_i, path) in embedding.target_hyperpath.paths().iter().enumerate() {
                let mut node_lvl = source_lvl;
                if node_lvl > max_lvl {
                    // we started above the max level
                    unreachable!()
                }
                let mut edge_i = 0usize;
                for edge in path.edges().iter().copied() {
                    match edge.kind {
                        EdgeKind::Transverse(..) => (),
                        EdgeKind::Concentrate => node_lvl = node_lvl.checked_add(1).unwrap(),
                        EdgeKind::Dilute => node_lvl = node_lvl.checked_sub(1).unwrap(),
                    }
                    if node_lvl > max_lvl {
                        break;
                    }
                    edge_i += 1;
                }

                let entry = if edge_i == 0 {
                    embedding.target_hyperpath.source()
                } else {
                    path.edges()[edge_i - 1].to
                };
                let mut exit = None;

                // color the backbone
                let backbone_visit = router.target_channeler.next_alg_visit();
                for edge in &path.edges()[edge_i..] {
                    if edge.kind == EdgeKind::Dilute {
                        exit = Some(edge.to);
                        break
                    }
                    if edge.kind == EdgeKind::Concentrate {
                        // currently relying on there only being one level of hyperpath edges above
                        // max
                        unreachable!()
                    }
                    router
                        .target_channeler
                        .cnodes
                        .get_val_mut(edge.to)
                        .unwrap()
                        .alg_visit = backbone_visit;
                }
                let exit = exit.unwrap();

                // TODO I suspect that we may want to do the routing from the source backwards,
                // because then we go through the multiple sources which could then be assigned
                // individual weights, or maybe since the ultimate constraints are the sinks
                // this is the correct way?

                let front_visit = router.target_channeler.next_alg_visit();
                let mut priority = BinaryHeap::new();
                priority.push(Reverse((0u32, entry)));
                let cnode = router.target_channeler.cnodes.get_val_mut(entry).unwrap();
                cnode.alg_visit = front_visit;
                cnode.alg_edge = (None, 0);
                loop {
                    if let Some(Reverse((cost, q))) = priority.pop() {
                        let mut adv = router.target_channeler.cnodes.advancer_surject(q);
                        while let Some(p_referent) = adv.advance(&router.target_channeler.cnodes) {
                            if let Referent::CEdgeIncidence(p_cedge, Some(j)) =
                                *router.target_channeler.cnodes.get_key(p_referent).unwrap()
                            {
                                let cedge = router.target_channeler.cedges.get(p_cedge).unwrap();

                                let q_sink = cedge.sink();

                                if q_sink == exit {
                                    // found the path
                                    //let mut new_path = vec![];
                                    //while let (Some(q_cedge), j) = cnode
                                }

                                let cnode =
                                    router.target_channeler.cnodes.get_val_mut(q_sink).unwrap();
                                // processing visits first and always setting them means that if
                                // other searches go out of the backbone shadow, they do not need to
                                // look up the supernode
                                if cnode.alg_visit != front_visit {
                                    cnode.alg_visit = front_visit;
                                    // avoid reborrow, this is cheaper
                                    cnode.alg_edge = (Some(p_cedge), j);
                                    let q_supernode = cnode.p_supernode.unwrap();
                                    let supernode = router
                                        .target_channeler
                                        .cnodes
                                        .get_val(q_supernode)
                                        .unwrap();
                                    if supernode.alg_visit == backbone_visit {
                                        // use `q_sink` as a valid Dijkstra node
                                        let next_cost = cost.saturating_add(
                                            cedge
                                                .delay_weight
                                                .get()
                                                .saturating_add(cedge.lagrangian),
                                        );
                                        priority.push(Reverse((next_cost, q_sink)));
                                    }
                                }
                            }
                        }
                    } else {
                        return Err(Error::OtherString(format!(
                            "could not find possible routing for embedding {p_embedding:?}, this \
                             is probably a bug with the router or channeler"
                        )));
                    }
                }
            }
        }
    }

    Ok(())
}
