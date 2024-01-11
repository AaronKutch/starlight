#![allow(clippy::large_enum_variant)]
#![allow(clippy::vec_init_then_push)]

use std::path::PathBuf;

use awint::awint_dag::{
    triple_arena::{Advancer, Arena, ArenaTrait, ChainArena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
};

use super::{cedge, Programmability};
use crate::{
    ensemble,
    route::{channel::Referent, CEdge, CNode, Channeler, PBack, PCEdge},
    Error,
};

#[derive(Debug, Clone)]
pub enum NodeKind {
    CNode(CNode),
    SubNode(PBack, PBack),
    SuperNode(PBack, PBack),
    CEdgeIncidence(PBack, PCEdge, usize, bool, CEdge, CEdge),
    EnsembleBackRef(PBack, ensemble::PBack),
    Remove,
}

impl DebugNodeTrait<PBack> for NodeKind {
    fn debug_node(p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            NodeKind::CNode(cnode) => DebugNode {
                sources: vec![],
                center: { vec!["cnode".to_owned(), format!("{}", cnode.p_this_cnode)] },
                sinks: vec![],
            },
            NodeKind::SubNode(p_back, p_back_forwarded) => DebugNode {
                sources: vec![],
                center: { vec!["sub".to_owned()] },
                sinks: vec![(*p_back_forwarded, format!("{p_back}"))],
            },
            NodeKind::SuperNode(p_back, p_back_forwarded) => DebugNode {
                sources: vec![(*p_back_forwarded, format!("{p_back}"))],
                center: { vec!["super".to_owned()] },
                sinks: vec![],
            },
            NodeKind::CEdgeIncidence(p_back, p_cedge, i, is_sink, cedge, cedge_forwarded) => {
                DebugNode {
                    sources: {
                        let mut v = vec![(*p_back, String::new())];
                        for (i, (source, source_forwarded)) in cedge
                            .sources()
                            .iter()
                            .zip(cedge_forwarded.sources().iter())
                            .enumerate()
                        {
                            v.push((*source_forwarded, format!("{source}")));
                        }
                        v
                    },
                    center: { vec![format!("{p_cedge}"), format!("{i} {is_sink}")] },
                    sinks: {
                        let mut v = vec![];
                        for (i, (sink, sink_forwarded)) in cedge
                            .sinks()
                            .iter()
                            .zip(cedge_forwarded.sinks().iter())
                            .enumerate()
                        {
                            v.push((*sink_forwarded, format!("{sink}")));
                        }
                        v
                    },
                }
            }
            NodeKind::EnsembleBackRef(p_back, ensemble_p_back) => DebugNode {
                sources: vec![(*p_back, String::new())],
                center: { vec!["backref".to_owned(), format!("{ensemble_p_back}")] },
                sinks: vec![],
            },
            NodeKind::Remove => panic!("should have been removed"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum HyperNodeKind {
    CNode(CNode),
    CEdge(CEdge, CEdge),
    Remove,
}

impl DebugNodeTrait<PBack> for HyperNodeKind {
    fn debug_node(p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            HyperNodeKind::CNode(cnode) => DebugNode {
                sources: vec![],
                center: { vec!["cnode".to_owned(), format!("{}", cnode.p_this_cnode)] },
                sinks: vec![],
            },
            HyperNodeKind::CEdge(cedge, cedge_forwarded) => DebugNode {
                sources: {
                    let mut v = vec![];
                    for (i, (source, source_forwarded)) in cedge
                        .sources()
                        .iter()
                        .zip(cedge_forwarded.sources().iter())
                        .enumerate()
                    {
                        v.push((*source_forwarded, format!("{source}")));
                    }
                    v
                },
                center: {
                    let mut v = vec![];
                    v.push(match cedge_forwarded.programmability() {
                        Programmability::Noop => "Noop".to_owned(),
                        Programmability::StaticLut(_) => "StaticLut".to_owned(),
                        Programmability::ArbitraryLut(_) => "ArbitraryLut".to_owned(),
                        Programmability::SelectorLut(_) => "SelectorLut".to_owned(),
                        Programmability::Bulk(_) => "Bulk".to_owned(),
                    });
                    v
                },
                sinks: {
                    let mut v = vec![];
                    for (i, (sink, sink_forwarded)) in cedge
                        .sinks()
                        .iter()
                        .zip(cedge_forwarded.sinks().iter())
                        .enumerate()
                    {
                        v.push((*sink_forwarded, format!("{sink}")));
                    }
                    v
                },
            },
            HyperNodeKind::Remove => panic!("should have been removed"),
        }
    }
}

impl Channeler {
    pub fn to_cnode_backrefs_debug(&self) -> Arena<PBack, NodeKind> {
        let mut arena = Arena::<PBack, NodeKind>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                let p_cnode = self.cnodes.get_val(p_self).unwrap().clone().p_this_cnode;
                match referent {
                    Referent::ThisCNode => {
                        NodeKind::CNode(self.cnodes.get_val(p_self).unwrap().clone())
                    }
                    Referent::SubNode(p_back) => NodeKind::SubNode(*p_back, p_cnode),
                    Referent::SuperNode(p_back) => NodeKind::SuperNode(*p_back, p_cnode),
                    Referent::CEdgeIncidence(p_cedge, i, is_sink) => {
                        let mut cedge = self.cedges.get(*p_cedge).unwrap().clone();
                        let mut cedge_forwarded = cedge.clone();
                        for source in cedge_forwarded.sources_mut() {
                            *source = self.cnodes.get_val(*source).unwrap().p_this_cnode;
                        }
                        for sink in cedge_forwarded.sinks_mut() {
                            *sink = self.cnodes.get_val(*sink).unwrap().p_this_cnode;
                        }
                        NodeKind::CEdgeIncidence(
                            p_cnode,
                            *p_cedge,
                            *i,
                            *is_sink,
                            cedge,
                            cedge_forwarded,
                        )
                    }
                    Referent::EnsembleBackRef(ensemble_p_backref) => {
                        NodeKind::EnsembleBackRef(p_cnode, *ensemble_p_backref)
                    }
                }
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let NodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn to_cnode_graph_debug(&self) -> Arena<PBack, HyperNodeKind> {
        let mut arena = Arena::<PBack, HyperNodeKind>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                let p_cnode = self.cnodes.get_val(p_self).unwrap().clone().p_this_cnode;
                match referent {
                    Referent::ThisCNode => {
                        HyperNodeKind::CNode(self.cnodes.get_val(p_self).unwrap().clone())
                    }
                    Referent::SubNode(_) => HyperNodeKind::Remove,
                    Referent::SuperNode(_) => HyperNodeKind::Remove,
                    Referent::CEdgeIncidence(p_cedge, i, is_sink) => {
                        // insures that there is only one `CEdge` per set of incidents
                        if (*i == 0) && *is_sink {
                            let mut cedge = self.cedges.get(*p_cedge).unwrap().clone();
                            let mut cedge_forwarded = cedge.clone();
                            for source in cedge_forwarded.sources_mut() {
                                *source = self.cnodes.get_val(*source).unwrap().p_this_cnode;
                            }
                            for sink in cedge_forwarded.sinks_mut() {
                                *sink = self.cnodes.get_val(*sink).unwrap().p_this_cnode;
                            }
                            HyperNodeKind::CEdge(cedge, cedge_forwarded)
                        } else {
                            HyperNodeKind::Remove
                        }
                    }
                    Referent::EnsembleBackRef(ensemble_p_backref) => HyperNodeKind::Remove,
                }
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let HyperNodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn render_to_svgs_in_dir(&self, out_file: PathBuf) -> Result<(), Error> {
        let dir = match out_file.canonicalize() {
            Ok(o) => {
                if !o.is_dir() {
                    return Err(Error::OtherStr("need a directory not a file"));
                }
                o
            }
            Err(e) => {
                return Err(Error::OtherString(format!("{e:?}")));
            }
        };
        let mut cnode_backrefs_file = dir.clone();
        cnode_backrefs_file.push("cnode_backrefs.svg");
        let mut cnode_graph_file = dir;
        cnode_graph_file.push("cnode_graph.svg");
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_cnode_backrefs_debug(), false, cnode_backrefs_file).unwrap();
        render_to_svg_file(&self.to_cnode_graph_debug(), false, cnode_graph_file).unwrap();
        res
    }

    pub fn backrefs_to_chain_arena(&self) -> ChainArena<PBack, Referent> {
        let mut chain_arena = ChainArena::new();
        self.cnodes
            .clone_keys_to_chain_arena(&mut chain_arena, |_, p_lnode| *p_lnode);
        chain_arena
    }

    pub fn eprint_debug_summary(&self) {
        let chain_arena = self.backrefs_to_chain_arena();
        eprintln!("chain_arena: {:#?}", chain_arena);
    }
}
