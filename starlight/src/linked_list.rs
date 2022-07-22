use std::fmt;

use triple_arena::{Arena, Ptr, PtrTrait};

pub struct Link<PLink: PtrTrait, T> {
    pub t: T,
    pub prev: Option<Ptr<PLink>>,
    pub next: Option<Ptr<PLink>>,
}

impl<PLink: PtrTrait, T> Link<PLink, T> {
    pub fn prev_next(&self) -> (Option<Ptr<PLink>>, Option<Ptr<PLink>>) {
        (self.prev, self.next)
    }
}

/// Able to cheaply insert and delete in the middle of a string of nodes
pub struct ChainArena<PLink: PtrTrait, T> {
    a: Arena<PLink, Link<PLink, T>>,
}

impl<PLink: PtrTrait, T> ChainArena<PLink, T> {
    pub fn new() -> Self {
        Self { a: Arena::new() }
    }

    /// If `link.prev.is_none() && link.next.is_none()` then a new chain is
    /// started in the arena. If `link.prev.is_some() || link.next.is_some()`
    /// then the link is inserted in a chain and the neighboring links are
    /// rerouted to be consistent. `link.prev.is_some() && link.next.is_none()`
    /// and the reverse is allowed even if the link is not at the end of the
    /// chain. If a pointer is not contained in the arena, or the `prev` and
    /// `next` nodes are farther than one node apart, then `None` is returned.
    pub fn insert(&mut self, link: Link<PLink, T>) -> Option<Ptr<PLink>> {
        let tmp = link.prev_next();
        let p = self.a.insert(link);
        match tmp {
            // new chain
            (None, None) => (),
            (None, Some(p1)) => {
                let l1 = self.a.get_mut(p1)?;
                if let Some(p0) = l1.prev {
                    // not at start of chain
                    l1.prev = Some(p);
                    let l0 = self.a.get_mut(p0).unwrap();
                    l0.next = Some(p);
                } else {
                    l1.prev = Some(p);
                }
            }
            (Some(p0), None) => {
                let l0 = self.a.get_mut(p0)?;
                if let Some(p1) = l0.next {
                    // not at end of chain
                    l0.next = Some(p);
                    let l1 = self.a.get_mut(p1).unwrap();
                    l1.prev = Some(p);
                } else {
                    l0.next = Some(p);
                }
            }
            (Some(p0), Some(p1)) => {
                let l0 = self.a.get_mut(p0)?;
                let next = l0.next?;
                if next != p1 {
                    // the links are not neighbors
                    return None
                }
                // the single link circular chain works with this order
                l0.next = Some(p);
                let l1 = self.a.get_mut(p1).unwrap();
                l1.prev = Some(p);
            }
        }
        Some(p)
    }

    /// Inserts `t` as a single link in a new chain
    pub fn insert_new(&mut self, t: T) -> Ptr<PLink> {
        self.a.insert(Link {
            t,
            prev: None,
            next: None,
        })
    }

    // in case we want to spin this off into its own crate we should actively
    // support this
    /// Inserts `t` as a single link cyclical chain and returns a `Ptr` to it
    pub fn insert_new_cyclic(&mut self, t: T) -> Ptr<PLink> {
        self.a.insert_with(|p| Link {
            t,
            prev: Some(p),
            next: Some(p),
        })
    }

    /// Inserts `t` as a new link at the end of a chain which has `p` as its
    /// last link. Returns `None` if `p` is not valid or is not the end of a
    /// chain
    pub fn insert_last(&mut self, p: Ptr<PLink>, t: T) -> Option<Ptr<PLink>> {
        let l0 = self.a.get(p)?;
        if l0.next.is_some() {
            // not at end of chain
            None
        } else {
            let p1 = self.a.insert(Link {
                t,
                prev: Some(p),
                next: None,
            });
            self.a[p].next = Some(p1);
            Some(p1)
        }
    }

    /// Removes the link at `p`. The `prev` and `next` `Ptr`s in the link will
    /// be valid `Ptr`s to neighboring links in the chain. Returns `None` if `p`
    /// is not valid.
    pub fn remove(&mut self, p: Ptr<PLink>) -> Option<Link<PLink, T>> {
        let l = self.a.remove(p)?;
        match l.prev_next() {
            (None, None) => (),
            (None, Some(p1)) => {
                let l1 = self.a.get_mut(p1)?;
                l1.prev = None;
            }
            (Some(p0), None) => {
                let l0 = self.a.get_mut(p0)?;
                l0.next = None;
            }
            (Some(p0), Some(p1)) => {
                if p != p0 {
                    let l0 = self.a.get_mut(p0)?;
                    l0.next = Some(p1);
                    let l1 = self.a.get_mut(p1)?;
                    l1.next = Some(p0);
                } // else it is a single link circular chain
            }
        }
        Some(l)
    }

    // exchanges the endpoints of the interlinks right after two given nodes
    // note: if the two interlinks are adjacent, there is a special case where the
    // middle node becomes a single link circular chain and the first node
    // interlinks to the last node. It is its own inverse like the other cases so it
    // appears to be the correct behavior.
    //pub fn exchange(&mut self, p0, p1)

    //pub fn break(&mut self, p)

    //pub fn connect(&mut self, p0, p1)

    pub fn get_arena(&self) -> &Arena<PLink, Link<PLink, T>> {
        &self.a
    }
}

impl<P: PtrTrait, T: fmt::Debug> fmt::Debug for Link<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.t)
    }
}

impl<P: PtrTrait, T: Clone> Clone for Link<P, T> {
    fn clone(&self) -> Self {
        Self {
            t: self.t.clone(),
            prev: self.prev,
            next: self.next,
        }
    }
}

impl<P: PtrTrait, T: fmt::Debug> fmt::Debug for ChainArena<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.a)
    }
}

impl<P: PtrTrait, T: Clone> Clone for ChainArena<P, T> {
    fn clone(&self) -> Self {
        Self { a: self.a.clone() }
    }
}

impl<PLink: PtrTrait, T> Default for ChainArena<PLink, T> {
    fn default() -> Self {
        Self::new()
    }
}
