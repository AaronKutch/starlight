use std::{
    cmp::{max, Reverse},
    collections::BinaryHeap,
    num::NonZeroU64,
};

use awint::awint_dag::triple_arena::Advancer;

use crate::{
    route::{
        Edge, EdgeKind, NodeOrEdge, PEdgeEmbed, PNodeEmbed, Programmability, QCNode, Referent,
        Router,
    },
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

/// Reduces the maximum level of hyperpaths. Currently requires that there is at
/// most one extra level above the current one
fn dilute_level(router: &mut Router, max_lvl: u16) -> Result<(), Error> {
    let max_loops = 1u64;
    for _ in 0..max_loops {
        // absolute violations correspond to things such as broken graphs or channel
        // constraints, LUT bit constraints that cannot support something even when
        // ignoring other hyperpaths
        let absolute_violations = false;

        // edge dilution must occur first
        let mut embeddings_to_process = vec![];
        let mut adv = router.edge_embeddings().advancer();
        while let Some(p_embedding) = adv.advance(router.edge_embeddings()) {
            embeddings_to_process.push(p_embedding);
            while !embeddings_to_process.is_empty() {
                dilute_edge_embedding(router, max_lvl, &mut embeddings_to_process)?;
            }
        }

        let mut embeddings_to_process = vec![];
        let mut adv = router.node_embeddings().advancer();
        while let Some(p_embedding) = adv.advance(router.node_embeddings()) {
            embeddings_to_process.push(p_embedding);
            while !embeddings_to_process.is_empty() {
                dilute_node_embedding(router, max_lvl, &mut embeddings_to_process)?;
            }
        }

        if !absolute_violations {
            break
        }
    }
    Ok(())
}

fn dilute_edge_embedding(
    router: &mut Router,
    max_lvl: u16,
    embeddings_to_process: &mut Vec<PEdgeEmbed>,
) -> Result<(), Error> {
    let p_embedding = embeddings_to_process.pop().unwrap();
    let embedding = router.edge_embeddings.get(p_embedding).unwrap();
    match embedding.target {
        NodeOrEdge::Node(_) => todo!(),
        NodeOrEdge::Edge(_) => todo!(),
    }
    let program_edge = router
        .program_channeler()
        .cedges
        .get(embedding.program_edge)
        .unwrap();
    match program_edge.programmability() {
        Programmability::StaticLut(_) => todo!(),
        Programmability::Bulk(_) => todo!(),
        Programmability::ArbitraryLut(_) | Programmability::SelectorLut(_) => unreachable!(),
    }
    Ok(())
}

fn dilute_node_embedding(
    router: &mut Router,
    max_lvl: u16,
    embeddings_to_process: &mut Vec<PNodeEmbed>,
) -> Result<(), Error> {
    let p_embedding = embeddings_to_process.pop().unwrap();
    let embedding = router.node_embeddings.get(p_embedding).unwrap();
    let hyperpath = &embedding.hyperpath;
    let program_source = hyperpath.program_source;
    let q_target_source = hyperpath.target_source;
    let target_source = router
        .target_channeler()
        .cnodes
        .get_val(q_target_source)
        .unwrap();
    let target_source_lvl = target_source.lvl;
    if target_source_lvl > (max_lvl + 1) {
        unreachable!()
    }
    let len = hyperpath.paths().len();
    for path_i in 0..len {
        // note: if we don't have single plateaus then we would have to have a retry
        // mechanism and other performance ugly stuff, we assume single plateaus

        let path = &router
            .node_embeddings()
            .get(p_embedding)
            .unwrap()
            .hyperpath
            .paths()[path_i];
        let mut node_lvl = target_source_lvl;
        // find a local plateau above `max_lvl`

        // `edge_i` and `edge_end_i` will correspond to indexes for the edges
        // immediately before and after a plateau. `loose_start` is set if the zeroeth
        // edge is already above `max_lvl`, and `loose_end` is set if the last edge is
        // above `max_lvl` and does not dilute.
        let loose_start = node_lvl > max_lvl;
        let mut edge_i = None;
        let mut edge_end_i = None;
        for (i, edge) in path.edges().iter().copied().enumerate() {
            match edge.kind {
                EdgeKind::Transverse(..) => (),
                EdgeKind::Concentrate => {
                    node_lvl = node_lvl.checked_add(1).unwrap();
                    if node_lvl > max_lvl {
                        edge_i = Some(i);
                    }
                }
                EdgeKind::Dilute => {
                    node_lvl = node_lvl.checked_sub(1).unwrap();
                    if node_lvl == max_lvl {
                        edge_end_i = Some(i);
                    }
                }
            }
            if node_lvl > (max_lvl + 1) {
                unreachable!()
            }
        }
        let loose_end = node_lvl > max_lvl;

        if program_source.is_none() {
            // the source should be on the base level, and thus we should never see a loose
            // start
            assert!(!loose_start);
        }
        if path.program_sink.is_none() {
            // the source should be on the base level, and thus we should never see a loose
            // start
            assert!(!loose_end);
        }
        if loose_start {
            // after triggering embedding we find where the last edge needs to go
            todo!()
        }
        if loose_end {
            todo!()
        }

        // a simple routing
        if let (Some(edge_i), Some(edge_end_i)) = (edge_i, edge_end_i) {
            let found = dilute_plateau(router, p_embedding, path_i, edge_i, edge_end_i)?;
            if !found {
                // for the combined source and sink embeddings, if
                // `dilute_plateau` could not
                // find the path then one is
                // not possible
                return Err(Error::OtherString(format!(
                    "could not find any possible routing (disregarding any width constraints) for \
                     embedding {p_embedding:?}, unless this is a poorly connected target, then \
                     this is probably a bug with the router"
                )));
            }
        }
    }

    Ok(())
}

// Subroutine to dilute a "plateau" by one level. `edge_i..edge_end_i` should be
// the range of edges that have `edge.to` at the plateau level (i.e., edge_i and
// edge_end_i correspond to the indexes of edges immediately before and after
// the plateau). Returns `false` if a valid path could not be found
fn dilute_plateau(
    router: &mut Router,
    p_embedding: PNodeEmbed,
    path_i: usize,
    edge_i: usize,
    edge_end_i: usize,
) -> Result<bool, Error> {
    let embedding = router.node_embeddings.get(p_embedding).unwrap();
    let target_source = embedding.hyperpath.target_source;
    let path = &embedding.hyperpath.paths()[path_i];
    let start = if edge_i == 0 {
        target_source
    } else {
        path.edges()[edge_i - 1].to
    };
    let end = path.edges()[edge_end_i].to;

    // if the node is root do not have a max level, otherwise set it to the level
    // that we will color the initial backbone with
    let cnode = router.target_channeler.cnodes.get_val(start).unwrap();
    let mut max_backbone_lvl = if cnode.p_supernode.is_some() {
        Some(cnode.lvl + 1)
    } else {
        None
    };

    // color the initial backbone which uses the concentrated path
    let backbone_visit = router.target_channeler.next_alg_visit();
    for edge in &path.edges()[edge_i..edge_end_i] {
        router
            .target_channeler
            .cnodes
            .get_val_mut(edge.to)
            .unwrap()
            .alg_visit = backbone_visit;
    }

    loop {
        let found =
            route_path_on_level(router, backbone_visit, max_backbone_lvl, start, end).unwrap();
        if found {
            break
        }
        if max_backbone_lvl.is_none() {
            return Ok(false)
        }
        // see `route_path_on_level`, we need to retry with a higher max backbone, but
        // first color the higher part of the backbone

        // TODO there is probably a way to optimize this
        max_backbone_lvl = max_backbone_lvl.map(|x| x + 1);
        let embedding = router.node_embeddings.get(p_embedding).unwrap();
        let path = &embedding.hyperpath.paths()[path_i];
        for edge in &path.edges()[edge_i..edge_end_i] {
            let mut q_supernode = router
                .target_channeler
                .cnodes
                .get_val(edge.to)
                .unwrap()
                .p_supernode;
            loop {
                if let Some(q) = q_supernode {
                    let cnode = router.target_channeler.cnodes.get_val_mut(q).unwrap();
                    if cnode.lvl == max_backbone_lvl.unwrap() {
                        cnode.alg_visit = backbone_visit;
                        break
                    }
                    q_supernode = cnode.p_supernode;
                } else {
                    // we have already reached the root
                    return Ok(false)
                }
            }
        }
    }
    // get the path which is stored on the `alg_edge`s
    let mut new_path = vec![];
    let mut q_cnode = end;
    loop {
        let cnode = router.target_channeler.cnodes.get_val_mut(q_cnode).unwrap();
        if let (Some(q_cedge), j) = cnode.alg_edge {
            let cedge = router.target_channeler.cedges.get(q_cedge).unwrap();
            new_path.push(Edge {
                kind: EdgeKind::Transverse(q_cedge, j),
                to: cnode.p_this_cnode,
            });
            q_cnode = cedge.sources()[j].p_cnode;
        } else {
            break
        }
    }
    // splice the new part into the old
    let edges = router
        .node_embeddings()
        .get(p_embedding)
        .unwrap()
        .hyperpath
        .paths()[path_i]
        .edges();
    let mut completed_path = edges[..edge_i].to_vec();
    while let Some(edge) = new_path.pop() {
        completed_path.push(edge);
    }
    completed_path.extend(edges[(edge_end_i + 1)..].iter().copied());
    // update the path
    router
        .node_embeddings
        .get_mut(p_embedding)
        .unwrap()
        .hyperpath
        .paths_mut()[path_i]
        .edges = completed_path;
    Ok(true)
}

/*
`route_path_on_level` derives its efficiency from only expanding a Dijkstra front within the
"shadow" of a certain set of supernodes, usually the path from the previous concentrated level.
This is even much better than A* in many cases if this is used all the way down the tree. If the
lower level was like

   A start
  / \
...  A
      \
       A  ...
      / \ /
    ...  B
          \
           B end

with the nodes labeled with their supernode counterparts, which in the concentrated level is

   A  ...
  / \ /
...  B

then when the concentrated path is A -> B, the A and B supernodes will be colored with
`backbone_visit` and then `route_path_on_level` will be called with that visit number, the
start on the lower level, the end on the lower level, and `max_backbone_lvl` set to the level that
the backbone nodes are on. The lower level routing will only search within the
region that has supernodes marked with `backbone_visit`, and thus they will not have to
explore the arbitrarily large "..." regions. This often produces a valid routing and also often
produces an optimized routing when considering paths outside of the region. However, there is an
important issue to consider:

In any hierarchy generation method, we will always end up in situations like:

         C
        /
       A
      / \
     /   \
end B     A start
           \
            C

Where there are two nodes ("A" here) that are concentrated to the same supernode, and a node
concentrated differently ("B" here) that will end up as

 A
 |\
 | \
 |  C
 | /
 |/
 B

on the next level since there was an edge from the "A" subnode group that has a sink in "B".
However, not all of the subnodes can actually reach B directly from A, so if the higher level path
and backbone coloring was A -> B, the lower level routing will find that it cannot reach the ending
because it needs to go through a route slightly outside of the shadow. `route_path_on_level` will
return false, but then it can be retried with a higher `max_backbone_lvl` where we
have gone through the previously used backbone and project them to a higher level. If the target has
any typical amount of cross connectivity, a path will be found within one or two higher levels.
If nothing is found by the root, then the connection is impossible if the `start` and `end` are
absolute, otherwise those need to be moved.
*/

// TODO make it so that the retry does not have to retry the entire level but
// only a part of it, probably want to keep track of the most recent common
// ancestor of all the edges to the node before the most recent or second most
// recent supernode that we have encountered (?).

// or, perhaps make a virtual valley embedding that starts from second most
// recent and ends at the next node we couldn't reach, and then restart with the
// new backbone to repair suboptimalities from the virtual embedding start and
// finish

/// Assumes that `start` and `end` are on the same level, and `max_backbone_lvl`
/// is at least one level above the leval that the `start` and `end` are on.
/// Returns `true` if the routing was successful, leaving the path information
/// on the `alg_edge`s starting at the `end` node. Returns an error if the
/// `max_backbone_lvl` is above the root node.
fn route_path_on_level(
    router: &mut Router,
    backbone_visit: NonZeroU64,
    max_backbone_lvl: Option<u16>,
    start: QCNode,
    end: QCNode,
) -> Result<bool, Error> {
    let front_visit = router.target_channeler.next_alg_visit();
    let mut priority = BinaryHeap::new();
    // initialize entry node for algorithm
    let cnode = router.target_channeler.cnodes.get_val_mut(start).unwrap();
    let route_lvl = cnode.lvl;
    cnode.alg_visit = front_visit;
    cnode.alg_edge.0 = None;
    if start == end {
        // only exit after we have set the `alg_edge`
        return Ok(true)
    }
    // push initial edges from the entry
    let mut adv = router.target_channeler.cnodes.advancer_surject(start);
    while let Some(q_referent) = adv.advance(&router.target_channeler.cnodes) {
        if let Referent::CEdgeIncidence(q_cedge, Some(source_j)) =
            *router.target_channeler.cnodes.get_key(q_referent).unwrap()
        {
            let cedge = router.target_channeler.cedges.get(q_cedge).unwrap();
            priority.push(Reverse((
                cedge.sources()[source_j]
                    .delay_weight
                    .get()
                    .saturating_add(cedge.lagrangian),
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
            if q_cnode == end {
                // found our new path
                found = true;
                break
            }
            let mut lvl = route_lvl;
            let mut q_cnode_consider = q_cnode;
            let mut use_it = false;
            if let Some(max_backbone_lvl) = max_backbone_lvl {
                while lvl <= max_backbone_lvl {
                    let cnode_consider = router
                        .target_channeler
                        .cnodes
                        .get_val(q_cnode_consider)
                        .unwrap();
                    if cnode_consider.alg_visit == backbone_visit {
                        use_it = true;
                        break
                    }
                    if let Some(q_supernode) = cnode_consider.p_supernode {
                        q_cnode_consider = q_supernode;
                        lvl += 1;
                    } else {
                        return Err(Error::OtherStr(
                            "`route_path_on_level` called with too high of a `backbone_lvl`",
                        ))
                    }
                }
            } else {
                use_it = true;
            }
            if use_it {
                // find new edges for the Dijkstra search
                let mut adv = router.target_channeler.cnodes.advancer_surject(q_cnode);
                while let Some(q_referent1) = adv.advance(&router.target_channeler.cnodes) {
                    if let Referent::CEdgeIncidence(q_cedge1, Some(source_j1)) =
                        *router.target_channeler.cnodes.get_key(q_referent1).unwrap()
                    {
                        let cedge = router.target_channeler.cedges.get(q_cedge1).unwrap();
                        priority.push(Reverse((
                            cost.saturating_add(cedge.sources()[source_j1].delay_weight.get())
                                .saturating_add(cedge.lagrangian),
                            q_cedge1,
                            source_j1,
                        )));
                    }
                }
            }
        }
    }
    Ok(found)
}
