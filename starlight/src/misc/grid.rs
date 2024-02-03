use std::{
    num::NonZeroUsize,
    ops::{Index, IndexMut},
};

/// Represents a direction on a grid
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum Direction {
    /// Negative .0 direction
    Neg0,
    /// Positive .0 direction
    Pos0,
    /// Negative .1 direction
    Neg1,
    /// Positive .1 direction
    Pos1,
}

// we forbid zero length sides because they shouldn't occur for almost all
// reasonable use cases, and it causes too many edge cases that cause certain
// kinds of functions to be fallible etc

#[derive(Debug, Clone)]
pub struct Grid<T> {
    m: Box<[T]>,
    len: (NonZeroUsize, NonZeroUsize),
}

impl<T> Grid<T> {
    /// Returns `None` if any of the side lengths are zero
    pub fn new<F: Fn((usize, usize)) -> T>(len: (usize, usize), fill: F) -> Option<Self> {
        let nzlen = (NonZeroUsize::new(len.0)?, NonZeroUsize::new(len.1)?);
        // unwrap because you would be in allocation failure territory anyways
        let elen = len.0.checked_mul(len.1).unwrap();
        let mut v = Vec::with_capacity(elen);
        for j in 0..len.1 {
            for i in 0..len.0 {
                v.push(fill((i, j)));
            }
        }
        Some(Self {
            m: v.into_boxed_slice(),
            len: nzlen,
        })
    }

    #[inline]
    pub fn nzlen(&self) -> (NonZeroUsize, NonZeroUsize) {
        self.len
    }

    #[inline]
    pub fn len(&self) -> (usize, usize) {
        (self.len.0.get(), self.len.1.get())
    }

    #[must_use]
    pub fn get(&self, ij: (usize, usize)) -> Option<&T> {
        let (i, j) = (ij.0, ij.1);
        let len = self.len();
        if (i >= len.0) || (j >= len.1) {
            None
        } else {
            self.m.get(i.wrapping_add(j.wrapping_mul(len.0)))
        }
    }

    #[must_use]
    pub fn get_mut(&mut self, ij: (usize, usize)) -> Option<&mut T> {
        let (i, j) = (ij.0, ij.1);
        let len = self.len();
        if (i >= len.0) || (j >= len.1) {
            None
        } else {
            self.m.get_mut(i.wrapping_add(j.wrapping_mul(len.0)))
        }
    }

    #[must_use]
    pub fn get2(&self, ij0: (usize, usize), ij1: (usize, usize)) -> Option<(&T, &T)> {
        let (i0, j0) = (ij0.0, ij0.1);
        let (i1, j1) = (ij1.0, ij1.1);
        let len = self.len();
        if (i0 >= len.0) || (j0 >= len.1) || (i1 >= len.0) || (j1 >= len.1) {
            None
        } else {
            let inx0 = i0.wrapping_add(j0.wrapping_mul(len.0));
            let inx1 = i1.wrapping_add(j1.wrapping_mul(len.0));
            if inx0 == inx1 {
                None
            } else if inx0 < inx1 {
                let (left, right) = self.m.split_at(inx1);
                Some((&left[inx0], &right[0]))
            } else {
                let (left, right) = self.m.split_at(inx0);
                Some((&right[0], &left[inx1]))
            }
        }
    }

    #[must_use]
    pub fn get2_mut(
        &mut self,
        ij0: (usize, usize),
        ij1: (usize, usize),
    ) -> Option<(&mut T, &mut T)> {
        let (i0, j0) = (ij0.0, ij0.1);
        let (i1, j1) = (ij1.0, ij1.1);
        let len = self.len();
        if (i0 >= len.0) || (j0 >= len.1) || (i1 >= len.0) || (j1 >= len.1) {
            None
        } else {
            let inx0 = i0.wrapping_add(j0.wrapping_mul(len.0));
            let inx1 = i1.wrapping_add(j1.wrapping_mul(len.0));
            if inx0 == inx1 {
                None
            } else if inx0 < inx1 {
                let (left, right) = self.m.split_at_mut(inx1);
                Some((&mut left[inx0], &mut right[0]))
            } else {
                let (left, right) = self.m.split_at_mut(inx0);
                Some((&mut right[0], &mut left[inx1]))
            }
        }
    }

