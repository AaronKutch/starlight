use triple_arena::Link;

use crate::{PBit, TDag};

impl TDag {
    /// Performs some basic simplification on `self` without invalidating noted
    /// bits. Should be run after `eval` has been run.
    pub fn contract(&mut self) {
        // acquire all leaf bits
        let mut front = vec![];
        for (p_bit, bit) in &self.bits {
            if Link::next(bit).is_none() {
                front.push(p_bit);
            }
        }

        // first cull unused leafward parts as much as possible
        for bit in front {
            let mut bit = bit;
            loop {
                let link = &self.bits[bit];
                if (link.rc == 0) && link.lut.is_none() {
                    if let Some(next_bit) = Link::prev(link) {
                        self.bits.remove(bit).unwrap();
                        bit = next_bit;
                    } else {
                        self.bits.remove(bit).unwrap();
                        break
                    }
                } else {
                    break
                }
            }
        }

        let bit_states: Vec<PBit> = self.bits.ptrs().collect();

        // this will be used to track if LUTs can be eliminated
        for lut in self.luts.vals_mut() {
            lut.bit_rc = lut.bits.len();
        }

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
