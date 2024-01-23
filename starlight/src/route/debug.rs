#![allow(clippy::large_enum_variant)]
#![allow(clippy::vec_init_then_push)]

use std::path::PathBuf;

use awint::awint_dag::{
    triple_arena::{Advancer, Arena, ChainArena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
};

use crate::{
    ensemble,
    route::{channel::Referent, CEdge, CNode, Channeler, PBack, PCEdge, Programmability},
    Error,
};

/// For viewing everything at once
#[derive(Debug, Clone)]
pub enum NodeKind {
    CNode(CNode),
    SubNode(PBack, PBack),
    SuperNode(PBack, PBack),
    CEdgeIncidence(PBack, PCEdge, Option<usize>, CEdge, CEdge),
    EnsembleBackRef(PBack, ensemble::PBack),
    Remove,
}

impl DebugNodeTrait<PBack> for NodeKind {
    fn debug_node(_p_this: PBack, this: &Self) -> DebugNode<PBack> {
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
            NodeKind::CEdgeIncidence(p_back, p_cedge, i, cedge, cedge_forwarded) => DebugNode {
                sources: {
                    let mut v = vec![(*p_back, String::new())];
                    for (source, source_forwarded) in
                        cedge.sources().iter().zip(cedge_forwarded.sources().iter())
                    {
                        v.push((*source_forwarded, format!("{source}")));
                    }
                    v
                },
                center: {
                    vec![
                        format!("{p_cedge}"),
                        if let Some(source_i) = i {
                            format!("{source_i}")
                        } else {
                            "".to_owned()
                        },
                    ]
                },
                sinks: {
                    let mut v = vec![];
                    if i.is_none() {
                        v.push((cedge_forwarded.sink(), "".to_owned()));
                    }
                    v
                },
            },
            NodeKind::EnsembleBackRef(p_back, ensemble_p_back) => DebugNode {
                sources: vec![(*p_back, String::new())],
                center: { vec!["backref".to_owned(), format!("{ensemble_p_back}")] },
                sinks: vec![],
            },
            NodeKind::Remove => panic!("should have been removed"),
        }
    }
}

/// For viewing the cgraph at only one level
#[derive(Debug, Clone)]
pub enum LevelNodeKind {
    CNode(CNode),
    CEdge(CEdge, CEdge),
    Remove,
}

impl DebugNodeTrait<PBack> for LevelNodeKind {
    fn debug_node(_p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            LevelNodeKind::CNode(cnode) => DebugNode {
                sources: vec![],
                center: {
                    vec![
                        format!("{} cnode {}", cnode.lvl, cnode.internal_behavior.lut_bits),
                        format!("{}", cnode.p_this_cnode),
                    ]
                },
                sinks: vec![],
            },
            LevelNodeKind::CEdge(cedge, cedge_forwarded) => DebugNode {
                sources: {
                    let mut v = vec![];
                    for (source, source_forwarded) in
                        cedge.sources().iter().zip(cedge_forwarded.sources().iter())
                    {
                        v.push((*source_forwarded, format!("{source}")));
                    }
                    v
                },
                center: { cedge.programmability().debug_strings() },
                sinks: { vec![(cedge_forwarded.sink(), "".to_owned())] },
            },
            LevelNodeKind::Remove => panic!("should have been removed"),
        }
    }
}

/// For viewing the hierarchy structure
#[derive(Debug, Clone)]
pub enum HierarchyNodeKind {
    // supernode edge is stored on the end
    CNode(CNode, Option<PBack>),
    CEdge(CEdge, CEdge),
    Remove,
}

