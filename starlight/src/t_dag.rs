use std::num::NonZeroUsize;

use awint::{
    awint_dag::{EvalError, OpDag, PNote},
    Bits, ExtAwi,
};

use crate::{triple_arena::Arena, PTNode, TNode};

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<PTNode>,
}

/// A DAG made primarily of lookup tables
#[derive(Debug, Clone)]
pub struct TDag {
    pub a: Arena<PTNode, TNode>,
    /// A kind of generation counter tracking the highest `visit` number
    pub visit_gen: u64,
    pub notes: Arena<PNote, Note>,
}

impl TDag {
    pub fn new() -> Self {
        Self {
            a: Arena::new(),
            visit_gen: 0,
            notes: Arena::new(),
        }
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
            for x in &node.inp {
                if !self.a.contains(*x) {
                    return Err(EvalError::OtherStr("broken input `PTNode`"))
                }
            }
            for y in &node.out {
                if !self.a.contains(*y) {
                    return Err(EvalError::OtherStr("broken output `PTNode`"))
                }
            }
        }
        // round trip
        for (p_node, node) in &self.a {
            for x in &node.inp {
                let mut found = false;
                for i in 0..self.a[x].out.len() {
                    if self.a[x].out[i] == p_node {
                        found = true;
                        break
                    }
                }
                if !found {
                    return Err(EvalError::OtherStr(
                        "failed round trip between inputs and outputs",
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
            for bit in &note.bits {
                if let Some(bit) = self.a.get(*bit) {
                    if bit.rc == 0 {
                        return Err(EvalError::OtherStr("reference count for noted bit is zero"))
                    }
                } else {
                    return Err(EvalError::OtherStr("broken `PTNode` in the noted bits"))
                }
            }
        }
        Ok(())
    }

    pub fn mark_nonloop_roots_permanent(&mut self) {
        for node in self.a.vals_mut() {
            if node.inp.is_empty() && node.loop_driver.is_none() {
                node.is_permanent = true;
            }
        }
    }

    pub fn propogate_permanence(&mut self) {
        self.visit_gen += 1;
        let this_visit = self.visit_gen;

        // acquire root nodes with permanence
        let mut front = vec![];
        for (p_node, node) in &mut self.a {
            let len = node.inp.len() as u64;
            node.alg_rc = len;
            if (len == 0) && node.is_permanent {
                front.push(p_node);
            }
        }

        while let Some(p_node) = front.pop() {
            self.a[p_node].visit = this_visit;

            // propogate
            for i in 0..self.a[p_node].out.len() {
                let leaf = self.a[p_node].out[i];
                if self.a[leaf].visit < this_visit {
                    if self.a[leaf].alg_rc > 0 {
                        self.a[leaf].alg_rc -= 1;
                    }
                    if self.a[leaf].alg_rc == 0 {
                        self.a[leaf].is_permanent = true;
                        front.push(leaf);
                    }
                }
            }
        }
    }

    // TODO this would be for trivial missed optimizations
    //pub fn verify_canonical(&self)

    // TODO need multiple variations of `eval`, one that assumes `lut` structure is
    // not changed and avoids propogation if equal values are detected.

    /// Evaluates `self` as much as possible. Uses only root `Some` bit values
    /// in propogation.
    pub fn eval(&mut self) {
        self.visit_gen += 1;
        let this_visit = self.visit_gen;

        // acquire root nodes with values
        let mut front = vec![];
        for (p_node, node) in &mut self.a {
            let len = node.inp.len() as u64;
            node.alg_rc = len;
            if (len == 0) && node.val.is_some() {
                front.push(p_node);
            }
        }

        while let Some(p_node) = front.pop() {
            self.a[p_node].visit = this_visit;
            if self.a[p_node].lut.is_some() {
                // acquire LUT input
                let mut inx = 0;
                for i in 0..self.a[p_node].inp.len() {
                    inx |= (self.a[self.a[p_node].inp[i]].val.unwrap() as usize) << i;
                }
                // evaluate
                let val = self.a[p_node].lut.as_ref().unwrap().get(inx).unwrap();
                self.a[p_node].val = Some(val);
            } else if !self.a[p_node].inp.is_empty() {
                // wire propogation
                self.a[p_node].val = self.a[self.a[p_node].inp[0]].val;
            }
            if self.a[p_node].val.is_none() {
                // val not updated
                continue
            }

            // propogate
            for i in 0..self.a[p_node].out.len() {
                let leaf = self.a[p_node].out[i];
                if self.a[leaf].visit < this_visit {
                    if self.a[leaf].alg_rc > 0 {
                        self.a[leaf].alg_rc -= 1;
                    }
                    if self.a[leaf].alg_rc == 0 {
                        front.push(leaf);
                    }
                }
            }
        }
    }

    pub fn drive_loops(&mut self) {
        let (mut p, mut b) = self.a.first_ptr();
        loop {
            if b {
                break
            }
            if let Some(driver) = self.a[p].loop_driver {
                self.a[p].val = self.a[driver].val;
            }
            self.a.next_ptr(&mut p, &mut b);
        }
    }

    pub fn get_noted_as_extawi(&self, p_note: PNote) -> ExtAwi {
        let note = &self.notes[p_note];
        let mut x = ExtAwi::zero(NonZeroUsize::new(note.bits.len()).unwrap());
        for (i, bit) in note.bits.iter().enumerate() {
            x.set(i, self.a[bit].val.unwrap()).unwrap();
        }
        x
    }

    pub fn set_noted(&mut self, p_note: PNote, val: &Bits) {
        let note = &self.notes[p_note];
        assert_eq!(note.bits.len(), val.bw());
        for (i, bit) in note.bits.iter().enumerate() {
            self.a[bit].val = Some(val.get(i).unwrap());
        }
    }
}

impl Default for TDag {
    fn default() -> Self {
        Self::new()
    }
}
