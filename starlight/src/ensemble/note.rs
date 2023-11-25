use awint::awint_dag::{PNote, PState};

use super::{Ensemble, Referent};
use crate::ensemble::PBack;

#[derive(Debug, Clone)]
pub struct Note {
    pub bits: Vec<PBack>,
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
            let p_back = self.make_note(p_note, p_bit).unwrap();
            self.notes[p_note].bits.push(p_back);
        }
        Some(p_note)
    }
}
