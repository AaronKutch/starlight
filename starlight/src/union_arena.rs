// TODO eventually include in main `triple_arena` crate

use std::num::NonZeroUsize;

use crate::triple_arena::{ptr_struct, Arena, Ptr};

// does not need generation counter
ptr_struct!(PVal());

#[derive(Clone, Copy)]
pub enum Find<P: Ptr> {
    /// Follow this to eventually find a `Root`
    Edge(P),
    /// The `PVal` points to a value in the `vals` arena. The `NonZeroUsize` is
    /// a reference count.
    Root(PVal, NonZeroUsize),
}

use Find::*;

/// A `UnionArena` is a generalization of an `Arena` that allows multiple `Ptr`s
/// to point to a single `T`. The `Find` keys are structured such that taking
/// unions is very efficient, and removal is possible through cheap reference
/// counting.
///
/// This is a more powerful version of union-find data structures, incorporating
/// a type and enabling removal.
///
/// # Note
///
/// Immutable Access to the internal keys and values is allowed for advanced use
/// cases. The associated `PVal` struct does _not_ have a generation counter and
/// has a `usize` index type.
pub struct UnionArena<P: Ptr, T> {
    keys: Arena<P, Find<P>>,
    // needs to be separate in case of large `T` and also we take advantage of smaller pointer
    // sizes
    vals: Arena<PVal, T>,
}

impl<P: Ptr, T> UnionArena<P, T> {
    /// Used by tests
    #[doc(hidden)]
    pub fn _check_invariants(this: &Self) -> Result<(), &'static str> {
        // TODO rework to check counts
        let mut encountered = 0;
        let (mut p, mut b) = this.keys.first_ptr();
        loop {
            if b {
                break
            }
            // detect loops that don't find a leaf
            let mut ok = false;
            let mut tmp0 = p;
            for _ in 0..this.keys.len() {
                match this.keys.get(tmp0) {
                    Some(Edge(e)) => {
                        tmp0 = *e;
                    }
                    Some(Root(l, _)) => {
                        if !this.vals.contains(*l) {
                            return Err("leaf in keys does not exist in this.vals")
                        }
                        encountered += 1;
                        ok = true;
                    }
                    None => return Err("broken keys list"),
                }
            }
            if !ok {
                return Err("keys has a loop")
            }
            this.keys.next_ptr(&mut p, &mut b);
        }
        // don't compare lengths directly, because we can have injective sets or other
        // such conditions
        if encountered != this.vals.len() {
            return Err("mismatch of number of vals in this.vals and vals found from keys")
        }
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            keys: Arena::new(),
            vals: Arena::new(),
        }
    }

    pub fn len_keys(&self) -> usize {
        self.keys.len()
    }

    pub fn len_vals(&self) -> usize {
        self.vals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }

    pub fn gen(&self) -> P::Gen {
        self.keys.gen()
    }

    /// If key `p` is contained in `self`
    pub fn contains(&self, p: P) -> bool {
        self.keys.contains(p)
    }

    fn get_root(&self, key: P) -> Option<(PVal, NonZeroUsize)> {
        // after the first `get` we can use unchecked indexing because of invariants
        let mut tmp = match self.keys.get(key) {
            None => return None,
            Some(p) => *p,
        };
        loop {
            match tmp {
                Edge(e) => tmp = self.keys[e],
                Root(p_val, ref_count) => break Some((p_val, ref_count)),
            }
        }
    }

    fn get_pval(&self, key: P) -> Option<PVal> {
        let mut tmp = match self.keys.get(key) {
            None => return None,
            Some(p) => *p,
        };
        loop {
            match tmp {
                Edge(e) => tmp = self.keys[e],
                Root(p_val, _) => break Some(p_val),
            }
        }
    }

    /// Inserts a new value and returns the first `Ptr` key to it.
    pub fn insert(&mut self, t: T) -> P {
        let p_val = self.vals.insert(t);
        self.keys.insert(Root(p_val, NonZeroUsize::new(1).unwrap()))
    }

    /// Adds a new key `Ptr` to the same set of keys that `key` is in, and returns the new key.
    pub fn union_key(&mut self, key: P) -> Option<P> {
        let mut tmp = match self.keys.get(key) {
            None => return None,
            Some(p) => *p,
        };
        let mut p = key;
        loop {
            match tmp {
                Edge(e) => {
                    p = e;
                    tmp = self.keys[e];
                },
                Root(_, ref mut c) => {
                    *c = NonZeroUsize::new(c.get().wrapping_add(1)).unwrap();
                    return Some(self.keys.insert(Edge(p)))
                },
            }
        }
    }

    /// Get the size of the key set that `key` is in.
    pub fn get_union_len(&self, key: P) -> Option<NonZeroUsize> {
        let mut tmp = match self.keys.get(key) {
            None => return None,
            Some(p) => *p,
        };
        loop {
            match tmp {
                Edge(e) => tmp = self.keys[e],
                Root(_, ref_count) => break Some(ref_count),
            }
        }
    }
/*
    /// Merges two sets of keys together, making the union of keys point to the `T` that `p_keep` originally pointed to, and removing the `T` that `p_remove` originally pointed to. If `p_keep` and `p_remove` are in the same set, nothing is removed and `Some(None)` is returned.
    pub fn union(&mut self, p_keep: P, p_remove: P) -> Option<Option<T>> {
        // verify containment of `p_remove`, find root of `p_keep`
        let (p_keep_val, ref_count, p_keep_root) = match self.keys.get(p_remove) {
            None => return None,
            Some(_) => {
                let mut tmp = match self.keys.get(p_keep) {
                    None => return None,
                    Some(p) => *p,
                };
                let mut p = p_keep;
                loop {
                    match tmp {
                        Edge(e) => {p = e;tmp = self.keys[e];},
                        Root(p_val, ref_count) => break (p_val, ref_count, p),
                    }
                }
            },
        };
        let mut tmp = n1;
        loop {
            match tmp {
                Edge(e) => tmp = self.keys[e],
                Leaf(l1) => break Some(l1),
            }
        }

        Some(())
    }*/

    // when
    //pub fn remove

    pub fn keys(&self) -> &Arena<P, Find<P>> {
        &self.keys
    }

    pub fn vals(&self) -> &Arena<PVal, T> {
        &self.vals
    }
}
