use std::num::NonZeroUsize;

use awint::awint_dag::{triple_arena::ptr_struct, EvalError, PState};

use crate::{
    awi,
    ensemble::{Ensemble, PBack, Referent, Value},
    epoch::get_current_epoch,
};

ptr_struct!(PNote);

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<Option<PBack>>,
}

impl Note {
    pub fn new() -> Self {
        Self { bits: vec![] }
    }
}

impl Ensemble {
    /// Sets up an extra reference to `p_refer`
    #[must_use]
    pub fn make_note(&mut self, p_note: PNote, p_refer: PBack) -> Option<PBack> {
        let p_equiv = self.backrefs.get_val(p_refer)?.p_self_equiv;
        let p_back_new = self
            .backrefs
            .insert_key(p_equiv, Referent::Note(p_note))
            .unwrap();
        Some(p_back_new)
    }

    #[must_use]
    pub fn note_pstate(&mut self, p_state: PState) -> Option<PNote> {
        self.initialize_state_bits_if_needed(p_state)?;
        let p_note = self.notes.insert(Note::new());
        let len = self.stator.states[p_state].p_self_bits.len();
        for i in 0..len {
            let p_bit = self.stator.states[p_state].p_self_bits[i];
            if let Some(p_bit) = p_bit {
                let p_back = self.make_note(p_note, p_bit).unwrap();
                self.notes[p_note].bits.push(Some(p_back));
            } else {
                self.notes[p_note].bits.push(None);
            }
        }
        Some(p_note)
    }

    pub fn remove_note(&mut self, p_note: PNote) -> Result<(), EvalError> {
        if let Some(note) = self.notes.remove(p_note) {
            for p_back in note.bits {
                if let Some(p_back) = p_back {
                    let referent = self.backrefs.remove_key(p_back).unwrap().0;
                    assert!(matches!(referent, Referent::Note(_)));
                }
            }
            Ok(())
        } else {
            Err(EvalError::InvalidPtr)
        }
    }

    pub fn get_thread_local_note_nzbw(p_note: PNote) -> Result<NonZeroUsize, EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some(note) = ensemble.notes.get(p_note) {
            Ok(NonZeroUsize::new(note.bits.len()).unwrap())
        } else {
            Err(EvalError::OtherStr("could not find thread local `Note`"))
        }
    }

    pub fn change_thread_local_note_value(
        p_note: PNote,
        bits: &awi::Bits,
    ) -> Result<(), EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        if let Some(note) = ensemble.notes.get(p_note) {
            if note.bits.len() != bits.bw() {
                return Err(EvalError::WrongBitwidth);
            }
        } else {
            return Err(EvalError::OtherStr("could not find thread local `Note`"))
        }
        for bit_i in 0..bits.bw() {
            let p_back = ensemble.notes[p_note].bits[bit_i];
            if let Some(p_back) = p_back {
                ensemble
                    .change_value(p_back, Value::Dynam(bits.get(bit_i).unwrap()))
                    .unwrap();
            }
        }
        Ok(())
    }

    pub fn calculate_thread_local_note_value(
        p_note: PNote,
        bit_i: usize,
    ) -> Result<Value, EvalError> {
        let epoch_shared = get_current_epoch().unwrap();
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        let p_back = if let Some(note) = ensemble.notes.get(p_note) {
            if bit_i >= note.bits.len() {
                return Err(EvalError::OtherStr(
                    "something went wrong with note bitwidth",
                ));
            }
            if let Some(p_back) = note.bits[bit_i] {
                p_back
            } else {
                return Err(EvalError::OtherStr(
                    "something went wrong, found `Note` for evaluator but a bit was denoted",
                ))
            }
        } else {
            return Err(EvalError::OtherStr("could not find thread local `Note`"))
        };
        if ensemble.stator.states.is_empty() {
            // optimization after total pruning from `optimization`
            ensemble.calculate_value(p_back)
        } else {
            drop(lock);
            Ensemble::calculate_value_with_lower_capability(&epoch_shared, p_back)
        }
    }
}

impl Default for Note {
    fn default() -> Self {
        Self::new()
    }
}
