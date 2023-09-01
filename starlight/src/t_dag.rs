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
pub struct Equiv {
    /// `Ptr` back to this equivalence
    pub p_self_equiv: PBack,
    /// Output of the equivalence surject
    pub val: Option<bool>,
    /// Used in algorithms
    pub equiv_alg_rc: usize,
}

impl Equiv {
    pub fn new(p_self_equiv: PBack, val: Option<bool>) -> Self {
        Self {
            p_self_equiv,
            val,
            equiv_alg_rc: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Referent {
    /// Self referent
    This,
    /// Equiv self referent
    ThisEquiv,
    /// Referent is using this for registering an input dependency
    Input(PTNode),
    LoopDriver(PTNode),
    /// Referent is a note
    Note(PNote),
}

/// A DAG made primarily of lookup tables
#[derive(Debug, Clone)]
pub struct TDag {
    pub(crate) backrefs: SurjectArena<PBack, Referent, PTNode>,
    pub(crate) tnodes: SurjectArena<PTNode, TNode, Equiv>,
    /// A kind of generation counter tracking the highest `visit` number
    visit_gen: NonZeroU64,
    pub notes: Arena<PNote, Note>,
    /// temporary used in evaluations
    tnode_front: Vec<PTNode>,
    equiv_front: Vec<PTNode>,
}

impl TDag {
    pub fn new() -> Self {
        Self {
            backrefs: SurjectArena::new(),
            tnodes: SurjectArena::new(),
            visit_gen: NonZeroU64::new(2).unwrap(),
            notes: Arena::new(),
            tnode_front: vec![],
            equiv_front: vec![],
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
        for referred in self.backrefs.vals() {
            if !self.tnodes.contains(*referred) {
                return Err(EvalError::OtherString(format!(
                    "referred {referred:?} is invalid"
                )))
            }
        }
        for equiv in self.tnodes.vals() {
            if let Some(referent) = self.backrefs.get_key(equiv.p_self_equiv) {
                if !matches!(referent, Referent::ThisEquiv) {
                    return Err(EvalError::OtherString(format!(
                        "{equiv:?}.p_self is not a self equiv referent"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{equiv:?}.p_self is invalid"
                )))
            }
        }
        for tnode in self.tnodes.keys() {
            if let Some(referent) = self.backrefs.get_key(tnode.p_self) {
                if !matches!(referent, Referent::This) {
                    return Err(EvalError::OtherString(format!(
                        "{tnode:?}.p_self is not a self referent"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{tnode:?}.p_self is invalid"
                )))
            }
        }
        for referent in self.backrefs.keys() {
            match referent {
                Referent::This => (),
                Referent::ThisEquiv => (),
                Referent::Input(p_input) => {
                    if !self.tnodes.contains(*p_input) {
                        return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
                    }
                }
                Referent::LoopDriver(p_driver) => {
                    if !self.tnodes.contains(*p_driver) {
                        return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
                    }
                }
                Referent::Note(p_note) => {
                    if !self.notes.contains(*p_note) {
                        return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
                    }
                }
            }
        }
        for p_tnode in self.tnodes.ptrs() {
            let tnode = self.tnodes.get_key(p_tnode).unwrap();
            for p_input in &tnode.inp {
                if let Some(referent) = self.backrefs.get_key(*p_input) {
                    if let Referent::Input(referent) = referent {
                        if !self.tnodes.contains(*referent) {
                            return Err(EvalError::OtherString(format!(
                                "{p_tnode}: {tnode:?} input {p_input} referrent {referent} is \
                                 invalid"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{p_tnode}: {tnode:?} input {p_input} has incorrect referrent"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} input {p_input} is invalid"
                    )))
                }
            }
            if let Some(loop_driver) = tnode.loop_driver {
                if let Some(referent) = self.backrefs.get_key(loop_driver) {
                    if let Referent::LoopDriver(p_driver) = referent {
                        if !self.tnodes.contains(*p_driver) {
                            return Err(EvalError::OtherString(format!(
                                "{p_tnode}: {tnode:?} loop driver referrent {p_driver} is invalid"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{p_tnode}: {tnode:?} loop driver has incorrect referrent"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{p_tnode}: {tnode:?} loop driver {loop_driver} is invalid"
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
            for p_back in &note.bits {
                if let Some(referent) = self.backrefs.get_key(*p_back) {
                    if let Referent::Note(p_note) = referent {
                        if !self.notes.contains(*p_note) {
                            return Err(EvalError::OtherString(format!(
                                "{note:?} backref {p_note} is invalid"
                            )))
                        }
                    } else {
                        return Err(EvalError::OtherString(format!(
                            "{note:?} backref {p_back} has incorrect referrent"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!("note {p_back} is invalid")))
                }
            }
        }
        Ok(())
    }

    /// Inserts a `TNode` with `lit` value and returns a `PTNode` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PTNode {
        self.tnodes.insert_with(|p_tnode| {
            let p_self = self.backrefs.insert(Referent::This, p_tnode);
            let p_self_equiv = self
                .backrefs
                .insert_key(p_self, Referent::ThisEquiv)
                .unwrap();
            let mut tnode = TNode::new(p_self);
            tnode.val = lit;
            (tnode, Equiv::new(p_self_equiv, lit))
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
            let p_self = self.backrefs.insert(Referent::This, p_tnode);
            let p_self_equiv = self
                .backrefs
                .insert_key(p_self, Referent::ThisEquiv)
                .unwrap();
            let mut tnode = TNode::new(p_self);
            tnode.lut = Some(ExtAwi::from(table));
            (tnode, Equiv::new(p_self_equiv, None))
        });
        for p_inx in p_inxs {
            let p_back_input = self.tnodes.get_key(*p_inx).unwrap().p_self;
            let p_back = self
                .backrefs
                .insert_key(p_back_input, Referent::Input(p_new_tnode))
                .unwrap();
            let tnode = self.tnodes.get_key_mut(p_new_tnode).unwrap();
            tnode.inp.push(p_back);
        }
        Some(p_new_tnode)
    }

    /// Sets up a loop from the loop source `p_looper` and driver `p_driver`
    pub fn make_loop(
        &mut self,
        p_looper: PTNode,
        p_driver: PTNode,
        init_val: Option<bool>,
    ) -> Option<()> {
        let p_driver = self.tnodes.get_key(p_driver)?.p_self;
        let looper = self.tnodes.get_key_mut(p_looper)?;
        looper.val = init_val;
        let p_backref = self
            .backrefs
            .insert_key(p_driver, Referent::LoopDriver(p_looper))
            .unwrap();
        looper.loop_driver = Some(p_backref);
        Some(())
    }

    /// Sets up an extra reference to `p_refer`
    pub fn make_note(&mut self, p_note: PNote, p_refer: PTNode) -> Option<PBack> {
        let p_back_self = self.tnodes.get_key_mut(p_refer)?.p_self;
        let p_back_new = self
            .backrefs
            .insert_key(p_back_self, Referent::Note(p_note))
            .unwrap();
        Some(p_back_new)
    }

    // TODO need multiple variations of `eval`, one that assumes `lut` structure is
    // not changed and avoids propogation if equal values are detected.

    /// Checks that `TNode` values within an equivalance surject agree and
    /// pushes tnodes to the tnode_front.
    fn eval_equiv_push_tnode_front(
        &mut self,
        p_equiv: PTNode,
        this_visit: NonZeroU64,
    ) -> Result<(), EvalError> {
        let mut common_val: Option<Option<bool>> = None;
        // equivalence class level
        let mut adv_equiv = self.tnodes.advancer_surject(p_equiv);
        while let Some(p_tnode) = adv_equiv.advance(&self.tnodes) {
            let tnode = self.tnodes.get_key(p_tnode).unwrap();
            if let Some(common_val) = common_val {
                if common_val != tnode.val {
                    return Err(EvalError::OtherString(format!(
                        "value disagreement within equivalence surject {p_equiv}, {p_tnode}"
                    )))
                }
            } else {
                common_val = Some(tnode.val);
            }

            // notify dependencies
            let mut adv_backref = self.backrefs.advancer_surject(tnode.p_self);
            while let Some(p_back) = adv_backref.advance(&self.backrefs) {
                match self.backrefs.get_key(p_back).unwrap() {
                    Referent::This => (),
                    Referent::ThisEquiv => (),
                    Referent::Input(p_dep) => {
                        let dep = self.tnodes.get_key_mut(*p_dep).unwrap();
                        // also ends up skipping self `Ptr`s
                        if dep.visit < this_visit {
                            dep.alg_rc = dep.alg_rc.checked_sub(1).unwrap();
                            if dep.alg_rc == 0 {
                                self.tnode_front.push(*p_dep);
                            }
                        }
                    }
                    Referent::LoopDriver(_) => (),
                    Referent::Note(_) => (),
                }
            }
        }

        self.tnodes.get_val_mut(p_equiv).unwrap().val = common_val.unwrap();
        Ok(())
    }

    /// Evaluates everything and checks equivalences
    pub fn eval_all(&mut self) -> Result<(), EvalError> {
        let this_visit = self.next_visit_gen();

        // set `alg_rc` and get the initial front
        self.tnode_front.clear();
        self.equiv_front.clear();
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get_key_mut(p_tnode).unwrap();
            let len = tnode.inp.len();
            tnode.alg_rc = u64::try_from(len).unwrap();
            // include `tnode.val.is_none()` values so that we can propogate `None`s
            if len == 0 {
                self.tnode_front.push(p_tnode);
            }

            // set equiv rc
            // TODO this is done for every tnode, could be done once for each surject
            let equiv = self.tnodes.get_val(p_tnode).unwrap();
            let p_equiv = self.backrefs.get_val(equiv.p_self_equiv).unwrap();
            let equiv_len = self.tnodes.len_key_set(*p_equiv).unwrap();
            let equiv = self.tnodes.get_val_mut(p_tnode).unwrap();
            equiv.equiv_alg_rc = equiv_len.get();
        }

        loop {
            // prioritize equivalences to find the root cause
            if let Some(p_equiv) = self.equiv_front.pop() {
                self.eval_equiv_push_tnode_front(p_equiv, this_visit)?;
                continue
            }
            if let Some(p_tnode) = self.tnode_front.pop() {
                let tnode = self.tnodes.get_key(p_tnode).unwrap();
                let (val, set_val) = if tnode.lut.is_some() {
                    // acquire LUT input
                    let mut inx = 0;
                    let len = tnode.inp.len();
                    let mut propogate_none = false;
                    for i in 0..len {
                        let inp_p_tnode = self.backrefs.get_val(tnode.inp[i]).unwrap();
                        let inp_tnode = self.tnodes.get_key(*inp_p_tnode).unwrap();
                        if let Some(val) = inp_tnode.val {
                            inx |= (val as usize) << i;
                        } else {
                            propogate_none = true;
                            break
                        }
                    }
                    if propogate_none {
                        (None, true)
                    } else {
                        // evaluate
                        let val = tnode.lut.as_ref().unwrap().get(inx).unwrap();
                        (Some(val), true)
                    }
                } else if tnode.inp.len() == 1 {
                    // wire propogation
                    let inp_p_tnode = self.backrefs.get_val(tnode.inp[0]).unwrap();
                    let inp_tnode = self.tnodes.get_key(*inp_p_tnode).unwrap();
                    (inp_tnode.val, true)
                } else {
                    // node with no input
                    (None, false)
                };
                let tnode = self.tnodes.get_key_mut(p_tnode).unwrap();
                if set_val {
                    tnode.val = val;
                }
                tnode.visit = this_visit;

                let equiv = self.tnodes.get_val_mut(p_tnode).unwrap();
                equiv.equiv_alg_rc = equiv.equiv_alg_rc.checked_sub(1).unwrap();
                if equiv.equiv_alg_rc == 0 {
                    self.equiv_front.push(p_tnode);
                }
                continue
            }
            break
        }
        Ok(())
    }

    pub fn drive_loops(&mut self) {
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            if let Some(driver) = self.tnodes.get_key(p_tnode).unwrap().loop_driver {
                let p_driver = self.backrefs.get_val(driver).unwrap();
                self.tnodes.get_key_mut(p_tnode).unwrap().val =
                    self.tnodes.get_key(*p_driver).unwrap().val;
            }
        }
    }

    pub fn get_p_tnode(&self, p_back: PBack) -> Option<PTNode> {
        Some(*self.backrefs.get_val(p_back)?)
    }

    pub fn get_tnode(&self, p_back: PBack) -> Option<&TNode> {
        let backref = self.backrefs.get_val(p_back)?;
        self.tnodes.get_key(*backref)
    }

    pub fn get_tnode_mut(&mut self, p_back: PBack) -> Option<&mut TNode> {
        let backref = self.backrefs.get_val(p_back)?;
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
            let backref = self.backrefs.get_val(*bit)?;
            let mut adv_equiv = self.tnodes.advancer_surject(*backref);
            while let Some(p_tnode) = adv_equiv.advance(&self.tnodes) {
                let tnode = self.tnodes.get_key_mut(p_tnode)?;
                tnode.val = val;
            }
        }
        Some(())
    }
}

impl Default for TDag {
    fn default() -> Self {
        Self::new()
    }
}
