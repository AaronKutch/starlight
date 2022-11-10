use std::num::NonZeroUsize;

use awint::ExtAwi;
use smallvec::SmallVec;
use triple_arena::Ptr;

use crate::TDag;

impl<PTNode: Ptr> TDag<PTNode> {
    /// Removes a node, cleaning up bidirectional references
    fn remove_tnode(&mut self, p: PTNode) {
        let removed = self.a.remove(p).unwrap();
        for inp in &removed.inp {
            for (i, out) in self.a[inp].out.iter().enumerate() {
                if *out == p {
                    self.a[inp].out.swap_remove(i);
                    break
                }
            }
        }
        for out in &removed.out {
            for (i, inp) in self.a[out].inp.iter().enumerate() {
                if *inp == p {
                    self.a[out].inp.swap_remove(i);
                    break
                }
            }
        }
    }

    // If some inputs of a LUT are known, reduce the LUT. Also handles cases of
    // input independence and guaranteed outputs.
    fn internal_eval_advanced(&mut self) {
        let (mut p, mut b) = self.a.first_ptr();
        loop {
            if b {
                break
            }
            if let Some(mut lut) = self.a[p].lut.take() {
                let mut simplified = false;
                loop {
                    if self.a[p].rc > 0 {
                        break
                    }
                    for i in 0..self.a[p].inp.len() {
                        let inp = self.a[p].inp[i];
                        if let Some(val) = self.a[inp].val {
                            let new_bw = lut.bw() / 2;
                            assert!((lut.bw() % 2) == 0);
                            let mut new_lut = ExtAwi::zero(NonZeroUsize::new(new_bw).unwrap());
                            let offset = if val { 1 << i } else { 0 };
                            let mut j = 0;
                            let mut k = 0;
                            loop {
                                if k >= new_bw {
                                    break
                                }
                                new_lut.set(k, lut.get(j + offset).unwrap()).unwrap();
                                j += 1;
                                if (j & (1 << i)) != 0 {
                                    j += 1 << i;
                                }
                                k += 1;
                            }
                            lut = new_lut;
                            self.a[p].inp.remove(i);
                            for (i, out) in self.a[inp].out.iter().enumerate() {
                                if *out == p {
                                    self.a[inp].out.swap_remove(i);
                                    break
                                }
                            }
                            simplified = true;
                            break
                        }
                    }
                    if !simplified {
                        break
                    }
                    simplified = false;
                }
                // TODO do other optimizations, need to integrate into tree eval also
                // if lut.is_zero()
                // if lut.is_umax()
                // independence
                self.a[p].lut = Some(lut);
            }
            self.a.next_ptr(&mut p, &mut b);
        }
    }

    /// Removes trees of nodes with unused outputs. Modifies `alg_rc`.
    fn internal_remove_unused_outputs(&mut self) {
        for tnode in self.a.vals_mut() {
            tnode.alg_rc = u64::try_from(tnode.out.len()).unwrap();
        }
        let (mut p, mut b) = self.a.first_ptr();
        let mut v = SmallVec::<[PTNode; 32]>::new();
        loop {
            if b {
                break
            }
            if (self.a[p].rc == 0) && self.a[p].out.is_empty() {
                v.push(p);
                // handle deleting whole trees, `v` will stay small in most cases
                while let Some(p) = v.pop() {
                    for i in 0..self.a[p].inp.len() {
                        let inp = self.a[p].inp[i];
                        if self.a[inp].dec_alg_rc().unwrap() && (self.a[inp].rc == 0) {
                            v.push(inp);
                        }
                    }
                    self.remove_tnode(p);
                }
            }
            self.a.next_ptr(&mut p, &mut b);
        }
    }

    /// Removes trivial single bit chains. Assumes evaluation has happened (or
    /// else it could erase set values).
    fn internal_remove_chains(&mut self) {
        let (mut p, mut b) = self.a.first_ptr();
        loop {
            if b {
                break
            }
            let inp_len = self.a[p].inp.len();
            let out_len = self.a[p].out.len();
            if self.a[p].lut.is_none() && (self.a[p].rc == 0) && (inp_len <= 1) && (out_len <= 1) {
                match (inp_len == 1, out_len == 1) {
                    (true, true) => {
                        // reconnect chain
                        let inp = self.a[p].inp[0];
                        let out = self.a[p].out[0];
                        //assert_eq!(self.a[inp].val, self.a[p].val);
                        //assert_eq!(self.a[p].val, self.a[out].val);
                        for (i, tmp) in self.a[inp].out.iter().enumerate() {
                            if *tmp == p {
                                self.a[inp].out[i] = out;
                                break
                            }
                        }
                        for (i, tmp) in self.a[out].inp.iter().enumerate() {
                            if *tmp == p {
                                self.a[out].inp[i] = inp;
                                break
                            }
                        }
                        self.remove_tnode(p);
                    }
                    (false, true) => {
                        // avoid removing LUT inputs
                        let out = self.a[p].out[0];
                        if self.a[out].lut.is_none() {
                            self.remove_tnode(p);
                        }
                    }
                    _ => (), // should be removed by unused outputs
                }
            }
            self.a.next_ptr(&mut p, &mut b);
        }
    }

    /// Removes trees of nodes with unused inputs. Assumes `self.eval()` was
    /// performed and that values are correct. Modifies `alg_rc`.
    fn internal_remove_unused_inputs(&mut self) {
        for tnode in self.a.vals_mut() {
            tnode.alg_rc = u64::try_from(tnode.out.len()).unwrap();
        }
        let (mut p, mut b) = self.a.first_ptr();
        let mut v = SmallVec::<[PTNode; 32]>::new();
        loop {
            if b {
                break
            }
            if self.a[p].val.is_some() {
                v.push(p);
                while let Some(p) = v.pop() {
                    if self.a[p].val.is_some() {
                        // since we have our value, delete input edges
                        for i in 0..self.a[p].inp.len() {
                            let inp = self.a[p].inp[i];
                            for (i, out) in self.a[inp].out.iter().enumerate() {
                                if *out == p {
                                    self.a[inp].out.swap_remove(i);
                                    break
                                }
                            }
                            if self.a[inp].dec_alg_rc().unwrap() {
                                v.push(inp);
                            }
                        }
                        self.a[p].inp.clear();
                    }
                    if (self.a[p].rc == 0) && (self.a[p].alg_rc == 0) {
                        // dependents have the values they need
                        self.remove_tnode(p);
                    }
                }
            }
            self.a.next_ptr(&mut p, &mut b);
        }
        // evaluated nodes with lookup tables may still be around for use of their
        // values, and the lookup tables need to be cleaned up
        for tnode in self.a.vals_mut() {
            if tnode.inp.is_empty() {
                tnode.lut = None;
            }
        }
    }

    /// Performs basic simplifications of `self`, removing unused nodes and
    /// performing independent bit operations that do not change the
    /// functionality. If a `TNode` has `rc` of at least 1, no changes to that
    /// node are made.
    pub fn basic_simplify(&mut self) {
        // always run one round of this at the beginning, earlier stages are often bad
        // about unused nodes
        self.internal_remove_unused_outputs();
        self.eval();
        // also get the many chains out of the way early
        self.internal_remove_chains(); // assumes eval
        self.internal_eval_advanced(); // assumes basic eval
        self.internal_remove_unused_inputs(); // assumes eval
        self.internal_remove_unused_outputs();
        self.internal_remove_chains();
    }
}
