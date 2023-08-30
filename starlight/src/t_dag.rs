use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{smallvec::smallvec, EvalError, OpDag, PNote},
    awint_macro_internals::triple_arena::Advancer,
    Bits, ExtAwi,
};

use crate::{
    triple_arena::{Arena, SurjectArena},
    PTNode, TNode,
};

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<PTNode>,
}

/// A DAG made primarily of lookup tables
#[derive(Debug, Clone)]
pub struct TDag {
    pub a: SurjectArena<PTNode, PTNode, TNode>,
    /// A kind of generation counter tracking the highest `visit` number
    visit_gen: NonZeroU64,
    pub notes: Arena<PNote, Note>,
    /// temporary used in evaluations
    front: Vec<PTNode>,
}

impl TDag {
    pub fn new() -> Self {
        Self {
            a: SurjectArena::new(),
            visit_gen: NonZeroU64::new(2).unwrap(),
            notes: Arena::new(),
            front: vec![],
        }
    }

    pub fn next_visit_gen(&mut self) -> NonZeroU64 {
        self.visit_gen = NonZeroU64::new(self.visit_gen.get().checked_add(1).unwrap()).unwrap();
        self.visit_gen
    }

    // TODO use "permanence" for more static-like ideas, use "noted" or "stable"?

    // but how to handle notes
    /*pub fn from_epoch(epoch: &StateEpoch) -> (Self, Result<(), EvalError>) {
        let (mut op_dag, res) = OpDag::from_epoch(epoch);
        if res.is_err() {
            return (Self::new(), res);
        }
        op_dag.lower_all()?;
        Self::from_op_dag(&mut op_dag)
    }*/

    /// Constructs a directed acyclic graph of lookup tables from an
    /// [awint::awint_dag::OpDag]. `op_dag` is taken by mutable reference only
    /// for the purposes of visitation updates.
    ///
    /// If an error occurs, the DAG (which may be in an unfinished or completely
    /// broken state) is still returned along with the error enum, so that debug
    /// tools like `render_to_svg_file` can be used.
    pub fn from_op_dag(op_dag: &mut OpDag) -> (Self, Result<(), EvalError>) {
        let mut res = Self::new();
        let err = res.add_op_dag(op_dag);
        (res, err)
    }

    pub fn verify_integrity(&self) -> Result<(), EvalError> {
        // return errors in order of most likely to be root cause
        for node in self.a.vals() {
            if let Some(p_backref) = self.a.get_key(node.p_self) {
                if node.p_self != *p_backref {
                    return Err(EvalError::OtherStr("`p_self` backref is broken"))
                }
            } else {
                return Err(EvalError::OtherStr("broken `p_self`"))
            }
            for p_input in &node.inp {
                if let Some(p_backref) = self.a.get_key(*p_input) {
                    if *p_backref != node.p_self {
                        return Err(EvalError::OtherStr("input backref does not agree"))
                    }
                } else {
                    return Err(EvalError::OtherStr("broken input `PTNode`"))
                }
            }
            if let Some(loop_driver) = node.loop_driver {
                if let Some(p_backref) = self.a.get_key(loop_driver) {
                    if node.p_self != *p_backref {
                        return Err(EvalError::OtherStr("loop driver backref does not agree"))
                    }
                } else {
                    return Err(EvalError::OtherStr(
                        "broken input `PTNode` of `loop_driver`",
                    ))
                }
            }
        }
        for node in self.a.vals() {
            if let Some(ref lut) = node.lut {
                if node.inp.is_empty() {
                    return Err(EvalError::OtherStr("no inputs for lookup table"))
                }
                if !lut.bw().is_power_of_two() {
                    return Err(EvalError::OtherStr(
                        "lookup table is not a power of two in bitwidth",
                    ))
                }
                if (lut.bw().trailing_zeros() as usize) != node.inp.len() {
                    return Err(EvalError::OtherStr(
                        "number of inputs does not correspond to lookup table size",
                    ))
                }
            } else if node.inp.len() > 1 {
                return Err(EvalError::OtherStr(
                    "`TNode` with no lookup table has more than one input",
                ))
            }
        }
        for note in self.notes.vals() {
            for p_bit in &note.bits {
                if let Some(p_backref) = self.a.get_key(*p_bit) {
                    if p_bit != p_backref {
                        return Err(EvalError::OtherStr("note backref does not agree"))
                    }
                } else {
                    return Err(EvalError::OtherStr("broken note `PTNode`"))
                }
            }
        }
        Ok(())
    }

