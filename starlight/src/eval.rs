use awint::awint_dag::{triple_arena::Ptr, PState, EvalError};

use crate::{PBack, TDag, Value};

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
            p_state: Option<PState>,
            found_t_node: bool,
        }
        let mut path = vec![DfsLvl {
            p_init: p_back,
            p_back,
            p_state: None,
            found_t_node: false,
        }];
        loop {
            let Some(lvl) = path.last() else { break };

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
                    if let Some(p_state) = lvl.p_state {
                        self.lower_state(p_state).unwrap();
                    }
                }
                path.pop();
            }
        }
        Ok(self.backrefs.get_val(p_back).unwrap().val)
    }
}
