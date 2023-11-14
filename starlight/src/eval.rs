use awint::awint_dag::{triple_arena::Ptr, EvalError, PState};

use crate::{PBack, Referent, TDag, Value};

struct DfsLvl {
    p_init: PBack,
    p_back: PBack,
    p_state: Option<(PState, usize)>,
    found_t_node: bool,
}

enum DfsFlow {
    LvlUp(DfsLvl),
    Next,
    LvlDown,
}

fn dfs_loop(tdag: &mut TDag, lvl: &mut DfsLvl) -> DfsFlow {
    // TODO
    if lvl.p_back == lvl.p_init {
        let visit = tdag.visit_gen();
        // on the first intrusion into this equivalence manage the visit number
        let equiv = tdag.backrefs.get_val_mut(lvl.p_back).unwrap();
        if equiv.visit == visit {
            // already visited, either from another branch or we have explored the entire surject
            return DfsFlow::LvlDown;
        } else {
            equiv.visit = visit;
        }
    }

    match tdag.backrefs.get_key(lvl.p_back).unwrap() {
        Referent::ThisEquiv => (),
        Referent::ThisTNode(p_tnode) => {
            lvl.found_t_node = true;
            let tnode = tdag.tnodes.get(*p_tnode).unwrap();
            //tnode.inp
            return DfsFlow::LvlUp(DfsLvl {
                p_init: tnode.p_self,
                p_back: tnode.p_self,
                p_state: None,
                found_t_node: false,
            });
        }
        Referent::ThisStateBit(p_state, i) => {
            lvl.p_state = Some((*p_state, *i));
        }
        Referent::Input(_) => (),
        Referent::LoopDriver(_) => (),
        Referent::Note(_) => (),
    }

    // if the value is set in the middle, this picks it up
    let (referent, equiv) = tdag.backrefs.get(lvl.p_back).unwrap();
    if equiv.val.is_known_with_visit_ge(tdag.visit_gen()) {
        return DfsFlow::LvlDown;
    }

    let (gen, link) = tdag.backrefs.get_link_no_gen(lvl.p_back.inx()).unwrap();
    let p_next = PBack::_from_raw(link.next().unwrap(), gen);
    if p_next == lvl.p_init {
        // at this point, nothing has set a value but we may have a state to lower
        if !lvl.found_t_node {
            if let Some((p_state, i)) = lvl.p_state {
                tdag.lower_state(p_state).unwrap();
                tdag.lower_state_to_tnodes(p_state).unwrap();
                // reset TODO prevent infinite
                lvl.p_back = lvl.p_init;
                //return DfsFlow::Continue;
                todo!();
            }
        }
        return DfsFlow::LvlDown;
    }
    DfsFlow::Next
}

impl TDag {
    /// This does not update the visit number
    pub fn internal_eval_bit(&mut self, p_back: PBack) -> Result<Value, EvalError> {
        if !self.backrefs.contains(p_back) {
            return Err(EvalError::InvalidPtr)
        }
        // a level of DFS searching starts at any key in an equivalence surject.
        let mut path = vec![DfsLvl {
            p_init: p_back,
            p_back,
            p_state: None,
            found_t_node: false,
        }];
        loop {
            let Some(lvl) = path.last_mut() else { break };
            match dfs_loop(self, lvl) {
                DfsFlow::LvlUp(lvl) => path.push(lvl),
                DfsFlow::Next => (),
                DfsFlow::LvlDown => {path.pop();}
            }
        }
        Ok(self.backrefs.get_val(p_back).unwrap().val)
    }
}
