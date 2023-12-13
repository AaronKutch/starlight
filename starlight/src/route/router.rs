use crate::{
    route::{
        channel::{PCEdge, PCNode},
        CEdge, CNode, HyperPath, PHyperPath,
    },
    triple_arena::{Arena, OrdArena},
};

#[derive(Debug, Clone)]
pub struct Router {
    cnodes: Arena<PCNode, CNode>,
    cedges: OrdArena<PCEdge, CEdge, ()>,
    hyperpaths: Arena<PHyperPath, HyperPath>,
}
