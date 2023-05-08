// TODO eventually include in main `triple_arena` crate

use std::{mem, num::NonZeroUsize};

use crate::triple_arena::{ptr_struct, Arena, ChainArena, Link, Ptr};

// does not need generation counter
ptr_struct!(PVal());

struct Val<T> {
    t: T,
    key_count: NonZeroUsize,
}

/// Used for organization of two mutable values
pub struct KeepRemove<'a, 'b, T> {
    pub t_keep: &'a mut T,
    pub t_remove: &'b mut T,
}

/// A `SurjectArena` is a generalization of an `Arena` that allows multiple
/// `Ptr`s to point to a single `T`. The `Find` keys are structured such that
/// taking unions is very efficient, and removal is possible through cheap
/// reference counting.
///
/// This is a more powerful version of union-find data structures, incorporating
/// a type and enabling removal.
///
/// # Note
///
/// Immutable Access to the internal keys and values is allowed for advanced use
/// cases. The associated `PVal` struct does _not_ have a generation counter and
/// has a `usize` index type.
pub struct SurjectArena<P: Ptr, T> {
    keys: ChainArena<P, PVal>,
    // the `usize` is the key reference count, we ultimately need it for efficient unions and it
    // has the bonus of being able to know key chain lengths
    vals: Arena<PVal, Val<T>>,
}

impl<P: Ptr, T> SurjectArena<P, T> {
    /// Used by tests
    #[doc(hidden)]
    pub fn _check_invariants(this: &Self) -> Result<(), &'static str> {
        // there should be exactly one key chain associated with each val
        let mut count = Arena::<PVal, usize>::new();
        for key in this.keys.vals() {
            match count.get_mut(key.t) {
                Some(p) => *p += 1,
                None => return Err("key points to nonexistent val"),
            }
        }
        for (p_val, n) in &count {
            if this.vals[p_val].key_count.get() != *n {
                return Err("key count does not match actual")
            }
        }

        let (mut p, mut b) = this.keys.first_ptr();
        loop {
            if b {
                break
            }
            let mut c = count[this.keys[p].t];
            if c != 0 {
                // upon encountering a nonzero count for the first time, we follow the chain and
                // count down, and if we reach back to the beginning (verifying cyclic chain)
                // and reach a count of zero, then we know that the chain encountered all the
                // needed keys. Subsequent encounters with the rest of the chain is ignored
                // because the count is zeroed afterwards.
                let mut tmp = p;
                loop {
                    c -= 1;
                    match this.keys.get(tmp) {
                        Some(link) => {
                            if let Some(next) = Link::next(link) {
                                tmp = next;
                            } else {
                                return Err("key chain is not cyclic")
                            }
                        }
                        None => {
                            // should not be possible unless `ChainArena` itself is broken
                            return Err("broken chain")
                        }
                    }
                    // have the test after the match so that we check for single node cyclics
                    if tmp == p {
                        if c != 0 {
                            return Err("key chain did not have all keys associated with value")
                        }
                        count[this.keys[p].t] = 0;
                        break
                    }
                }
            }
            this.keys.next_ptr(&mut p, &mut b);
        }
        Ok(())
    }

    pub fn new() -> Self {
        Self {
            keys: ChainArena::new(),
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

    /// Returns the size of the set of keys pointing to a value, with `p` being
    /// one of those keys
    pub fn key_set_len(&self, p: P) -> Option<NonZeroUsize> {
        let p_val = self.keys.get(p)?;
        Some(self.vals[p_val.t].key_count)
    }

    /// Inserts a new value and returns the first `Ptr` key to it.
    pub fn insert(&mut self, t: T) -> P {
        let p_val = self.vals.insert(Val {
            t,
            key_count: NonZeroUsize::new(1).unwrap(),
        });
        self.keys.insert_new_cyclic(p_val)
    }

    /// Adds a new `Ptr` key to the same set of keys that `p` is in, and returns
    /// the new key.
    pub fn add_key(&mut self, p: P) -> Option<P> {
        let link = match self.keys.get(p) {
            None => return None,
            Some(p) => *p,
        };
        self.vals[link.t].key_count =
            NonZeroUsize::new(self.vals[link.t].key_count.get().wrapping_add(1)).unwrap();
        Some(self.keys.insert((Some(p), None), link.t).unwrap())
    }

    pub fn get(&mut self, p: P) -> Option<&T> {
        let link = match self.keys.get(p) {
            None => return None,
            Some(p) => *p,
        };
        Some(&self.vals[link.t].t)
    }

    /// Given `p0` and `p1` pointing to different `T` values, this function will
    /// choose to keep one of the `T` values (accessible as `t_keep`) and remove
    /// the other `T` value (accessible as `t_remove` ). # Note
    ///
    /// The order of `t_keep` and `t_remove` does not correspond to `p0` and
    /// `p1`. In order to enforce O(log n) efficiency, `union` may select either
    /// the `T` corresponding to `p0` or `p1` when choosing which `T` to keep in
    /// the arena for both sets of keys to point to
    pub fn union<F: FnMut(KeepRemove<T>)>(&mut self, mut p0: P, mut p1: P, mut f: F) -> Option<T> {
        let mut p_link0 = *self.keys.get(p0)?;
        let mut p_link1 = *self.keys.get(p1)?;
        if p_link0.t == p_link1.t {
            // corresponds to same set
            return None
        }
        let len0 = self.vals[p_link0.t].key_count.get();
        let len1 = self.vals[p_link1.t].key_count.get();
        if len0 > len1 {
            mem::swap(&mut p_link0, &mut p_link1);
            mem::swap(&mut p0, &mut p1);
        }
        let mut t_remove = self.vals.remove(p_link1.t).unwrap().t;
        let keep_remove = KeepRemove {
            t_keep: &mut self.vals[p_link0.t].t,
            t_remove: &mut t_remove,
        };
        f(keep_remove);

        // first, overwrite the `PVal`s in key chain 1
        let mut tmp = p1;
        loop {
            self.keys[tmp].t = p_link0.t;
            tmp = Link::next(&self.keys[tmp]).unwrap();
            if tmp == p1 {
                break
            }
        }
        // combine chains cheaply
        self.keys.exchange_next(p0, p1).unwrap();
        // it is be impossible to overflow this, it would mean that we have already
        // inserted `usize + 1` elements
        self.vals[p_link0.t].key_count = NonZeroUsize::new(len0.wrapping_add(len1)).unwrap();
        Some(t_remove)
    }
}
