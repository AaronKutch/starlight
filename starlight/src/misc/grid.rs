use std::{
    num::NonZeroUsize,
    ops::{Index, IndexMut},
};

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

    pub fn get(&self, ij: (usize, usize)) -> Option<&T> {
        let (i, j) = (ij.0, ij.1);
        let len = self.len();
        if (i >= len.0) || (j >= len.1) {
            None
        } else {
            self.m.get(i.wrapping_add(j.wrapping_mul(len.0)))
        }
    }

    pub fn get_mut(&mut self, ij: (usize, usize)) -> Option<&mut T> {
        let (i, j) = (ij.0, ij.1);
        let len = self.len();
        if (i >= len.0) || (j >= len.1) {
            None
        } else {
            self.m.get_mut(i.wrapping_add(j.wrapping_mul(len.0)))
        }
    }

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
    ///     (0, 1), (1, 2),
    ///     (0, 3), (3, 4), (1, 4), (4, 5), (2, 5),
    ///     (3, 6), (6, 7), (4, 7), (7, 8), (5, 8),
    /// ];
    /// let mut encountered = vec![];
    /// grid.for_each_orthogonal_pair(|t0, _, t1, _| encountered.push((*t0, *t1)));
    /// assert_eq!(expected_pairs.as_slice(), encountered.as_slice());
    /// ```
    pub fn for_each_orthogonal_pair<F: FnMut(&T, (usize, usize), &T, (usize, usize))>(
        &self,
        mut f: F,
    ) {
        // j == 0 row
        for i in 1..self.len().0 {
            let (t0, t1) = self.get2((i - 1, 0), (i, 0)).unwrap();
            f(t0, (i - 1, 0), t1, (i, 0));
        }
        for j in 1..self.len().1 {
            // i == 0 column element
            let (t0, t1) = self.get2((0, j - 1), (0, j)).unwrap();
            f(t0, (0, j - 1), t1, (0, j));
            // nonedge cases
            for i in 1..self.len().0 {
                let (t0, t1) = self.get2((i - 1, j), (i, j)).unwrap();
                f(t0, (i - 1, j), t1, (i, j));
                let (t0, t1) = self.get2((i, j - 1), (i, j)).unwrap();
                f(t0, (i, j - 1), t1, (i, j));
            }
        }
    }

    pub fn for_each_orthogonal_pair_mut<
        F: FnMut(&mut T, (usize, usize), &mut T, (usize, usize)),
    >(
        &mut self,
        mut f: F,
    ) {
        // j == 0 row
        for i in 1..self.len().0 {
            let (t0, t1) = self.get2_mut((i - 1, 0), (i, 0)).unwrap();
            f(t0, (i - 1, 0), t1, (i, 0));
        }
        for j in 1..self.len().1 {
            // i == 0 column element
            let (t0, t1) = self.get2_mut((0, j - 1), (0, j)).unwrap();
            f(t0, (0, j - 1), t1, (0, j));
            // nonedge cases
            for i in 1..self.len().0 {
                let (t0, t1) = self.get2_mut((i - 1, j), (i, j)).unwrap();
                f(t0, (i - 1, j), t1, (i, j));
                let (t0, t1) = self.get2_mut((i, j - 1), (i, j)).unwrap();
                f(t0, (i, j - 1), t1, (i, j));
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
