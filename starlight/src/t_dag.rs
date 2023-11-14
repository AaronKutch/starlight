use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::{
        smallvec::{smallvec, SmallVec},
        EvalError, Location, Op, OpDag, PNote, PState,
    },
    awint_macro_internals::triple_arena::Advancer,
    Awi, Bits,
};

use crate::{
    triple_arena::{Arena, SurjectArena},
    Optimizer, PBack, PTNode, TNode,
};

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<PBack>,
}

#[derive(Debug, Clone, Copy)]
pub enum Value {
    Unknown,
    Const(bool),
    Dynam(bool, NonZeroU64),
}

impl Value {
    pub fn from_dag_lit(lit: Option<bool>) -> Self {
        if let Some(lit) = lit {
            Value::Const(lit)
        } else {
            // TODO how to handle `Opaque`s?
            Value::Unknown
        }
    }

    pub fn known_value(self) -> Option<bool> {
        match self {
            Value::Unknown => None,
            Value::Const(b) => Some(b),
            Value::Dynam(b, _) => Some(b),
        }
    }

    pub fn is_const(self) -> bool {
        matches!(self, Value::Const(_))
    }

    pub fn is_known_with_visit_ge(self, visit: NonZeroU64) -> bool {
        match self {
            Value::Unknown => false,
            Value::Const(_) => true,
            Value::Dynam(_, this_visit) => this_visit >= visit,
        }
    }

