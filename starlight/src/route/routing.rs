use std::{
    cmp::{max, Reverse},
    collections::BinaryHeap,
};

use awint::awint_dag::triple_arena::Advancer;

use crate::{
    route::{Edge, EdgeKind, EmbeddingKind, PEmbedding, Referent, Router},
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

    let max_loops = 1u64;
    for _ in 0..max_loops {
        let violations = false;

        let mut adv = router.embeddings().advancer();
        while let Some(p_embedding) = adv.advance(router.embeddings()) {
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
            let len = embedding.target_hyperpath.paths().len();
            for path_i in 0..len {
                loop {
                    let path = &router
                        .embeddings
                        .get(p_embedding)
                        .unwrap()
                        .target_hyperpath
                        .paths()[path_i];
                    let mut node_lvl = source_lvl;
                    // find a local plateau above `max_lvl`
                    let mut edge_i = None;
                    let mut edge_end_i = None;
                    for (i, edge) in path.edges().iter().copied().enumerate() {
                        match edge.kind {
                            EdgeKind::Transverse(..) => (),
                            EdgeKind::Concentrate => {
                                node_lvl = node_lvl.checked_add(1).unwrap();
                                edge_i = Some(i);
                                edge_end_i = None;
                            }
                            EdgeKind::Dilute => {
                                node_lvl = node_lvl.checked_sub(1).unwrap();
                                if edge_end_i.is_none() {
                                    edge_end_i = Some(i);
                                }
                            }
                        }
                    }
                    if let Some(edge_i) = edge_i {
                        if let Some(edge_end_i) = edge_end_i {
                            dilute_plateau(router, p_embedding, path_i, edge_i, edge_end_i)?;
                        } else {
                            // plateau does not have an end going down
                            unreachable!();
                        }
                    } else {
                        // else there is nothing else to dilute
                        break
                    }
                }
            }
        }
    }

    Ok(())
}

// subroutine to dilute a "plateau" by one level
fn dilute_plateau(
    router: &mut Router,
    p_embedding: PEmbedding,
    path_i: usize,
    edge_i: usize,
    edge_end_i: usize,
) -> Result<(), Error> {
    let embedding = router.embeddings.get(p_embedding).unwrap();
    let q_source = embedding.target_hyperpath.source();
    let path = &embedding.target_hyperpath.paths()[path_i];
    let entry = if edge_i == 0 {
        q_source
    } else {
        path.edges()[edge_i - 1].to
    };
    let exit = path.edges()[edge_end_i].to;

    // color the backbone
    let backbone_visit = router.target_channeler.next_alg_visit();
    for edge in &path.edges()[(edge_i + 1)..edge_end_i] {
        router
            .target_channeler
            .cnodes
            .get_val_mut(edge.to)
            .unwrap()
            .alg_visit = backbone_visit;
    }

    // TODO I suspect that we may want to do the routing from the source backwards,
    // because then we go through the multiple sources which could then be assigned
    // individual weights, or maybe since the ultimate constraints are the sinks
    // this is the correct way?

    // the priority queue is based around the cost of getting to an edge plus its
    // cost, because if we based around the nodes there are cases where there can be
    // multiple edge sources from a node to the same sink, and if there were an
    // incidence advancer loop inside the priority loop it would introduce issues
    // about selecting the best edge from a node

    let front_visit = router.target_channeler.next_alg_visit();
    let mut priority = BinaryHeap::new();
    // initialize entry node for algorithm
    let cnode = router.target_channeler.cnodes.get_val_mut(entry).unwrap();
    cnode.alg_visit = front_visit;
    cnode.alg_edge.0 = None;
    // push initial edges from the entry
    let mut adv = router.target_channeler.cnodes.advancer_surject(entry);
    while let Some(q_referent) = adv.advance(&router.target_channeler.cnodes) {
        if let Referent::CEdgeIncidence(q_cedge, Some(source_j)) =
            *router.target_channeler.cnodes.get_key(q_referent).unwrap()
        {
            let cedge = router.target_channeler.cedges.get(q_cedge).unwrap();
            priority.push(Reverse((
                cedge.delay_weight.get().saturating_add(cedge.lagrangian),
                q_cedge,
                source_j,
            )));
        }
    }
    let mut found = false;
    while let Some(Reverse((cost, q_cedge, source_j))) = priority.pop() {
        let cedge = router.target_channeler.cedges.get(q_cedge).unwrap();
        let q_cnode = cedge.sink();
        let cnode = router.target_channeler.cnodes.get_val_mut(q_cnode).unwrap();
        let q_cnode = cnode.p_this_cnode;
        // processing visits first and always setting them means that if
        // other searches go out of the backbone shadow, they do not need to
        // look up the supernode
        if cnode.alg_visit != front_visit {
            cnode.alg_visit = front_visit;
            // avoid reborrow, this is cheaper
            cnode.alg_edge = (Some(q_cedge), source_j);
            if q_cnode == exit {
                // found our new path
                found = true;
                break
            }
            let q_supernode = cnode.p_supernode.unwrap();
            let supernode = router.target_channeler.cnodes.get_val(q_supernode).unwrap();
            if supernode.alg_visit == backbone_visit {
                // find new edges for the Dijkstra search

                let mut adv = router.target_channeler.cnodes.advancer_surject(q_cnode);
                while let Some(q_referent1) = adv.advance(&router.target_channeler.cnodes) {
                    if let Referent::CEdgeIncidence(q_cedge1, Some(source_j1)) =
                        *router.target_channeler.cnodes.get_key(q_referent1).unwrap()
                    {
                        let cedge = router.target_channeler.cedges.get(q_cedge1).unwrap();
                        priority.push(Reverse((
                            cost.saturating_add(cedge.delay_weight.get())
                                .saturating_add(cedge.lagrangian),
                            q_cedge1,
                            source_j1,
                        )));
                    }
                }
            }
        }
    }
    if found {
        let mut new_path = vec![];
        let mut q_cnode = exit;
        loop {
            let cnode = router.target_channeler.cnodes.get_val_mut(q_cnode).unwrap();
            if let (Some(q_cedge), j) = cnode.alg_edge {
                let cedge = router.target_channeler.cedges.get(q_cedge).unwrap();
                new_path.push(Edge {
                    kind: EdgeKind::Transverse(q_cedge, j),
                    to: cnode.p_this_cnode,
                });
                q_cnode = cedge.sources()[j];
            } else {
                break
            }
        }
        // splice the new part into the old
        let edges = router
            .embeddings
            .get(p_embedding)
            .unwrap()
            .target_hyperpath
            .paths()[path_i]
            .edges();
        let mut completed_path = edges[..edge_i].to_vec();
        while let Some(edge) = new_path.pop() {
            completed_path.push(edge);
        }
        completed_path.extend(edges[(edge_end_i + 1)..].iter().copied());
        // update the path
        router
            .embeddings
            .get_mut(p_embedding)
            .unwrap()
            .target_hyperpath
            .paths_mut()[path_i]
            .edges = completed_path;
    } else {
        return Err(Error::OtherString(format!(
            "could not find possible routing (disregarding width constraints) for embedding \
             {p_embedding:?}, this is probably a bug with the router or channeler"
        )));
    }
    Ok(())
}
