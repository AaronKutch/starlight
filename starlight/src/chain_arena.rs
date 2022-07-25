use std::{
    borrow::Borrow,
    fmt,
    ops::{Deref, DerefMut, Index, IndexMut},
};

use triple_arena::{Arena, Ptr, PtrTrait};

// TODO is it possible to break the arena externally with `mem::swap`?

/// This represents a link in a `ChainArena` that has a public `t: T` field and
/// `Option<Ptr<PLink>>` interlinks to the previous and next links. Note that
/// `Deref` and `DerefMut` are implemented to grant automatic access to the
/// methods on `T`. The interlinks are private and only accessible through
/// methods so that the whole `Link` can be returned by indexing the arena
/// (preventing a lot of cumbersome code when traversing chains).
#[derive(Clone, Copy)]
pub struct Link<PLink: PtrTrait, T> {
    // I think the code gen should be overall better if this is done
    prev_next: (Option<Ptr<PLink>>, Option<Ptr<PLink>>),
    pub t: T,
}

impl<PLink: PtrTrait, T> Link<PLink, T> {
    #[doc(hidden)]
    pub fn new(prev_next: (Option<Ptr<PLink>>, Option<Ptr<PLink>>), t: T) -> Self {
        Self { prev_next, t }
    }

    pub fn prev_next(this: &Link<PLink, T>) -> (Option<Ptr<PLink>>, Option<Ptr<PLink>>) {
        this.prev_next
    }

    pub fn prev(this: &Link<PLink, T>) -> Option<Ptr<PLink>> {
        this.prev_next.0
    }

    pub fn next(this: &Link<PLink, T>) -> Option<Ptr<PLink>> {
        this.prev_next.1
    }
}

impl<PLink: PtrTrait, T> Deref for Link<PLink, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<PLink: PtrTrait, T> DerefMut for Link<PLink, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}

/// Able to cheaply insert and delete in the middle of a string of nodes
pub struct ChainArena<PLink: PtrTrait, T> {
    a: Arena<PLink, Link<PLink, T>>,
}

