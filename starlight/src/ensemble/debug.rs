use std::path::PathBuf;

use awint::{
    awint_dag::{EvalError, Op, PState},
    awint_macro_internals::triple_arena::Arena,
};

use crate::{
    ensemble::{Ensemble, Equiv, LNode, PBack, PNote, PTNode, Referent, State},
    triple_arena::{Advancer, ChainArena},
    triple_arena_render::{render_to_svg_file, DebugNode, DebugNodeTrait},
    Epoch,
};

impl DebugNodeTrait<PState> for State {
    fn debug_node(p_this: PState, this: &Self) -> DebugNode<PState> {
        DebugNode {
            sources: {
                let mut v = vec![];
                for i in 0..this.op.operands_len() {
                    if let Some(name) = this.op.operand_names().get(i) {
                        v.push((this.op.operands()[i], (*name).to_owned()));
                    } else {
                        v.push((this.op.operands()[i], "".to_owned()));
                    }
                }
                v
            },
            center: {
                let mut v = vec![format!("{:?}", p_this)];
                match this.op {
                    Op::Literal(ref lit) => {
                        v.push(format!("{}", lit));
                    }
                    Op::StaticGet(_, inx) => {
                        v.push(format!("{} get({})", this.nzbw, inx));
                    }
                    Op::StaticLut(_, ref lut) => {
                        v.push(format!("{} lut({})", this.nzbw, lut));
                    }
                    _ => {
                        v.push(format!("{} {}", this.nzbw, this.op.operation_name()));
                    }
                }
                fn short(b: bool) -> &'static str {
                    if b {
                        "t"
                    } else {
                        "f"
                    }
                }
                v.push(format!(
                    "{} {} {} {}",
                    this.rc,
                    this.extern_rc,
                    short(this.lowered_to_elementary),
                    short(this.lowered_to_lnodes)
                ));
                if let Some(ref e) = this.err {
                    let s = format!("{e}");
                    for line in s.lines() {
                        v.push(line.to_owned());
                    }
                }
                v
            },
            sinks: vec![],
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateBit {
    p_equiv: Option<PBack>,
    p_state: PState,
    i: usize,
}

#[derive(Debug, Clone)]
pub struct TNodeTmp {
    p_self: PBack,
    p_driver: PBack,
    p_tnode: PTNode,
}

#[derive(Debug, Clone)]
pub struct NoteTmp {
    p_self: PBack,
    p_equiv: PBack,
    p_note: PNote,
    i: u64,
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    StateBit(StateBit),
    LNode(LNode),
    TNode(TNodeTmp),
    Equiv(Equiv, Vec<PBack>),
    Note(NoteTmp),
    Remove,
}

impl DebugNodeTrait<PBack> for NodeKind {
    fn debug_node(p_this: PBack, this: &Self) -> DebugNode<PBack> {
        match this {
            NodeKind::StateBit(state_bit) => DebugNode {
                sources: vec![],
                center: {
                    let mut v = vec![format!("{:?}", p_this)];
                    v.push(format!("{} [{}]", state_bit.p_state, state_bit.i));
                    v
                },
                sinks: {
                    if let Some(p_equiv) = state_bit.p_equiv {
                        vec![(p_equiv, "".to_string())]
                    } else {
                        vec![]
                    }
                },
            },
            NodeKind::LNode(lnode) => DebugNode {
                sources: lnode
                    .inp
                    .iter()
                    .enumerate()
                    .map(|(i, p)| (*p, format!("{i}")))
                    .collect(),
                center: {
                    let mut v = vec![format!("{:?}", p_this)];
                    if let Some(ref lut) = lnode.lut {
                        v.push(format!("{:?} ", lut));
                    }
                    if let Some(lowered_from) = lnode.lowered_from {
                        v.push(format!("{:?}", lowered_from));
                    }
                    v
                },
                sinks: vec![],
            },
            NodeKind::TNode(tnode) => DebugNode {
                sources: vec![
                    (tnode.p_self, "self".to_owned()),
                    (tnode.p_driver, "driver".to_owned()),
                ],
                center: {
                    let mut v = vec![format!("{:?}", p_this)];
                    v.push(format!("{:?}", tnode.p_tnode));
                    v
                },
                sinks: vec![],
            },
            NodeKind::Equiv(equiv, p_lnodes) => DebugNode {
                sources: p_lnodes.iter().map(|p| (*p, String::new())).collect(),
                center: {
                    vec![
                        format!("{:?}", equiv.p_self_equiv),
                        format!("{:?}", equiv.val),
                    ]
                },
                sinks: vec![],
            },
            NodeKind::Note(note) => DebugNode {
                sources: vec![(note.p_equiv, String::new())],
                center: {
                    vec![
                        format!("{}", note.p_self),
                        format!("{} [{}]", note.p_note, note.i),
                    ]
                },
                sinks: vec![],
            },
            NodeKind::Remove => panic!("should have been removed"),
        }
    }
}

impl Ensemble {
    pub fn backrefs_to_chain_arena(&self) -> ChainArena<PBack, Referent> {
        let mut chain_arena = ChainArena::new();
        self.backrefs
            .clone_keys_to_chain_arena(&mut chain_arena, |_, p_lnode| *p_lnode);
        chain_arena
    }

    pub fn to_debug(&self) -> Arena<PBack, NodeKind> {
        let mut arena = Arena::<PBack, NodeKind>::new();
        self.backrefs
            .clone_keys_to_arena(&mut arena, |p_self, referent| {
                match referent {
                    Referent::ThisEquiv => {
                        let mut v = vec![];
                        let mut adv = self.backrefs.advancer_surject(p_self);
                        while let Some(p) = adv.advance(&self.backrefs) {
                            if let Referent::ThisLNode(_) = self.backrefs.get_key(p).unwrap() {
                                // get every LNode that is in this equivalence
                                v.push(p);
                            }
                        }
                        NodeKind::Equiv(self.backrefs.get_val(p_self).unwrap().clone(), v)
                    }
                    Referent::ThisStateBit(p_state, i) => {
                        let state = self.stator.states.get(*p_state).unwrap().clone();
                        if let Some(p_bit) = state.p_self_bits[*i] {
                            let p_equiv = self.backrefs.get_val(p_bit).unwrap().p_self_equiv;
                            NodeKind::StateBit(StateBit {
                                p_equiv: Some(p_equiv),
                                p_state: *p_state,
                                i: *i,
                            })
                        } else {
                            NodeKind::StateBit(StateBit {
                                p_equiv: None,
                                p_state: *p_state,
                                i: *i,
                            })
                        }
                    }
                    Referent::ThisLNode(p_lnode) => {
                        let mut lnode = self.lnodes.get(*p_lnode).unwrap().clone();
                        // forward to the `PBack`s of LNodes
                        for inp in &mut lnode.inp {
                            if let Referent::Input(_) = self.backrefs.get_key(*inp).unwrap() {
                                let p_input = self.backrefs.get_val(*inp).unwrap().p_self_equiv;
                                *inp = p_input;
                            }
                        }
                        NodeKind::LNode(lnode)
                    }
                    Referent::ThisTNode(p_tnode) => {
                        let tnode = self.tnodes.get(*p_tnode).unwrap();
                        // forward to the `PBack`s
                        let p_self = self.backrefs.get_val(tnode.p_self).unwrap().p_self_equiv;
                        let p_driver = self.backrefs.get_val(tnode.p_driver).unwrap().p_self_equiv;
                        NodeKind::TNode(TNodeTmp {
                            p_self,
                            p_driver,
                            p_tnode: *p_tnode,
                        })
                    }
                    Referent::Note(p_note) => {
                        let note = self.notes.get(*p_note).unwrap();
                        let mut inx = u64::MAX;
                        for (i, bit) in note.bits.iter().enumerate() {
                            if *bit == Some(p_self) {
                                inx = u64::try_from(i).unwrap();
                            }
                        }
                        let equiv = self.backrefs.get_val(p_self).unwrap();
                        NodeKind::Note(NoteTmp {
                            p_self,
                            p_equiv: equiv.p_self_equiv,
                            p_note: *p_note,
                            i: inx,
                        })
                    }
                    _ => NodeKind::Remove,
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

    pub fn render_to_svgs_in_dir(&mut self, out_file: PathBuf) -> Result<(), EvalError> {
        let dir = match out_file.canonicalize() {
            Ok(o) => {
                if !o.is_dir() {
                    return Err(EvalError::OtherStr("need a directory not a file"));
                }
                o
            }
            Err(e) => {
                return Err(EvalError::OtherString(format!("{e:?}")));
            }
        };
        let mut ensemble_file = dir.clone();
        ensemble_file.push("ensemble.svg");
        let mut state_file = dir;
        state_file.push("states.svg");
        let res = self.verify_integrity();
        render_to_svg_file(&self.to_debug(), false, ensemble_file).unwrap();
        render_to_svg_file(&self.stator.states, false, state_file).unwrap();
        res
    }
}

impl Epoch {
    pub fn eprint_debug_summary(&self) {
        let ensemble = self.ensemble();
        let chain_arena = ensemble.backrefs_to_chain_arena();
        let debug = ensemble.to_debug();
        eprintln!(
            "ensemble: {:#?}\nchain_arena: {:#?}\ndebug: {:#?}",
            ensemble, chain_arena, debug
        );
    }

    pub fn render_to_svgs_in_dir(&self, out_file: PathBuf) -> Result<(), EvalError> {
        self.ensemble().render_to_svgs_in_dir(out_file)
    }
}