    /// Converts constants to dynamics, and sets any generations to `visit_gen`
    pub fn const_to_dynam(self, visit_gen: NonZeroU64) -> Self {
        match self {
            Value::Unknown => Value::Unknown,
            Value::Const(b) => Value::Dynam(b, visit_gen),
            Value::Dynam(b, _) => Value::Dynam(b, visit_gen),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Equiv {
    /// `Ptr` back to this equivalence through a `Referent::ThisEquiv` in the
    /// backref surject associated with this `Equiv`
    pub p_self_equiv: PBack,
    /// Output of the equivalence surject
    pub val: Value,
    /// Used in algorithms
    pub equiv_alg_rc: usize,
    pub visit: NonZeroU64,
}

impl Equiv {
    pub fn new(p_self_equiv: PBack, val: Value) -> Self {
        Self {
            p_self_equiv,
            val,
            equiv_alg_rc: 0,
            visit:  NonZeroU64::new(1).unwrap(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Referent {
    /// Self equivalence class referent
    ThisEquiv,
    /// Self referent, used by all the `Tnode`s of an equivalence class
    ThisTNode(PTNode),
    /// Self referent to a particular bit of a `State`
    ThisStateBit(PState, usize),
    /// Referent is using this for registering an input dependency
    Input(PTNode),
    LoopDriver(PTNode),
    /// Referent is a note
    Note(PNote),
}

/// Represents the state resulting from a mimicking operation
#[derive(Debug, Clone)]
pub struct State {
    pub nzbw: NonZeroUsize,
    /// This either has zero length or has a length equal to `nzbw`
    pub p_self_bits: SmallVec<[PBack; 4]>,
    /// Operation
    pub op: Op<PState>,
    /// Location where this state is derived from
    pub location: Option<Location>,
    /// Used in algorithms for DFS tracking and to allow multiple DAG
    /// constructions from same nodes
    pub visit: NonZeroU64,
}

/// A DAG
#[derive(Debug, Clone)]
pub struct TDag {
    pub backrefs: SurjectArena<PBack, Referent, Equiv>,
    pub tnodes: Arena<PTNode, TNode>,
    // In order to preserve sanity, states are fairly weak in their existence.
    pub states: Arena<PState, State>,
    pub notes: Arena<PNote, Note>,
    /// A kind of generation counter tracking the highest `visit` number
    visit_gen: NonZeroU64,
    /// temporary used in evaluations
    tnode_front: Vec<PTNode>,
    equiv_front: Vec<PBack>,
}

impl TDag {
    pub fn new() -> Self {
        Self {
            backrefs: SurjectArena::new(),
            tnodes: Arena::new(),
            states: Arena::new(),
            visit_gen: NonZeroU64::new(2).unwrap(),
            notes: Arena::new(),
            tnode_front: vec![],
            equiv_front: vec![],
        }
    }

    pub fn visit_gen(&self) -> NonZeroU64 {
        self.visit_gen
    }

    pub fn next_visit_gen(&mut self) -> NonZeroU64 {
        self.visit_gen = NonZeroU64::new(self.visit_gen.get().checked_add(1).unwrap()).unwrap();
        self.visit_gen
    }

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

        // first check that equivalences aren't broken by themselves
        for p_back in self.backrefs.ptrs() {
            let equiv = self.backrefs.get_val(p_back).unwrap();
            if let Some(Referent::ThisEquiv) = self.backrefs.get_key(equiv.p_self_equiv) {
                if !self
                    .backrefs
                    .in_same_set(p_back, equiv.p_self_equiv)
                    .unwrap()
                {
                    return Err(EvalError::OtherString(format!(
                        "{equiv:?}.p_self_equiv roundtrip fail"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{equiv:?}.p_self_equiv is invalid"
                )))
            }
            // need to roundtrip in both directions to ensure existence and uniqueness of a
            // `ThisEquiv` for each equivalence surject
            if let Some(Referent::ThisEquiv) = self.backrefs.get_key(p_back) {
                if p_back != equiv.p_self_equiv {
                    return Err(EvalError::OtherString(format!(
                        "{equiv:?}.p_self_equiv roundtrip fail"
                    )))
                }
            }
        }
        // check other kinds of self refs
        for (p_tnode, tnode) in &self.tnodes {
            if let Some(Referent::ThisTNode(p_self)) = self.backrefs.get_key(tnode.p_self) {
                if p_tnode != *p_self {
                    return Err(EvalError::OtherString(format!(
                        "{tnode:?}.p_self roundtrip fail"
                    )))
                }
            } else {
                return Err(EvalError::OtherString(format!(
                    "{tnode:?}.p_self is invalid"
                )))
            }
        }
        for (p_state, state) in &self.states {
            for (inx, p_self_bit) in state.p_self_bits.iter().enumerate() {
                if let Some(Referent::ThisStateBit(p_self, inx_self)) =
                    self.backrefs.get_key(*p_self_bit)
                {
                    if (p_state != *p_self) || (inx != *inx_self) {
                        return Err(EvalError::OtherString(format!(
                            "{state:?}.p_self_bits roundtrip fail"
                        )))
                    }
                } else {
                    return Err(EvalError::OtherString(format!(
                        "{state:?}.p_self_bits is invalid"
                    )))
                }
            }
        }
        // check other referent validities
        for referent in self.backrefs.keys() {
            let invalid = match referent {
                // already checked
                Referent::ThisEquiv => false,
                Referent::ThisTNode(_) => false,
                Referent::ThisStateBit(..) => false,
                Referent::Input(p_input) => !self.tnodes.contains(*p_input),
                Referent::LoopDriver(p_driver) => !self.tnodes.contains(*p_driver),
                Referent::Note(p_note) => !self.notes.contains(*p_note),
            };
            if invalid {
                return Err(EvalError::OtherString(format!("{referent:?} is invalid")))
            }
        }
        // other kinds of validity
        for p_tnode in self.tnodes.ptrs() {
            let tnode = self.tnodes.get(p_tnode).unwrap();
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
        // Other roundtrips from `backrefs` direction to ensure bijection
        for p_back in self.backrefs.ptrs() {
            let referent = self.backrefs.get_key(p_back).unwrap();
            let fail = match referent {
                // already checked
                Referent::ThisEquiv => false,
                Referent::ThisTNode(p_tnode) => {
                    let tnode = self.tnodes.get(*p_tnode).unwrap();
                    p_back != tnode.p_self
                }
                Referent::ThisStateBit(p_state, inx) => {
                    let state = self.states.get(*p_state).unwrap();
                    let p_bit = state.p_self_bits.get(*inx).unwrap();
                    *p_bit != p_back
                }
                Referent::Input(p_input) => {
                    let tnode1 = self.tnodes.get(*p_input).unwrap();
                    let mut found = false;
                    for p_back1 in &tnode1.inp {
                        if *p_back1 == p_back {
                            found = true;
                            break
                        }
                    }
                    !found
                }
                Referent::LoopDriver(p_loop) => {
                    let tnode1 = self.tnodes.get(*p_loop).unwrap();
                    tnode1.loop_driver != Some(p_back)
                }
                Referent::Note(p_note) => {
                    let note = self.notes.get(*p_note).unwrap();
                    let mut found = false;
                    for bit in &note.bits {
                        if *bit == p_back {
                            found = true;
                            break
                        }
                    }
                    !found
                }
            };
            if fail {
                return Err(EvalError::OtherString(format!(
                    "{referent:?} roundtrip fail"
                )))
            }
        }
        // non-pointer invariants
        for tnode in self.tnodes.vals() {
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

        // TODO verify DAGness
        Ok(())
    }

    pub fn make_state(
        &mut self,
        nzbw: NonZeroUsize,
        op: Op<PState>,
        location: Option<Location>,
    ) -> PState {
        self.states.insert(State {
            nzbw,
            p_self_bits: SmallVec::new(),
            op,
            location,
            visit: NonZeroU64::new(2).unwrap(),
        })
    }

    /// If `p_state_bits.is_empty`, this will create new equivalences and
    /// `Referent::ThisStateBits`s needed for every self bit. Sets the values to
    /// a constant if the `Op` is a `Literal`, otherwise sets to unknown.
    pub fn initialize_state_bits_if_needed(&mut self, p_state: PState) -> Option<()> {
        let state = self.states.get(p_state)?;
        if !state.p_self_bits.is_empty() {
            return Some(())
        }
        let mut bits = smallvec![];
        for i in 0..state.nzbw.get() {
            let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
                (
                    Referent::ThisEquiv,
                    Equiv::new(
                        p_self_equiv,
                        if let Op::Literal(ref awi) = state.op {
                            Value::Const(awi.get(i).unwrap())
                        } else {
                            Value::Unknown
                        },
                    ),
                )
            });
            bits.push(
                self.backrefs
                    .insert_key(p_equiv, Referent::ThisStateBit(p_state, i))
                    .unwrap(),
            );
        }
        let state = self.states.get_mut(p_state).unwrap();
        state.p_self_bits = bits;
        Some(())
    }

    /// Inserts a `TNode` with `lit` value and returns a `PBack` to it
    pub fn make_literal(&mut self, lit: Option<bool>) -> PBack {
        self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::from_dag_lit(lit)),
            )
        })
    }

    /// Makes a single output bit lookup table `TNode` and returns a `PBack` to
    /// it. Returns `None` if the table length is incorrect or any of the
    /// `p_inxs` are invalid.
    pub fn make_lut(&mut self, p_inxs: &[PBack], table: &Bits) -> Option<PBack> {
        let num_entries = 1 << p_inxs.len();
        if table.bw() != num_entries {
            return None
        }
        for p_inx in p_inxs {
            if !self.backrefs.contains(*p_inx) {
                return None
            }
        }
        let p_equiv = self.backrefs.insert_with(|p_self_equiv| {
            (
                Referent::ThisEquiv,
                Equiv::new(p_self_equiv, Value::Unknown),
            )
        });
        self.tnodes.insert_with(|p_tnode| {
            let p_self = self
                .backrefs
                .insert_key(p_equiv, Referent::ThisTNode(p_tnode))
                .unwrap();
            let mut tnode = TNode::new(p_self);
            tnode.lut = Some(Awi::from(table));
            for p_inx in p_inxs {
                let p_back = self
                    .backrefs
                    .insert_key(*p_inx, Referent::Input(p_tnode))
                    .unwrap();
                tnode.inp.push(p_back);
            }
            tnode
        });
        Some(p_equiv)
    }

    /// Sets up a loop from the loop source `p_looper` and driver `p_driver`
    pub fn make_loop(&mut self, p_looper: PBack, p_driver: PBack, init_val: Value) -> Option<()> {
        let looper_equiv = self.backrefs.get_val_mut(p_looper)?;
        match looper_equiv.val {
            Value::Unknown => (),
            // shouldn't fail unless the special Opaque loopback structure is broken
            _ => panic!("looper is already set to a known value"),
        }
        looper_equiv.val = init_val;

        let referent = self.backrefs.get_key(p_looper)?;
        let p_looper_tnode = match referent {
            Referent::ThisEquiv => {
                // need to create the TNode
                self.tnodes.insert_with(|p_tnode| {
                    let p_back_self = self
                        .backrefs
                        .insert_key(p_looper, Referent::ThisTNode(p_tnode))
                        .unwrap();
                    TNode::new(p_back_self)
                })
            }
            // we might want to support more cases in the future
            _ => panic!("bad referent {referent:?}"),
        };
        let p_back_driver = self
            .backrefs
            .insert_key(p_driver, Referent::LoopDriver(p_looper_tnode))
            .unwrap();
        let tnode = self.tnodes.get_mut(p_looper_tnode).unwrap();
        tnode.loop_driver = Some(p_back_driver);
        Some(())
    }

    /// Sets up an extra reference to `p_refer`
    pub fn make_note(&mut self, p_note: PNote, p_refer: PBack) -> Option<PBack> {
        let p_equiv = self.backrefs.get_val(p_refer)?.p_self_equiv;
        let p_back_new = self
            .backrefs
            .insert_key(p_equiv, Referent::Note(p_note))
            .unwrap();
        Some(p_back_new)
    }

    /// Evaluates everything and checks equivalences
    pub fn eval_all(&mut self) -> Result<(), EvalError> {
        let this_visit = self.next_visit_gen();

        // set `alg_rc` and get the initial front
        self.tnode_front.clear();
        self.equiv_front.clear();
        for (p, tnode) in &mut self.tnodes {
            let len = tnode.inp.len();
            tnode.alg_rc = u64::try_from(len).unwrap();
            if len == 0 {
                self.tnode_front.push(p);
            }
        }
        for equiv in self.backrefs.vals_mut() {
            equiv.equiv_alg_rc = 0;
        }
        let mut adv = self.backrefs.advancer();
        while let Some(p_back) = adv.advance(&self.backrefs) {
            let (referent, equiv) = self.backrefs.get_mut(p_back).unwrap();
            match referent {
                Referent::ThisEquiv => (),
                Referent::ThisTNode(_) => {
                    equiv.equiv_alg_rc += 1;
                }
                // we should do this
                Referent::ThisStateBit(..) => todo!(),
                Referent::Input(_) => (),
                Referent::LoopDriver(_) => (),
                Referent::Note(_) => (),
            }
        }
        for equiv in self.backrefs.vals() {
            if equiv.equiv_alg_rc == 0 {
                self.equiv_front.push(equiv.p_self_equiv);
            }
        }

        loop {
            // prioritize tnodes before equivalences, better finds the root cause of
            // equivalence mismatches
            if let Some(p_tnode) = self.tnode_front.pop() {
                let tnode = self.tnodes.get_mut(p_tnode).unwrap();
                let (val, set_val) = if tnode.lut.is_some() {
                    // acquire LUT input
                    let mut inx = 0;
                    let len = tnode.inp.len();
                    let mut propogate_unknown = false;
                    for i in 0..len {
                        let equiv = self.backrefs.get_val(tnode.inp[i]).unwrap();
                        match equiv.val {
                            Value::Unknown => {
                                propogate_unknown = true;
                                break
                            }
                            Value::Const(val) => {
                                inx |= (val as usize) << i;
                            }
                            Value::Dynam(val, _) => {
                                inx |= (val as usize) << i;
                            }
                        }
                    }
                    if propogate_unknown {
                        (Value::Unknown, true)
                    } else {
                        // evaluate
                        let val = tnode.lut.as_ref().unwrap().get(inx).unwrap();
                        (Value::Dynam(val, this_visit), true)
                    }
                } else if tnode.inp.len() == 1 {
                    // wire propogation
                    let equiv = self.backrefs.get_val(tnode.inp[0]).unwrap();
                    (equiv.val, true)
                } else {
                    // some other case like a looper, value gets set by something else
                    (Value::Unknown, false)
                };
                let equiv = self.backrefs.get_val_mut(tnode.p_self).unwrap();
                if set_val {
                    match equiv.val {
                        Value::Unknown => {
                            equiv.val = val;
                        }
                        Value::Const(_) => unreachable!(),
                        Value::Dynam(prev_val, prev_visit) => {
                            if prev_visit == this_visit {
                                let mismatch = match val {
                                    Value::Unknown => true,
                                    Value::Const(_) => unreachable!(),
                                    Value::Dynam(new_val, _) => new_val != prev_val,
                                };
                                if mismatch {
                                    // dynamic sets from this visit are disagreeing
                                    return Err(EvalError::OtherString(format!(
                                        "disagreement on equivalence value for {}",
                                        equiv.p_self_equiv
                                    )))
                                }
                            } else {
                                equiv.val = val;
                            }
                        }
                    }
                }
                equiv.equiv_alg_rc = equiv.equiv_alg_rc.checked_sub(1).unwrap();
                if equiv.equiv_alg_rc == 0 {
                    self.equiv_front.push(equiv.p_self_equiv);
                }
                tnode.visit = this_visit;
                continue
            }
            if let Some(p_equiv) = self.equiv_front.pop() {
                let mut adv = self.backrefs.advancer_surject(p_equiv);
                while let Some(p_back) = adv.advance(&self.backrefs) {
                    // notify dependencies
                    match self.backrefs.get_key(p_back).unwrap() {
                        Referent::ThisEquiv => (),
                        Referent::ThisTNode(_) => (),
                        Referent::ThisStateBit(..) => (),
                        Referent::Input(p_dep) => {
                            let dep = self.tnodes.get_mut(*p_dep).unwrap();
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
                continue
            }
            break
        }
        Ok(())
    }

    pub fn drive_loops(&mut self) {
        let mut adv = self.tnodes.advancer();
        while let Some(p_tnode) = adv.advance(&self.tnodes) {
            let tnode = self.tnodes.get(p_tnode).unwrap();
            if let Some(p_driver) = tnode.loop_driver {
                let driver_equiv = self.backrefs.get_val(p_driver).unwrap();
                let val = driver_equiv.val;
                let looper_equiv = self.backrefs.get_val_mut(tnode.p_self).unwrap();
                looper_equiv.val = val;
            }
        }
    }

    pub fn get_val(&self, p_back: PBack) -> Option<Value> {
        Some(self.backrefs.get_val(p_back)?.val)
    }

    pub fn get_noted_as_extawi(&self, p_note: PNote) -> Result<Awi, EvalError> {
        if let Some(note) = self.notes.get(p_note) {
            // avoid partially setting by prechecking validity of all bits
            for p_bit in &note.bits {
                if let Some(equiv) = self.backrefs.get_val(*p_bit) {
                    match equiv.val {
                        Value::Unknown => return Err(EvalError::Unevaluatable),
                        Value::Const(_) => (),
                        Value::Dynam(..) => (),
                    }
                } else {
                    return Err(EvalError::OtherStr("broken note"))
                }
            }
            let mut x = Awi::zero(NonZeroUsize::new(note.bits.len()).unwrap());
            for (i, p_bit) in note.bits.iter().enumerate() {
                let equiv = self.backrefs.get_val(*p_bit).unwrap();
                let val = match equiv.val {
                    Value::Unknown => unreachable!(),
                    Value::Const(val) => val,
                    Value::Dynam(val, _) => val,
                };
                x.set(i, val).unwrap();
            }
            Ok(x)
        } else {
            Err(EvalError::InvalidPtr)
        }
    }

    #[track_caller]
    pub fn set_noted(&mut self, p_note: PNote, val: &Bits) -> Option<()> {
        let note = self.notes.get(p_note)?;
        assert_eq!(note.bits.len(), val.bw());
        for (i, bit) in note.bits.iter().enumerate() {
            let equiv = self.backrefs.get_val_mut(*bit)?;
            equiv.val = Value::Dynam(val.get(i).unwrap(), self.visit_gen);
        }
        Some(())
    }

    pub fn optimize_basic(&mut self) {
        // all 0 gas optimizations
        let mut opt = Optimizer::new();
        opt.optimize_all(self);
    }
}

impl Default for TDag {
    fn default() -> Self {
        Self::new()
    }
}
