use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{EvalError, OpDag, PNote},
    awint_macro_internals::triple_arena::Advancer,
    Bits, ExtAwi,
};

use crate::{
    triple_arena::{Arena, SurjectArena},
    PBack, PTNode, TNode,
};

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<PBack>,
}

#[derive(Debug, Clone)]
pub struct Equiv {}

impl Equiv {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for Equiv {
    fn default() -> Self {
        Self::new()
    }
}

/// A DAG made primarily of lookup tables
#[derive(Debug, Clone)]
pub struct TDag {
    pub backrefs: SurjectArena<PBack, PTNode, ()>,
    pub tnodes: SurjectArena<PTNode, TNode, Equiv>,
    /// A kind of generation counter tracking the highest `visit` number
    visit_gen: NonZeroU64,
    pub notes: Arena<PNote, Note>,
    /// temporary used in evaluations
    front: Vec<PTNode>,
}

impl TDag {
    pub fn new() -> Self {
        Self {
            backrefs: SurjectArena::new(),
            tnodes: SurjectArena::new(),
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
        for (p_tnode, tnode, _) in &self.tnodes {
            if let Some(p_backref) = self.backrefs.get_key(tnode.p_back_self) {
                if p_tnode != *p_backref {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} `p_back_self` is broken"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{p_tnode}: {tnode:?} `p_back_self` is invalid"
                )))
            }
            for p_input in &tnode.inp {
                if let Some(p_backref) = self.backrefs.get_key(*p_input) {
                    if !self.tnodes.contains(*p_backref) {
                        return Err(EvalError::OtherString(format!(
                            "{p_tnode}: {tnode:?} input {p_input} has backref {p_backref} which \
                             is invalid"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} input {p_input} is invalid"
                    )))
                }
            }
            if let Some(loop_driver) = tnode.loop_driver {
                if let Some(p_backref) = self.backrefs.get_key(loop_driver) {
                    if p_tnode != *p_backref {
                        return Err(EvalError::OtherString(format!(
                            "{p_tnode}: {tnode:?} loop_driver {loop_driver} is broken"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} loop_driver {loop_driver} is invalid"
                    )))
                }
            }
        }
        for tnode in self.tnodes.keys() {
            if let Some(ref lut) = tnode.lut {
                if tnode.inp.is_empty() {
                    return Err(EvalError::OtherStr("no inputs for lookup table"))
                }
                if !lut.bw().is_power_of_two() {
                    return Err(EvalError::OtherStr(
                        "lookup table is not a power of two in bitwidth",
                    ))
                }
                if (lut.bw().trailing_zeros() as usize) != tnode.inp.len() {
                    return Err(EvalError::OtherStr(
                        "number of inputs does not correspond to lookup table size",
                    ))
                }
            } else if tnode.inp.len() > 1 {
                return Err(EvalError::OtherStr(
                    "`TNode` with no lookup table has more than one input",
                ))
            }
        }
        for note in self.notes.vals() {
            for p_bit in &note.bits {
                if let Some(p_backref) = self.backrefs.get_key(*p_bit) {
                    if self.tnodes.get_key(*p_backref).is_none() {
                        return Err(EvalError::OtherString(format!(
                            "note {p_bit}: backref {p_backref} is invalid"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!("note {p_bit} is invalid")))
                }
            }
        }
        Ok(())
    }

    /// Inserts a `TNode` with `lit` value and returns a `PTNode` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PTNode {
        self.tnodes.insert_with(|p| {
            let p_back_self = self.backrefs.insert(p, ());
            let mut tnode = TNode::new(p_back_self);
            tnode.val = lit;
            (tnode, Equiv::new())
        })
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
            if !self.tnodes.contains(*p_inx) {
                return None
            }
        }
        let p_new_tnode = self.tnodes.insert_with(|p_tnode| {
            let p_back_self = self.backrefs.insert(p_tnode, ());
            let mut tnode = TNode::new(p_back_self);
            tnode.lut = Some(ExtAwi::from(table));
            for p_inx in p_inxs {
                let p_back_self = tnode.p_back_self;
                let p_back = self.backrefs.insert_key(p_back_self, *p_inx).unwrap();
                tnode.inp.push(p_back);
            }
            (tnode, Equiv::new())
        });
        Some(p_new_tnode)
    }

    /// Sets up a loop from the loop source `p_looper` and driver `p_driver`
    pub fn make_loop(&mut self, p_looper: PTNode, p_driver: PTNode) -> Option<()> {
        let p_back_driver = self.tnodes.get_key(p_driver)?.p_back_self;
        let looper = self.tnodes.get_key_mut(p_looper)?;
        let p_backref = self.backrefs.insert_key(p_back_driver, p_looper).unwrap();
        looper.loop_driver = Some(p_backref);
        Some(())
    }

    /// Sets up an extra reference to `p_refer`
    pub fn make_extra_reference(&mut self, p_refer: PTNode) -> Option<PBack> {
        let p_back_self = self.tnodes.get_key_mut(p_refer)?.p_back_self;
        let p_back_new = self.backrefs.insert_key(p_back_self, p_refer).unwrap();
        Some(p_back_new)
    }

    // TODO need multiple variations of `eval`, one that assumes `lut` structure is
    // not changed and avoids propogation if equal values are detected.

    /// Evaluates everything and checks equivalences
    pub fn eval_all(&mut self) {
        let this_visit = self.next_visit_gen();

        // set `alg_rc` and get the initial front
        self.front.clear();
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get_key_mut(p_tnode).unwrap();
            let len = tnode.inp.len();
            tnode.alg_rc = u64::try_from(len).unwrap();
            if (len == 0) && tnode.val.is_some() {
                self.front.push(p_tnode);
            }
        }

        while let Some(p_tnode) = self.front.pop() {
            let tnode = self.tnodes.get_key(p_tnode).unwrap();
            let (val, propogate) = if tnode.lut.is_some() {
                // acquire LUT input
                let mut inx = 0;
                let len = tnode.inp.len();
                for i in 0..len {
                    let inp_p_tnode = self.backrefs.get_key(tnode.inp[i]).unwrap();
                    let inp_tnode = self.tnodes.get_key(*inp_p_tnode).unwrap();
                    inx |= (inp_tnode.val.unwrap() as usize) << i;
                }
                // evaluate
                let val = tnode.lut.as_ref().unwrap().get(inx).unwrap();
                (Some(val), true)
            } else if tnode.inp.len() == 1 {
                // wire propogation
                let inp_p_tnode = self.backrefs.get_key(tnode.inp[0]).unwrap();
                let inp_tnode = self.tnodes.get_key(*inp_p_tnode).unwrap();
                (inp_tnode.val, true)
            } else {
                (None, false)
            };
            let tnode = self.tnodes.get_key_mut(p_tnode).unwrap();
            if propogate {
                tnode.val = val;
            }
            tnode.visit = this_visit;
            // propogate
            let mut adv = self.tnodes.advancer_surject(p_tnode);
            while let Some(p_backref) = adv.advance(&self.tnodes) {
                let next = self.tnodes.get_key_mut(p_backref).unwrap();
                if (next.visit < this_visit) && (next.alg_rc != 0) {
                    next.alg_rc -= 1;
                    if next.alg_rc == 0 {
                        self.front.push(p_backref);
                    }
                }
            }
        }
    }

    pub fn drive_loops(&mut self) {
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            if let Some(driver) = self.tnodes.get_key(p_tnode).unwrap().loop_driver {
                let p_driver = self.backrefs.get_key(driver).unwrap();
                self.tnodes.get_key_mut(p_tnode).unwrap().val =
                    self.tnodes.get_key(*p_driver).unwrap().val;
            }
        }
    }

    pub fn get_p_tnode(&self, p_back: PBack) -> Option<PTNode> {
        Some(*self.backrefs.get_key(p_back)?)
    }

    pub fn get_tnode(&self, p_back: PBack) -> Option<&TNode> {
        let backref = self.backrefs.get_key(p_back)?;
        self.tnodes.get_key(*backref)
    }

    pub fn get_tnode_mut(&mut self, p_back: PBack) -> Option<&mut TNode> {
        let backref = self.backrefs.get_key(p_back)?;
        self.tnodes.get_key_mut(*backref)
    }

    pub fn get_noted_as_extawi(&self, p_note: PNote) -> Option<ExtAwi> {
        let note = self.notes.get(p_note)?;
        let mut x = ExtAwi::zero(NonZeroUsize::new(note.bits.len())?);
        for (i, p_bit) in note.bits.iter().enumerate() {
            let bit = self.get_tnode(*p_bit)?;
            let val = bit.val?;
            x.set(i, val).unwrap();
        }
        Some(x)
    }

    #[track_caller]
    pub fn set_noted(&mut self, p_note: PNote, val: &Bits) -> Option<()> {
        let note = self.notes.get(p_note)?;
        assert_eq!(note.bits.len(), val.bw());
        for (i, bit) in note.bits.iter().enumerate() {
            let val = Some(val.get(i).unwrap());
            let backref = self.backrefs.get_key(*bit)?;
            let tnode = self.tnodes.get_key_mut(*backref)?;
            tnode.val = val;
        }
        Some(())
    }
}

impl Default for TDag {
    fn default() -> Self {
        Self::new()
    }
}
