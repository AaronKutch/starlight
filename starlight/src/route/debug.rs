#![allow(clippy::large_enum_variant)]
#![allow(clippy::vec_init_then_push)]

use std::path::PathBuf;

use awint::awint_dag::{
    triple_arena::{Advancer, Arena, ChainArena, Ptr},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
};

use crate::{
    route::{channel::Referent, CEdge, CNode, Channeler},
    Error,
};

/// For viewing everything at once
#[derive(Debug, Clone)]
pub enum NodeKind<PCNode: Ptr, PCEdge: Ptr> {
    CNode(CNode<PCNode>),
    SubNode(PCNode, PCNode),
    CEdgeIncidence(PCNode, PCEdge, Option<usize>, CEdge<PCNode>, CEdge<PCNode>),
    Remove,
}

impl<PCNode: Ptr, PCEdge: Ptr> DebugNodeTrait<PCNode> for NodeKind<PCNode, PCEdge> {
    fn debug_node(_p_this: PCNode, this: &Self) -> DebugNode<PCNode> {
        match this {
            NodeKind::CNode(cnode) => DebugNode {
                sources: {
                    let mut v = vec![];
                    if let Some(p_supernode) = cnode.p_supernode {
                        v.push((p_supernode, "super".to_owned()));
                    }
                    v
                },
                center: { vec!["cnode".to_owned(), format!("{:?}", cnode.p_this_cnode)] },
                sinks: vec![],
            },
            NodeKind::SubNode(p_back, p_back_forwarded) => DebugNode {
                sources: vec![],
                center: { vec!["sub".to_owned()] },
                sinks: vec![(*p_back_forwarded, format!("{p_back:?}"))],
            },
            NodeKind::CEdgeIncidence(p_back, p_cedge, i, cedge, cedge_forwarded) => DebugNode {
                sources: {
                    let mut v = vec![(*p_back, String::new())];
                    for (source, source_forwarded) in
                        cedge.sources().iter().zip(cedge_forwarded.sources().iter())
                    {
                        v.push((*source_forwarded, format!("{source:?}")));
                    }
                    v
                },
                center: {
                    vec![
                        format!("{p_cedge:?}"),
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
            NodeKind::Remove => panic!("should have been removed"),
        }
    }
}

/// For viewing the cgraph at only one level
#[derive(Debug, Clone)]
pub enum LevelNodeKind<PCNode: Ptr> {
    CNode(CNode<PCNode>),
    CEdge(CEdge<PCNode>, CEdge<PCNode>),
    Remove,
}

impl<PCNode: Ptr> DebugNodeTrait<PCNode> for LevelNodeKind<PCNode> {
    fn debug_node(_p_this: PCNode, this: &Self) -> DebugNode<PCNode> {
        match this {
            LevelNodeKind::CNode(cnode) => DebugNode {
                sources: vec![],
                center: {
                    vec![
                        format!("{} cnode {}", cnode.lvl, cnode.internal_behavior.lut_bits),
                        format!("{:?}", cnode.p_this_cnode),
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
                        v.push((*source_forwarded, format!("{source:?}")));
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
pub enum HierarchyNodeKind<PCNode: Ptr> {
    // supernode edge and forwarded version is stored on the end
    CNode(CNode<PCNode>, Option<(PCNode, PCNode)>),
    CEdge(CEdge<PCNode>, CEdge<PCNode>),
    Remove,
}

impl<PCNode: Ptr> DebugNodeTrait<PCNode> for HierarchyNodeKind<PCNode> {
    fn debug_node(_p_this: PCNode, this: &Self) -> DebugNode<PCNode> {
        match this {
            HierarchyNodeKind::CNode(cnode, p_super) => DebugNode {
                sources: if let Some((p_super, p_super_forwarded)) = p_super {
                    vec![(*p_super_forwarded, format!("{p_super:?}"))]
                } else {
                    vec![]
                },
                center: {
                    vec![
                        format!("{} cnode {}", cnode.lvl, cnode.internal_behavior.lut_bits),
                        format!("{:?}", cnode.p_this_cnode),
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
                        v.push((*source_forwarded, format!("{source:?}")));
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

impl<PCNode: Ptr, PCEdge: Ptr> Channeler<PCNode, PCEdge> {
    pub fn to_cnode_backrefs_debug(&self) -> Arena<PCNode, NodeKind<PCNode, PCEdge>> {
        let mut arena = Arena::<PCNode, NodeKind<PCNode, PCEdge>>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                let p_cnode = self.cnodes.get_val(p_self).unwrap().clone().p_this_cnode;
                match *referent {
                    Referent::ThisCNode => {
                        NodeKind::CNode(self.cnodes.get_val(p_self).unwrap().clone())
                    }
                    Referent::SubNode(p_back) => NodeKind::SubNode(p_back, p_cnode),
                    Referent::CEdgeIncidence(p_cedge, i) => {
                        let cedge = self.cedges.get(p_cedge).unwrap().clone();
                        let mut cedge_forwarded = cedge.clone();
                        for source in cedge_forwarded.sources_mut() {
                            *source = self.cnodes.get_val(*source).unwrap().p_this_cnode;
                        }
                        if i.is_none() {
                            *cedge_forwarded.sink_mut() =
                                self.cnodes.get_val(cedge.sink()).unwrap().p_this_cnode;
                        }
                        NodeKind::CEdgeIncidence(p_cnode, p_cedge, i, cedge, cedge_forwarded)
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

    pub fn to_cnode_level_debug(&self, lvl: usize) -> Arena<PCNode, LevelNodeKind<PCNode>> {
        let mut arena = Arena::<PCNode, LevelNodeKind<PCNode>>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                match *referent {
                    Referent::ThisCNode => {
                        let cnode = self.cnodes.get_val(p_self).unwrap();
                        if cnode.lvl == u16::try_from(lvl).unwrap() {
                            LevelNodeKind::CNode(cnode.clone())
                        } else {
                            LevelNodeKind::Remove
                        }
                    }
                    Referent::SubNode(_) => LevelNodeKind::Remove,
                    Referent::CEdgeIncidence(p_cedge, i) => {
                        // insures that there is only one `CEdge` per set of incidents
                        if i.is_none() {
                            let cedge = self.cedges.get(p_cedge).unwrap().clone();
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

    pub fn to_cnode_hierarchy_debug(&self) -> Arena<PCNode, HierarchyNodeKind<PCNode>> {
        let mut arena = Arena::<PCNode, HierarchyNodeKind<PCNode>>::new();
        self.cnodes
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                match *referent {
                    Referent::ThisCNode => {
                        let cnode = self.cnodes.get_val(p_self).unwrap();
                        if let Some(p) = self.get_supernode(cnode.p_this_cnode) {
                            let p_forwarded = self.cnodes.get_val(p).unwrap().p_this_cnode;
                            HierarchyNodeKind::CNode(cnode.clone(), Some((p, p_forwarded)))
                        } else {
                            HierarchyNodeKind::CNode(cnode.clone(), None)
                        }
                    }
                    Referent::SubNode(_) => HierarchyNodeKind::Remove,
                    Referent::CEdgeIncidence(p_cedge, i) => {
                        // insures that there is only one `CEdge` per set of incidents
                        if i.is_none() {
                            let cedge = self.cedges.get(p_cedge).unwrap().clone();
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

    pub fn backrefs_to_chain_arena(&self) -> ChainArena<PCNode, Referent<PCNode, PCEdge>> {
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