    /// Returns a reference to `self` as a flat one dimensional slice in
    /// `self.len.1` major order
    pub fn get_flat1(&self) -> &[T] {
        &self.m
    }

    pub fn get_mut_flat1(&mut self) -> &mut [T] {
        &mut self.m
    }

    pub fn for_each<F: FnMut(&T, (usize, usize))>(&self, mut f: F) {
        for j in 0..self.len().1 {
            for i in 0..self.len().0 {
                f(self.get((i, j)).unwrap(), (i, j));
            }
        }
    }

    pub fn for_each_mut<F: FnMut(&mut T, (usize, usize))>(&mut self, mut f: F) {
        for j in 0..self.len().1 {
            for i in 0..self.len().0 {
                f(self.get_mut((i, j)).unwrap(), (i, j));
            }
        }
    }

    /// For each case where there is not an orthogonal element to an element,
    /// this will call `f` with the element, its index, and direction. Corner
    /// elements are called on twice, edges once. The order is by `Direction`
    /// first, `for_each` ordering second.
    // TODO fix this attribute
    /// ```no_format
    /// use starlight::misc::{Grid, Direction::*};
    ///
    /// let grid: Grid<u64> = Grid::try_from([
    ///     [0, 1, 2, 3],
    ///     [4, 5, 6, 7],
    ///     [8, 9, 10, 11],
    /// ]).unwrap();
    ///
    /// // note 5 and 6 are skipped entirely, and the corners
    /// // have both edges called on separately
    /// let expected = [
    ///     (0, Neg0), (4, Neg0), (8, Neg0),
    ///     (3, Pos0), (7, Pos0), (11, Pos0),
    ///     (0, Neg1), (1, Neg1), (2, Neg1), (3, Neg1),
    ///     (8, Pos1), (9, Pos1), (10, Pos1), (11, Pos1)
    /// ];
    /// let mut encountered = vec![];
    /// grid.for_each_edge(|t, _, dir| encountered.push((*t, dir)));
    /// assert_eq!(expected.as_slice(), encountered.as_slice());
    /// ```
    pub fn for_each_edge<F: FnMut(&T, (usize, usize), Direction)>(&self, mut f: F) {
        let len = self.len();
        let i = 0;
        for j in 0..len.1 {
            f(self.get((i, j)).unwrap(), (i, j), Direction::Neg0);
        }
        let i = len.0 - 1;
        for j in 0..len.1 {
            f(self.get((i, j)).unwrap(), (i, j), Direction::Pos0);
        }
        let j = 0;
        for i in 0..len.0 {
            f(self.get((i, j)).unwrap(), (i, j), Direction::Neg1);
        }
        let j = len.1 - 1;
        for i in 0..len.0 {
            f(self.get((i, j)).unwrap(), (i, j), Direction::Pos1);
        }
    }

    pub fn for_each_edge_mut<F: FnMut(&mut T, (usize, usize), Direction)>(&mut self, mut f: F) {
        let len = self.len();
        let i = 0;
        for j in 0..len.1 {
            f(self.get_mut((i, j)).unwrap(), (i, j), Direction::Neg0);
        }
        let i = len.0 - 1;
        for j in 0..len.1 {
            f(self.get_mut((i, j)).unwrap(), (i, j), Direction::Pos0);
        }
        let j = 0;
        for i in 0..len.0 {
            f(self.get_mut((i, j)).unwrap(), (i, j), Direction::Neg1);
        }
        let j = len.1 - 1;
        for i in 0..len.0 {
            f(self.get_mut((i, j)).unwrap(), (i, j), Direction::Pos1);
        }
    }

    // TODO need somewhat of a fuzzing routine to test these functions against edge
    // cases