    /// Inserts a `TNode` with `lit` value and returns a `PTNode` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PTNode {
        self.a.insert_with(|p| {
            let mut tnode = TNode::new(p);
            tnode.val = lit;
            (p, tnode)
        })
    }

    /// Makes a single bit copying `TNode` that uses `copy` and returns a
    /// `PTNode` to it. Returns `None` if `p_copy` is invalid.
    pub fn make_copy(&mut self, p_copy: PTNode) -> Option<PTNode> {
        if !self.a.contains(p_copy) {
            return None
        }
        // inserts a surject with a self referential key and the tnode value
        let p_new_tnode = self.a.insert_with(|p| {
            let tnode = TNode::new(p);
            (p, tnode)
        });
        // inserts a backreference to the surject of the copied node
        let p_backref = self.a.insert_key(p_copy, p_new_tnode).unwrap();
        // use the backreference key as the input to the tnode
        self.a.get_val_mut(p_new_tnode).unwrap().inp = smallvec![p_backref];
        Some(p_new_tnode)
    }

    /// Makes a single output bit lookup table `TNode` and returns a `PTNode` to
    /// it. Returns `None` if the table length is incorrect or any of the
    /// `p_inxs` are invalid.
    pub fn make_lut(&mut self, p_inxs: &[PTNode], table: &Bits) -> Option<PTNode> {
        let num_entries = 1 << p_inxs.len();
        if table.bw() != num_entries {
            return None
        }
        for p_inx in p_inxs {
            if !self.a.contains(*p_inx) {
                return None
            }
        }
        let p_new_tnode = self.a.insert_with(|p| {
            let mut tnode = TNode::new(p);
            tnode.lut = Some(ExtAwi::from(table));
            (p, tnode)
        });
        for p_inx in p_inxs {
            let p_backref = self.a.insert_key(*p_inx, p_new_tnode).unwrap();
            self.a.get_val_mut(p_new_tnode).unwrap().inp.push(p_backref);
        }
        Some(p_new_tnode)
    }

    /// Sets up a loop from the loop source `p_looper` and driver `p_driver`
    pub fn make_loop(&mut self, p_looper: PTNode, p_driver: PTNode) -> Option<()> {
        if !self.a.contains(p_looper) {
            return None
        }
        if !self.a.contains(p_driver) {
            return None
        }
        let p_backref = self.a.insert_key(p_driver, p_looper).unwrap();

        let looper = self.a.get_val_mut(p_looper).unwrap();
        looper.loop_driver = Some(p_backref);
        Some(())
    }

    /// Sets up an extra reference to `p_refer`
    pub fn make_extra_reference(&mut self, p_refer: PTNode) -> Option<PTNode> {
        self.a.insert_key_with(p_refer, |p| p)
    }

    // TODO need multiple variations of `eval`, one that assumes `lut` structure is
    // not changed and avoids propogation if equal values are detected.

    /// Evaluates `self` as much as possible. Uses only root `Some` bit values
    /// in propogation.
    pub fn eval(&mut self) {
        let this_visit = self.next_visit_gen();

        // set `alg_rc` and get the initial front
        self.front.clear();
        let mut adv = self.a.advancer();
        while let Some(p) = adv.advance(&self.a) {
            let key = *self.a.get_key(p).unwrap();
            let node = self.a.get_val_mut(p).unwrap();
            if key == node.p_self {
                let len = node.inp.len();
                node.alg_rc = u64::try_from(len).unwrap();
                if (len == 0) && node.val.is_some() {
                    self.front.push(p);
                }
            }
        }

        while let Some(p_node) = self.front.pop() {
            let node = self.a.get_val(p_node).unwrap();
            let (val, propogate) = if node.lut.is_some() {
                // acquire LUT input
                let mut inx = 0;
                let len = node.inp.len();
                for i in 0..len {
                    inx |= (self.a.get_val(node.inp[i]).unwrap().val.unwrap() as usize) << i;
                }
                // evaluate
                let val = node.lut.as_ref().unwrap().get(inx).unwrap();
                (Some(val), true)
            } else if node.inp.len() == 1 {
                // wire propogation
                let val = self.a.get_val(node.inp[0]).unwrap().val;
                (val, true)
            } else {
                (None, false)
            };
            let node = self.a.get_val_mut(p_node).unwrap();
            if propogate {
                node.val = val;
            }
            node.visit = this_visit;
            // propogate
            let mut adv = self.a.advancer_surject(p_node);
            while let Some(p_backref) = adv.advance(&self.a) {
                let p_next = *self.a.get_key(p_backref).unwrap();
                let next = self.a.get_val_mut(p_next).unwrap();
                if (next.visit < this_visit) && (next.alg_rc != 0) {
                    next.alg_rc -= 1;
                    if next.alg_rc == 0 {
                        self.front.push(next.p_self);
                    }
                }
            }
        }
    }

    pub fn drive_loops(&mut self) {
        let mut adv = self.a.advancer();
        while let Some(p) = adv.advance(&self.a) {
            if let Some(driver) = self.a.get_val(p).unwrap().loop_driver {
                self.a.get_val_mut(p).unwrap().val = self.a.get_val(driver).unwrap().val;
            }
        }
    }

    pub fn get_noted_as_extawi(&self, p_note: PNote) -> Option<ExtAwi> {
        let note = self.notes.get(p_note)?;
        let mut x = ExtAwi::zero(NonZeroUsize::new(note.bits.len())?);
        for (i, p_bit) in note.bits.iter().enumerate() {
            let bit = self.a.get_val(*p_bit)?;
            let val = bit.val?;
            x.set(i, val).unwrap();
        }
        Some(x)
    }

    #[track_caller]
    pub fn set_noted(&mut self, p_note: PNote, val: &Bits) {
        let note = &self.notes[p_note];
        assert_eq!(note.bits.len(), val.bw());
        for (i, bit) in note.bits.iter().enumerate() {
            self.a.get_val_mut(*bit).unwrap().val = Some(val.get(i).unwrap());
        }
    }
}

impl Default for TDag {
    fn default() -> Self {
        Self::new()
    }
}
