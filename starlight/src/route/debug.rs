#![allow(clippy::large_enum_variant)]
#![allow(clippy::vec_init_then_push)]

use std::path::PathBuf;

use awint::awint_dag::{
    triple_arena::{Advancer, Arena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
};

use crate::{
    route::{CEdge, CNode, Channeler, PCEdge, PCNode},
    Error,
};

/// For viewing the cgraph at only one level
#[derive(Debug, Clone)]
pub enum LevelNodeKind {
    CNode(CNode),
    CEdge(PCEdge, CEdge),
    Remove,
}

impl DebugNodeTrait<PCNode> for LevelNodeKind {
    fn debug_node(p_this: PCNode, this: &Self) -> DebugNode<PCNode> {
        match this {
            LevelNodeKind::CNode(cnode) => DebugNode {
                sources: vec![],
                center: {
                    let mut v = vec![
                        format!("{} cnode {}", cnode.lvl, cnode.internal_behavior.lut_bits),
                        format!("{:?}", p_this),
                    ];
                    if let Some(base_p_equiv) = cnode.base_p_equiv {
                        v.push(format!("{}", base_p_equiv));
                    }
                    if let Some(p_supernode) = cnode.p_supernode {
                        v.push(format!("sup: {:?}", p_supernode));
                    }
                    v
                },
                sinks: vec![],
            },
            LevelNodeKind::CEdge(p_cedge, cedge) => DebugNode {
                sources: {
                    let mut v = vec![];
                    for source in cedge.sources().iter().copied() {
                        v.push((source.p_cnode, format!("{}", source.delay_weight)));
                    }
                    v
                },
                center: {
                    let mut v = cedge.programmability().debug_strings();
                    v.push(format!("{p_cedge:?}"));
                    v
                },
                sinks: { vec![(cedge.sink(), "".to_owned())] },
            },
            LevelNodeKind::Remove => panic!("should have been removed"),
        }
    }
}

/// For viewing the hierarchy structure
#[derive(Debug, Clone)]
pub enum HierarchyNodeKind {
    CNode(CNode),
    CEdge(PCEdge, CEdge),
    Remove,
}

impl DebugNodeTrait<PCNode> for HierarchyNodeKind {
    fn debug_node(p_this: PCNode, this: &Self) -> DebugNode<PCNode> {
        match this {
            HierarchyNodeKind::CNode(cnode) => DebugNode {
                sources: if let Some(p_supernode) = cnode.p_supernode {
                    vec![(p_supernode, String::new())]
                } else {
                    vec![]
                },
                center: {
                    vec![
                        format!("{} cnode {}", cnode.lvl, cnode.internal_behavior.lut_bits),
                        format!("{:?}", p_this),
                    ]
                },
                sinks: vec![],
            },
            HierarchyNodeKind::CEdge(p_cedge, cedge) => DebugNode {
                sources: {
                    let mut v = vec![];
                    for source in cedge.sources().iter().copied() {
                        v.push((source.p_cnode, format!("{}", source.delay_weight)));
                    }
                    v
                },
                center: {
                    let mut v = cedge.programmability().debug_strings();
                    v.push(format!("{p_cedge:?}"));
                    v
                },
                sinks: { vec![(cedge.sink(), "".to_owned())] },
            },
            HierarchyNodeKind::Remove => panic!("should have been removed"),
        }
    }
}

impl Channeler {
    pub fn to_cnode_level_debug(&self, lvl: usize) -> Arena<PCNode, LevelNodeKind> {
        let mut arena = Arena::<PCNode, LevelNodeKind>::new();
        arena.clone_from_with(&self.cnodes, |_, cnode| {
            if cnode.lvl == u16::try_from(lvl).unwrap() {
                LevelNodeKind::CNode(cnode.clone())
            } else {
                LevelNodeKind::Remove
            }
        });
        for (p_cedge, cedge) in &self.cedges {
            if self.cnodes.get(cedge.sink()).unwrap().lvl == u16::try_from(lvl).unwrap() {
                arena.insert(LevelNodeKind::CEdge(p_cedge, cedge.clone()));
            }
        }
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let LevelNodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn to_cnode_hierarchy_debug(&self) -> Arena<PCNode, HierarchyNodeKind> {
        let mut arena = Arena::<PCNode, HierarchyNodeKind>::new();
        arena.clone_from_with(&self.cnodes, |_, cnode| {
            HierarchyNodeKind::CNode(cnode.clone())
        });
        for (p_cedge, cedge) in &self.cedges {
            arena.insert(HierarchyNodeKind::CEdge(p_cedge, cedge.clone()));
        }
        let mut adv = arena.advancer();
        while let Some(p) = adv.advance(&arena) {
            if let HierarchyNodeKind::Remove = arena.get(p).unwrap() {
                arena.remove(p).unwrap();
            }
        }
        arena
    }

    pub fn render_to_svgs_in_dir(&self, lvl: usize, out_dir: PathBuf) -> Result<(), Error> {
        let dir = match out_dir.canonicalize() {
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
        let mut cnode_level_file = dir.clone();
        cnode_level_file.push("cnode_level.svg");
        let mut cnode_hierarchy_file = dir;
        cnode_hierarchy_file.push("cnode_hierarchy.svg");
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_cnode_level_debug(lvl), false, cnode_level_file).unwrap();
        render_to_svg_file(
            &self.to_cnode_hierarchy_debug(),
            false,
            cnode_hierarchy_file,
        )
        .unwrap();
        res
    }
}
