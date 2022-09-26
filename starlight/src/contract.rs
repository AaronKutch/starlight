use triple_arena::Link;

use crate::{Bit, PBit, PermDag};

impl PermDag {
    /// Performs some basic simplification on `self` without invalidating noted
    /// bits. Should be run after `eval` has been run.
    pub fn contract(&mut self) {
        let mut bit_states: Vec<PBit> = self.bits.ptrs().collect();

        for bit in &bit_states {
            if self.bits[bit].rc == 0 {
                if self.bits[bit].state.is_some() {
                    if let Some(next) = Link::next(&self.bits[bit]) {
                        if self.bits[next].state.is_some() {
                            // only remove if the next bit knows its value
                            self.bits.remove(*bit);
                        }
                    }
                } else if self.bits[bit].lut.is_none() {
                    // no-op bit, remove
                    self.bits.remove(*bit);
                }
            } else if self.bits[bit].state.is_some() {
                // never remove bit, but we can remove lut
                self.bits[bit].lut = None;
            }
        }
    }
}