impl DebugNodeTrait<PBack> for HierarchyNodeKind {
    fn debug_node(_p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            HierarchyNodeKind::CNode(cnode, p_super) => DebugNode {
                sources: if let Some(p_super) = p_super {
                    vec![(*p_super, String::new())]
                } else {
                    vec![]
                },
                center: {
                    vec![
                        format!("{} cnode {}", cnode.lvl, cnode.internal_behavior.lut_bits),
                        format!("{}", cnode.p_this_cnode),
                    ]
                },
                sinks: vec![],
            },
            HierarchyNodeKind::CEdge(cedge, cedge_forwarded) => DebugNode {
                sources: {
                    let mut v = vec![];
                    for (source, source_forwarded) in
                        cedge.sources().iter().zip(cedge_forwarded.sources().iter())
                    {
                        v.push((*source_forwarded, format!("{source}")));
                    }
                    v
                },
                center: { cedge.programmability().debug_strings() },
                sinks: { vec![(cedge_forwarded.sink(), "".to_owned())] },
            },
            HierarchyNodeKind::Remove => panic!("should have been removed"),
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
                    Referent::CEdgeIncidence(p_cedge, i) => {
                        let cedge = self.cedges.get(*p_cedge).unwrap().clone();
                        let mut cedge_forwarded = cedge.clone();
                        for source in cedge_forwarded.sources_mut() {
                            *source = self.cnodes.get_val(*source).unwrap().p_this_cnode;
                        }
                        if i.is_none() {
                            *cedge_forwarded.sink_mut() =
                                self.cnodes.get_val(cedge.sink()).unwrap().p_this_cnode;
                        }
                        NodeKind::CEdgeIncidence(p_cnode, *p_cedge, *i, cedge, cedge_forwarded)
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

    pub fn to_cnode_level_debug(&self, lvl: usize) -> Arena<PBack, LevelNodeKind> {
        let mut arena = Arena::<PBack, LevelNodeKind>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                match referent {
                    Referent::ThisCNode => {
                        let cnode = self.cnodes.get_val(p_self).unwrap();
                        if cnode.lvl == u16::try_from(lvl).unwrap() {
                            LevelNodeKind::CNode(cnode.clone())
                        } else {
                            LevelNodeKind::Remove
                        }
                    }
                    Referent::SubNode(_) => LevelNodeKind::Remove,
                    Referent::SuperNode(_) => LevelNodeKind::Remove,
                    Referent::CEdgeIncidence(p_cedge, i) => {
                        // insures that there is only one `CEdge` per set of incidents
                        if i.is_none() {
                            let cedge = self.cedges.get(*p_cedge).unwrap().clone();
                            if self.cnodes.get_val(cedge.sink()).unwrap().lvl
                                == u16::try_from(lvl).unwrap()
                            {
                                let mut cedge_forwarded = cedge.clone();
                                for source in cedge_forwarded.sources_mut() {
                                    *source = self.cnodes.get_val(*source).unwrap().p_this_cnode;
                                }
                                if i.is_none() {
                                    *cedge_forwarded.sink_mut() =
                                        self.cnodes.get_val(cedge.sink()).unwrap().p_this_cnode;
                                }
                                LevelNodeKind::CEdge(cedge, cedge_forwarded)
                            } else {
                                LevelNodeKind::Remove
                            }
                        } else {
                            LevelNodeKind::Remove
                        }
                    }
                    Referent::EnsembleBackRef(_) => LevelNodeKind::Remove,
                }
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let LevelNodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn to_cnode_hierarchy_debug(&self) -> Arena<PBack, HierarchyNodeKind> {
        let mut arena = Arena::<PBack, HierarchyNodeKind>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                match referent {
                    Referent::ThisCNode => {
                        let cnode = self.cnodes.get_val(p_self).unwrap();
                        if let Some(p) = self.get_supernode(cnode.p_this_cnode) {
                            HierarchyNodeKind::CNode(cnode.clone(), Some(p))
                        } else {
                            HierarchyNodeKind::CNode(cnode.clone(), None)
                        }
                    }
                    Referent::SubNode(_) => HierarchyNodeKind::Remove,
                    Referent::SuperNode(_) => HierarchyNodeKind::Remove,
                    Referent::CEdgeIncidence(p_cedge, i) => {
                        // insures that there is only one `CEdge` per set of incidents
                        if i.is_none() {
                            let cedge = self.cedges.get(*p_cedge).unwrap().clone();
                            let mut cedge_forwarded = cedge.clone();
                            for source in cedge_forwarded.sources_mut() {
                                *source = self.cnodes.get_val(*source).unwrap().p_this_cnode;
                            }
                            if i.is_none() {
                                *cedge_forwarded.sink_mut() =
                                    self.cnodes.get_val(cedge.sink()).unwrap().p_this_cnode;
                            }
                            HierarchyNodeKind::CEdge(cedge, cedge_forwarded)
                        } else {
                            HierarchyNodeKind::Remove
                        }
                    }
                    Referent::EnsembleBackRef(_) => HierarchyNodeKind::Remove,
                }
            });
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let HierarchyNodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn render_to_svgs_in_dir(&self, lvl: usize, out_file: PathBuf) -> Result<(), Error> {
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
        let mut cnode_level_file = dir.clone();
        cnode_level_file.push("cnode_level.svg");
        let mut cnode_hierarchy_file = dir;
        cnode_hierarchy_file.push("cnode_hierarchy.svg");
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_cnode_backrefs_debug(), false, cnode_backrefs_file).unwrap();
        render_to_svg_file(&self.to_cnode_level_debug(lvl), false, cnode_level_file).unwrap();
        render_to_svg_file(
            &self.to_cnode_hierarchy_debug(),
            false,
            cnode_hierarchy_file,
        )
        .unwrap();
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
