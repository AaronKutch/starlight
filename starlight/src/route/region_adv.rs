use std::{cmp::Ordering, marker::PhantomData};

use awint::awint_dag::triple_arena::{Advancer, OrdArena, Ptr};

// TODO may want to add to `triple_arena`, also add a `find_similar_with` to fix
// the issue with needing to rewind

pub struct RegionAdvancer<F: FnMut(P, &K, &V) -> Ordering, P: Ptr, K, V> {
    p: Option<P::Inx>,
    cmp: F,
    boo: PhantomData<fn() -> (K, V)>,
}

impl<F: FnMut(P, &K, &V) -> Ordering, P: Ptr, K, V> Advancer for RegionAdvancer<F, P, K, V> {
    type Collection = OrdArena<P, K, V>;
    type Item = P;

    fn advance(&mut self, collection: &Self::Collection) -> Option<Self::Item> {
        // go in the `next` direction
        if let Some(current) = self.p {
            let (gen, link) = collection.get_link_no_gen(current).unwrap();
            self.p = link.next();
            let p_current = P::_from_raw(current, gen);
            if (self.cmp)(p_current, &link.t.0, &link.t.1) == Ordering::Equal {
                Some(p_current)
            } else {
                // have reached the end of the region, also set to `None` to shortcut in
                // case this advancer is used after `None` was first reached
                self.p = None;
                None
            }
        } else {
            None
        }
    }
}

impl<F: FnMut(P, &K, &V) -> Ordering, P: Ptr, K, V> RegionAdvancer<F, P, K, V> {
    /// Sometimes when advancing over an `OrdArena`, there is a contiguous
    /// subset of keys that are equal with respect to some common prefix, and we
    /// want to advance over all of them. This will return an advancer if it
    /// finds at least one `Ordering::Equal` case from the `cmp` function,
    /// otherwise it returns `None`. The advancer will then return all `Ptr`s to
    /// keys from the region that compare as `Ordering::Equal` with the same
    /// `cmp` function. Additionally, it returns `Ptr`s in order.
    pub fn new(collection: &OrdArena<P, K, V>, mut cmp: F) -> Option<Self> {
        if let Some(p) = collection.find_with(|p, k, v| cmp(p, k, v)) {
            let mut p = p.inx();
            loop {
                let (gen, link) = collection.get_link_no_gen(p).unwrap();
                if cmp(Ptr::_from_raw(p, gen), &link.t.0, &link.t.1) == Ordering::Equal {
                    if let Some(p_prev) = link.prev() {
                        p = p_prev;
                    } else {
                        // region is the first
                        break
                    }
                } else {
                    // we would exit the region
                    break
                }
            }
            Some(RegionAdvancer {
                p: Some(p),
                cmp,
                boo: PhantomData,
            })
        } else {
            None
        }
    }
}
