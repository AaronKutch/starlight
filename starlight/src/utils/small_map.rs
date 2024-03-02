use std::{cmp::Ordering, mem};

use awint::awint_dag::smallvec::{smallvec, SmallVec};

/// Binary searches `slice` with the comparator function. Assuming that `slice`
/// is ordered and `f` is consistent, finds an index that is as similar to the
/// comparator element as possible. If an equal match is found, returns the
/// index with `Ordering::Equal`. If there is not an equal match, returns an
/// index with a nonequal ordering such that if the comparator element were
/// inserted, it would be inserted inbetween the index and a neighboring index,
/// with the returned `Ordering` indicating which direction it is in. `(0,
/// Ordering::Less)` is returned if `slice.is_empty`.
///
/// Note that returns of the form `(x, Ordering::Less)` could be equivalently
/// returned as `(x - 1, Ordering::Greater)` (assuming `x > 0`), and vice versa.
/// The current implementation tends to choose the `Ordering::Less` option if it
/// can, but users should not rely on this property.
///
/// ```
/// use core::cmp::Ordering;
///
/// use starlight::utils::binary_search_similar_by;
///
/// let empty = [0u64; 0];
/// assert_eq!(
///     binary_search_similar_by(&empty, |t| t.cmp(&0)),
///     (0, Ordering::Less)
/// );
///
/// let mut v: Vec<u64> = vec![1, 2, 3, 5, 8, 13, 21, 34, 55];
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&5)),
///     (3, Ordering::Equal)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&7)),
///     (4, Ordering::Less)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&8)),
///     (4, Ordering::Equal)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&10)),
///     (5, Ordering::Less)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&13)),
///     (5, Ordering::Equal)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&17)),
///     (6, Ordering::Less)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&21)),
///     (6, Ordering::Equal)
/// );
/// assert_eq!(
///     binary_search_similar_by(&v, |t| t.cmp(&99)),
///     (8, Ordering::Greater)
/// );
/// ```
pub fn binary_search_similar_by<T, F: FnMut(&T) -> Ordering>(
    slice: &[T],
    mut f: F,
) -> (usize, Ordering) {
    if slice.is_empty() {
        return (0, Ordering::Less)
    }
    let mut size = slice.len();
    let mut left = 0;
    let mut right = size;
    while left < right {
        let mid = left + (size / 2);
        let cmp = f(&slice[mid]);
        left = if cmp == Ordering::Less { mid + 1 } else { left };
        right = if cmp == Ordering::Greater { mid } else { right };
        if cmp == Ordering::Equal {
            return (mid, Ordering::Equal);
        }

        size = right - left;
    }
    if left == slice.len() {
        // edge case this kind of binary search usually doesn't have to worry about
        (slice.len() - 1, Ordering::Greater)
    } else {
        (left, f(&slice[left]).reverse())
    }
}

/// Intended for very small (most of the time there should be no more than 8)
/// hereditary maps of keys to values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SmallMap<K, V> {
    set: SmallVec<[(K, V); 8]>,
}

impl<K, V> SmallMap<K, V> {
    pub fn new() -> Self {
        Self { set: smallvec![] }
    }

    pub fn len(&self) -> usize {
        self.set.len()
    }

    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    pub fn clear_and_shrink(&mut self) {
        self.set.clear();
        self.set.shrink_to_fit();
    }
}

impl<K: Ord, V> SmallMap<K, V> {
    /// Inserts key `k` and value `v` into the map. If `k` is equal to a key
    /// already in the map, `v` replaces the value and the old value is
    /// returned.
    pub fn insert(&mut self, k: K, v: V) -> Result<(), V> {
        let (i, direction) = binary_search_similar_by(&self.set, |(k_prime, _)| k_prime.cmp(&k));
        match direction {
            Ordering::Less => {
                self.set.insert(i, (k, v));
            }
            Ordering::Equal => return Err(mem::replace(&mut self.set[i].1, v)),
            Ordering::Greater => {
                self.set.insert(i + 1, (k, v));
            }
        }
        Ok(())
    }

    #[must_use]
    pub fn contains(&mut self, k: &K) -> bool {
        binary_search_similar_by(&self.set, |(k_prime, _)| k_prime.cmp(k)).1 == Ordering::Equal
    }

    #[must_use]
    pub fn get(&mut self, k: &K) -> Option<&V> {
        let (i, direction) = binary_search_similar_by(&self.set, |(k_prime, _)| k_prime.cmp(k));
        match direction {
            Ordering::Equal => Some(&self.set.get(i).unwrap().1),
            _ => None,
        }
    }

    #[must_use]
    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        let (i, direction) = binary_search_similar_by(&self.set, |(k_prime, _)| k_prime.cmp(k));
        match direction {
            Ordering::Equal => Some(&mut self.set.get_mut(i).unwrap().1),
            _ => None,
        }
    }

    #[must_use]
    pub fn remove(&mut self, k: &K) -> Option<V> {
        let (i, direction) = binary_search_similar_by(&self.set, |(k_prime, _)| k_prime.cmp(k));
        match direction {
            Ordering::Equal => Some(self.set.remove(i).1),
            _ => None,
        }
    }
}

impl<K, V> Default for SmallMap<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Intended for very small (most of the time there should be no more than 8)
/// hereditary sets of keys to values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SmallSet<K> {
    small_map: SmallMap<K, ()>,
}

impl<K> SmallSet<K> {
    pub fn new() -> Self {
        Self {
            small_map: SmallMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.small_map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.small_map.is_empty()
    }

    pub fn clear_and_shrink(&mut self) {
        self.small_map.clear_and_shrink();
    }
}

impl<K: Ord> SmallSet<K> {
    /// Returns whether the value was newly inserted
    pub fn insert(&mut self, k: K) -> bool {
        self.small_map.insert(k, ()).is_ok()
    }

    #[must_use]
    pub fn contains(&mut self, k: &K) -> bool {
        self.small_map.contains(k)
    }

    #[must_use]
    pub fn remove(&mut self, k: &K) -> Option<()> {
        self.small_map.remove(k)
    }
}

impl<K> Default for SmallSet<K> {
    fn default() -> Self {
        Self::new()
    }
}