impl<PLink: PtrTrait, T> ChainArena<PLink, T> {
    #[doc(hidden)]
    pub fn _check_invariants(this: &Self) -> Result<(), &'static str> {
        for (p, link) in &this.a {
            if let Some(prev) = Link::prev(link) {
                if let Some(prev) = this.a.get(prev) {
                    if let Some(next) = Link::next(prev) {
                        if p != next {
                            return Err("interlink does not correspond")
                        }
                    } else {
                        return Err("next node does not exist")
                    }
                } else {
                    return Err("prev node does not exist")
                }
            }
            // there are going to be duplicate checks but this must be done for invariant
            // breaking cases
            if let Some(next) = Link::next(link) {
                if let Some(next) = this.a.get(next) {
                    if let Some(prev) = Link::prev(next) {
                        if p != prev {
                            return Err("interlink does not correspond")
                        }
                    } else {
                        return Err("prev node does not exist")
                    }
                } else {
                    return Err("next node does not exist")
                }
            }
        }
        Ok(())
    }

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
    pub fn insert(
        &mut self,
        prev_next: (Option<Ptr<PLink>>, Option<Ptr<PLink>>),
        t: T,
    ) -> Option<Ptr<PLink>> {
        match prev_next {
            // new chain
            (None, None) => Some(self.a.insert(Link::new((None, None), t))),
            (None, Some(p1)) => {
                // if there is a failure it cannot result in a node being inserted
                if let Some(p0) = Link::prev(self.a.get_mut(p1)?) {
                    // insert into middle of chain
                    let res = Some(self.a.insert(Link::new((Some(p0), Some(p1)), t)));
                    self.a.get_mut(p0).unwrap().prev_next.1 = res;
                    self.a.get_mut(p1).unwrap().prev_next.0 = res;
                    res
                } else {
                    let res = Some(self.a.insert(Link::new((None, Some(p1)), t)));
                    self.a.get_mut(p1).unwrap().prev_next.0 = res;
                    res
                }
            }
            (Some(p0), None) => {
                if let Some(p1) = Link::next(self.a.get_mut(p0)?) {
                    // insert into middle of chain
                    let res = Some(self.a.insert(Link::new((Some(p0), Some(p1)), t)));
                    self.a.get_mut(p0).unwrap().prev_next.1 = res;
                    self.a.get_mut(p1).unwrap().prev_next.0 = res;
                    res
                } else {
                    let res = Some(self.a.insert(Link::new((Some(p0), None), t)));
                    self.a.get_mut(p0).unwrap().prev_next.1 = res;
                    res
                }
            }
            (Some(p0), Some(p1)) => {
                // check for existence and that the nodes are neighbors
                let mut err = true;
                if let Some(l0) = self.a.get(p0) {
                    if let Some(next) = Link::next(l0) {
                        if next == p1 {
                            // `p1` must implicitly exist if the invariants hold
                            err = false;
                        }
                    }
                }
                if err {
                    return None
                }
                let res = Some(self.a.insert(Link::new((Some(p0), Some(p1)), t)));
                self.a.get_mut(p0).unwrap().prev_next.1 = res;
                self.a.get_mut(p1).unwrap().prev_next.0 = res;
                res
            }
        }
    }

    /// Inserts `t` as a single link in a new chain
    pub fn insert_new(&mut self, t: T) -> Ptr<PLink> {
        self.a.insert(Link::new((None, None), t))
    }

    // in case we want to spin this off into its own crate we should actively
    // support this
    /// Inserts `t` as a single link cyclical chain and returns a `Ptr` to it
    pub fn insert_new_cyclic(&mut self, t: T) -> Ptr<PLink> {
        self.a.insert_with(|p| Link::new((Some(p), Some(p)), t))
    }

    /// Inserts `t` as a new link at the end of a chain which has `p` as its
    /// last link. Returns `None` if `p` is not valid or is not the end of a
    /// chain
    pub fn insert_last(&mut self, p: Ptr<PLink>, t: T) -> Option<Ptr<PLink>> {
        let p0 = p;
        if Link::next(self.a.get_mut(p0)?).is_some() {
            // not at end of chain
            None
        } else {
            let res = Some(self.a.insert(Link::new((Some(p0), None), t)));
            self.a.get_mut(p0).unwrap().prev_next.1 = res;
            res
        }
    }

    /// Removes the link at `p`. The `prev` and `next` `Ptr`s in the link will
    /// be valid `Ptr`s to neighboring links in the chain. Returns `None` if `p`
    /// is not valid.
    pub fn remove(&mut self, p: Ptr<PLink>) -> Option<Link<PLink, T>> {
        let l = self.a.remove(p)?;
        match Link::prev_next(&l) {
            (None, None) => (),
            (None, Some(p1)) => {
                self.a.get_mut(p1)?.prev_next.0 = None;
            }
            (Some(p0), None) => {
                self.a.get_mut(p0)?.prev_next.1 = None;
            }
            (Some(p0), Some(p1)) => {
                if p != p0 {
                    self.a.get_mut(p0)?.prev_next.1 = Some(p1);
                    self.a.get_mut(p1)?.prev_next.0 = Some(p0);
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

    // TODO add Arena::swap so this can be done efficiently
    /*pub fn swap(&self, p0: Ptr<PLink>, p1: Ptr<PLink>) -> Option<()> {
    }*/

    pub fn get_arena(&self) -> &Arena<PLink, Link<PLink, T>> {
        &self.a
    }
}

impl<P: PtrTrait, T, B: Borrow<Ptr<P>>> Index<B> for ChainArena<P, T> {
    type Output = Link<P, T>;

    fn index(&self, index: B) -> &Self::Output {
        self.a.get(*index.borrow()).unwrap()
    }
}

impl<P: PtrTrait, T, B: Borrow<Ptr<P>>> IndexMut<B> for ChainArena<P, T> {
    fn index_mut(&mut self, index: B) -> &mut Self::Output {
        self.a.get_mut(*index.borrow()).unwrap()
    }
}

impl<P: PtrTrait, T: fmt::Debug> fmt::Debug for Link<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({:?}, {:?}) {:?}",
            Link::prev(self),
            Link::next(self),
            self.t
        )
    }
}

impl<P: PtrTrait, T: fmt::Display> fmt::Display for Link<P, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "({:?}, {:?}) {}",
            Link::prev(self),
            Link::next(self),
            self.t
        )
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
