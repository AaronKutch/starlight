use std::collections::VecDeque;

use awint::awint_dag::triple_arena::{ptr_struct, Advancer, OrdArena};

use crate::ensemble::{Ensemble, PBack, PExternal, PLNode, PRNode, PTNode, Referent};

ptr_struct!(PRenderNode);

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum RenderNodeKind {
    Equiv(PBack),
    LNode(PLNode),
    TNode(PTNode),
    RNode(PRNode),
}

#[derive(Debug, Clone)]
pub struct RenderNode {
    pub position: (i32, i32),
    pub fixed: bool,
    // there can be multiedges but in optimized practice there should be few
    pub incidents: Vec<RenderNodeKind>,
}

impl Ensemble {
    /// For 2D rendering. Given a starting set of `PExternal`s, this will
    /// compute the positions of nodes in a balanced web between them.
    pub fn debug_web<I: IntoIterator<Item = (PExternal, (i32, i32))>>(
        &self,
        fixed: I,
    ) -> OrdArena<PRenderNode, RenderNodeKind, RenderNode> {
        // initialize map and front with the fixed nodes
        let mut map = OrdArena::new();
        // using a `VecDeque`, pushing to the back and popping to the front produces a
        // more even front for free
        let mut front = VecDeque::new();
        for (p_external, xy) in fixed.into_iter() {
            let p_rnode = self.notary.get_rnode(p_external).unwrap().0;
            let kind = RenderNodeKind::RNode(p_rnode);
            front.push_back(kind);
            let _ = map.insert(kind, RenderNode {
                position: xy,
                fixed: true,
                incidents: vec![],
            });
        }
        // fill out the graph
        while let Some(kind) = front.pop_front() {
            // acquire all web edges
            let mut edges = vec![];
            match kind {
                RenderNodeKind::Equiv(p_back) => {
                    let mut adv = self.backrefs.advancer_surject(p_back);
                    while let Some(p_ref) = adv.advance(&self.backrefs) {
                        match self.backrefs.get_key(p_ref).unwrap() {
                            Referent::ThisEquiv => (),
                            Referent::ThisLNode(p_lnode) => {
                                edges.push(RenderNodeKind::LNode(*p_lnode))
                            }
                            Referent::ThisTNode(p_tnode) => {
                                edges.push(RenderNodeKind::TNode(*p_tnode))
                            }
                            Referent::ThisStateBit(..) => (),
                            Referent::Input(p_lnode) => edges.push(RenderNodeKind::LNode(*p_lnode)),
                            Referent::Driver(p_tnode) => {
                                edges.push(RenderNodeKind::TNode(*p_tnode))
                            }
                            Referent::ThisRNode(p_rnode) => {
                                edges.push(RenderNodeKind::RNode(*p_rnode))
                            }
                        }
                    }
                }
                RenderNodeKind::LNode(p_lnode) => {
                    let lnode = self.lnodes.get(p_lnode).unwrap();
                    edges.push(RenderNodeKind::Equiv(lnode.p_self));
                    lnode.inputs(|p| edges.push(RenderNodeKind::Equiv(p)));
                }
                RenderNodeKind::TNode(p_tnode) => {
                    let tnode = self.tnodes.get(p_tnode).unwrap();
                    edges.push(RenderNodeKind::Equiv(tnode.p_self));
                    edges.push(RenderNodeKind::Equiv(tnode.p_driver));
                }
                RenderNodeKind::RNode(p_rnode) => {
                    let rnode = self.notary.rnodes().get_val(p_rnode).unwrap();
                    if let Some(bits) = rnode.bits() {
                        for p in bits {
                            if let Some(p) = p {
                                edges.push(RenderNodeKind::Equiv(*p));
                            }
                        }
                    }
                }
            };
            // convert any `PBack`s into self equivalence `PBack`s
            for edge in &mut edges {
                if let RenderNodeKind::Equiv(p_back) = edge {
                    *p_back = self.backrefs.get_val(*p_back).unwrap().p_self_equiv;
                }
            }
            // to reduce the iterating we will need to do, the initial position will use the
            // position of the render node we are coming from
            let p = map.find_key(&kind).unwrap();
            let position = map.get_val(p).unwrap().position;
            // advance the front if the incident has not already been seen
            for incident in edges {
                if let Some(p) = map.find_key(&incident) {
                    map.get_val_mut(p).unwrap().incidents.push(kind);
                } else {
                    let _ = map.insert(incident, RenderNode {
                        position,
                        fixed: false,
                        incidents: vec![kind],
                    });
                    front.push_back(incident);
                }
            }
        }
        // iterate to get better positions, TODO more aggressive strategies, maybe use
        // geometric median
        for _ in 0..4 {
            let mut adv = map.advancer();
            while let Some(p0) = adv.advance(&map) {
                let node = map.get_val(p0).unwrap();
                if !(node.fixed || node.incidents.is_empty()) {
                    // use center of mass of incident positions
                    let mut sum = (0i64, 0i64);
                    for incident in &node.incidents {
                        let p1 = map.find_key(incident).unwrap();
                        let position = map.get_val(p1).unwrap().position;
                        sum.0 += i64::from(position.0);
                        sum.1 += i64::from(position.1);
                    }
                    let len = i64::try_from(node.incidents.len()).unwrap();
                    sum.0 /= len;
                    sum.1 /= len;
                    map.get_val_mut(p0).unwrap().position =
                        (i32::try_from(sum.0).unwrap(), i32::try_from(sum.1).unwrap());
                }
            }
        }
        map
    }
}
