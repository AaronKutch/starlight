use std::mem;

use awint::awint_dag::smallvec::{smallvec, SmallVec};

/// Intended for very small (most of the time there should be no more than 8)
/// hereditary maps of keys to values.
pub struct SmallMap<K, V> {
    set: SmallVec<[(K, V); 8]>,
}

impl<K, V> SmallMap<K, V> {
    pub fn new() -> Self {
        Self { set: smallvec![] }
    }
}

impl<K: Ord, V> SmallMap<K, V> {
    /// Inserts key `k` and value `v` into the map. If `k` is equal to a key
    /// already in the map, `v` replaces the value and the old value is
    /// returned.
    pub fn insert(&mut self, k: K, v: V) -> Result<(), V> {
        // low number of branches

        // TODO: this should have a conditional switch to using vec insertion and binary
        // searching for large lengths before we make `SmallMap` public
        for (k1, v1) in &mut self.set {
            if *k1 == k {
                return Err(mem::replace(v1, v))
            }
        }
        self.set.push((k, v));
        Ok(())
    }

    /*pub fn get_mut(&mut self, k: K) -> Option<&mut V> {
        for (k1, v1) in &mut self.set {
            if *k1 == k {
                return Some(v1)
            }
        }
        None
    }*/
}

impl<K, V> Default for SmallMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