    /// For each pair of orthogonal elements in the grid (the same element can
    /// be an argument up to 4 times for each pairing with an orthogonal
    /// neighbor), this calls `f` with one element, the element's index, an
    /// element orthogonal to the first with an `ij.0 + 1` or `ij.1 + 1` offset,
    /// and a boolean indicating offset direction with `true` being the `+ij.1`
    /// direction.
    // TODO fix this attribute
    /// ```no_format
    /// use starlight::misc::Grid;
    ///
    /// let grid: Grid<u64> = Grid::try_from([
    ///     [0, 1, 2],
    ///     [3, 4, 5],
    ///     [6, 7, 8]
    /// ]).unwrap();
    ///
    /// let expected_pairs = [
    ///     (0, 1, false), (1, 2, false),
    ///     (0, 3, true), (3, 4, false), (1, 4, true), (4, 5, false), (2, 5, true),
    ///     (3, 6, true), (6, 7, false), (4, 7, true), (7, 8, false), (5, 8, true),
    /// ];
    /// let mut encountered = vec![];
    /// grid.for_each_orthogonal_pair(|t0, _, t1, dir| encountered.push((*t0, *t1, dir)));
    /// assert_eq!(expected_pairs.as_slice(), encountered.as_slice());
    /// ```
    pub fn for_each_orthogonal_pair<F: FnMut(&T, (usize, usize), &T, bool)>(&self, mut f: F) {
        let len = self.len();
        let j = 0;
        for i in 1..len.0 {
            let (t0, t1) = self.get2((i - 1, j), (i, j)).unwrap();
            f(t0, (i - 1, j), t1, false);
        }
        for j in 1..len.1 {
            let i = 0;
            let (t0, t1) = self.get2((i, j - 1), (i, j)).unwrap();
            f(t0, (i, j - 1), t1, true);
            // nonedge cases
            for i in 1..len.0 {
                let (t0, t1) = self.get2((i - 1, j), (i, j)).unwrap();
                f(t0, (i - 1, j), t1, false);
                let (t0, t1) = self.get2((i, j - 1), (i, j)).unwrap();
                f(t0, (i, j - 1), t1, true);
            }
        }
    }

    pub fn for_each_orthogonal_pair_mut<F: FnMut(&mut T, (usize, usize), &mut T, bool)>(
        &mut self,
        mut f: F,
    ) {
        let len = self.len();
        let j = 0;
        for i in 1..len.0 {
            let (t0, t1) = self.get2_mut((i - 1, j), (i, j)).unwrap();
            f(t0, (i - 1, j), t1, false);
        }
        for j in 1..len.1 {
            let i = 0;
            let (t0, t1) = self.get2_mut((i, j - 1), (i, j)).unwrap();
            f(t0, (i, j - 1), t1, true);
            // nonedge cases
            for i in 1..len.0 {
                let (t0, t1) = self.get2_mut((i - 1, j), (i, j)).unwrap();
                f(t0, (i - 1, j), t1, false);
                let (t0, t1) = self.get2_mut((i, j - 1), (i, j)).unwrap();
                f(t0, (i, j - 1), t1, true);
            }
        }
    }
}

impl<T, const N: usize, const M: usize> TryFrom<[[T; N]; M]> for Grid<T> {
    type Error = ();

    /// Returns an error if `N` or `M` are zero
    fn try_from(grid: [[T; N]; M]) -> Result<Self, Self::Error> {
        if let (Some(nzlen0), Some(nzlen1)) = (NonZeroUsize::new(N), NonZeroUsize::new(M)) {
            let elen = N.checked_mul(M).unwrap();
            let mut v = Vec::with_capacity(elen);
            for row in grid {
                for e in row {
                    v.push(e);
                }
            }
            Ok(Self {
                m: v.into_boxed_slice(),
                len: (nzlen0, nzlen1),
            })
        } else {
            Err(())
        }
    }
}

impl<T> Index<(usize, usize)> for Grid<T> {
    type Output = T;

    fn index(&self, i: (usize, usize)) -> &T {
        self.get(i).unwrap()
    }
}

impl<T> IndexMut<(usize, usize)> for Grid<T> {
    fn index_mut(&mut self, i: (usize, usize)) -> &mut T {
        self.get_mut(i).unwrap()
    }
}
