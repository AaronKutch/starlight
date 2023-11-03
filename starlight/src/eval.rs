use awint::awint_dag::{triple_arena::Ptr, EvalError, PState};

use crate::{PBack, Referent, TDag, Value};

impl TDag {
    /// This does not update the visit number
    pub fn internal_eval_bit(&mut self, p_back: PBack) -> Result<Value, EvalError> {
        if !self.backrefs.contains(p_back) {
            return Err(EvalError::InvalidPtr)
        }
        // a level of DFS searching starts at any key in an equivalence surject.
        struct DfsLvl {
            p_init: PBack,
            p_back: PBack,
            p_state: Option<(PState, usize)>,
            found_t_node: bool,
        }
        let mut path = vec![DfsLvl {
            p_init: p_back,
            p_back,
            p_state: None,
            found_t_node: false,
        }];
        loop {
            let Some(lvl) = path.last_mut() else { break };

            // TODO
            //self.backrefs.get_val_mut(lvl.p_back).unwrap().visit

            match self.backrefs.get_key(lvl.p_back).unwrap() {
                Referent::ThisEquiv => (),
                Referent::ThisTNode(p_tnode) => {
                    lvl.found_t_node = true;
                    let tnode = self.tnodes.get(*p_tnode).unwrap();
                    path.push(DfsLvl {
                        p_init: tnode.p_self,
                        p_back: tnode.p_self,
                        p_state: None,
                        found_t_node: false,
                    });
                    continue
                }
                Referent::ThisStateBit(p_state, i) => {
                    lvl.p_state = Some((*p_state, *i));
                }
                Referent::Input(_) => (),
                Referent::LoopDriver(_) => (),
                Referent::Note(_) => (),
            }

            // if the value is set in the middle, this picks it up
            let (referent, equiv) = self.backrefs.get(lvl.p_back).unwrap();
            if equiv.val.is_known_with_visit_ge(self.visit_gen()) {
                path.pop();
                continue
            }

            let (gen, link) = self.backrefs.get_link_no_gen(p_back.inx()).unwrap();
            let p_next = PBack::_from_raw(link.next().unwrap(), gen);
            if p_next == lvl.p_init {
                // at this point, nothing has set a value but we may have a state to lower
                if !lvl.found_t_node {
                    if let Some((p_state, i)) = lvl.p_state {
                        self.lower_state(p_state).unwrap();
                        self.lower_state_to_tnodes(p_state).unwrap();
                        // reset TODO prevent infinite
                        lvl.p_back = lvl.p_init;
                        continue
                    }
                }
                path.pop();
            }
        }
        Ok(self.backrefs.get_val(p_back).unwrap().val)
    }
}
