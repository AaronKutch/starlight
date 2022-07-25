use std::{
    fmt::{self, Write},
    num::NonZeroUsize,
};

use awint::{Bits, ExtAwi, InlAwi};

const BITS: usize = usize::BITS as usize;
const MAX: usize = usize::MAX;

/// A permutation lookup table.
///
/// A permutation lookup table has the properties
/// that:
/// - There is a nonzero integer `n` for the number of input bits
/// - The number of input or index bits is equal to the number of output bits
/// - There are `l = 2^n` entries, one for each possible input
/// - The entries include all `2^n` integers `0..2^n` exactly once
#[derive(Clone, PartialEq, Eq)]
pub struct Perm {
    /// The number of index bits
    nz_n: NonZeroUsize,
    /// The lookup table
    lut: ExtAwi,
}

// TODO use fully constant structures and optimizations for small lookup tables
// up to n = 4 at least

impl Perm {
    /// Identity permutation. Returns `None` if `n == 0` or there is some kind
    /// of memory overflow.
    pub fn ident(n: NonZeroUsize) -> Option<Self> {
        if n.get() >= BITS {
            return None
        }
        let l = 1 << n.get();
        let lut = ExtAwi::zero(NonZeroUsize::new(n.get().checked_mul(l)?)?);
        let mut res = Self { nz_n: n, lut };
        res.ident_assign();
        Some(res)
    }

    pub fn from_raw(nz_n: NonZeroUsize, lut: ExtAwi) -> Self {
        Self { nz_n, lut }
    }

    /// The index bitwidth
    pub const fn nz_n(&self) -> NonZeroUsize {
        self.nz_n
    }

    /// The index bitwidth
    pub const fn n(&self) -> usize {
        self.nz_n.get()
    }

    /// The number of entries
    pub fn nz_l(&self) -> NonZeroUsize {
        NonZeroUsize::new(1 << self.nz_n.get()).unwrap()
    }

    /// The number of entries
    pub fn l(&self) -> usize {
        self.nz_l().get()
    }

    /// A mask of `n` set bits
    const fn mask(&self) -> usize {
        MAX >> (BITS - self.n())
    }

    /// Assigns the entry corresponding to `inx` to `out`. Returns `None` if
    /// `inx.bw() != self.n()` or `out.bw() != self.n()`.
    // use a distinct signature from `Bits::lut` because we can't have `out` on the
    // left hand side without still calling `self`.
    pub fn lut(out: &mut Bits, this: &Self, inx: &Bits) -> Option<()> {
        out.lut(&this.lut, inx)
    }

    /// Gets the `i`th entry and returns it. Returns `None` if `i >= self.l()`.
    pub fn get(&self, i: usize) -> Option<usize> {
        if i >= self.l() {
            return None
        }
        Some(self.lut.get_digit(i * self.n()) & self.mask())
    }

    /// Used in the algorithms to do unchecked sets of entries.
    fn set(&mut self, i: usize, x: usize) {
        let x = InlAwi::from_usize(x);
        let n = self.n();
        self.lut.field_to(i * n, &x, n).unwrap();
    }

    /// Sets the `i`th entry to `x`. Returns `None` if `i >= self.l()`.
    ///
    /// # Note
    ///
    /// This can break the permutation property if not used properly, and `x` is
    /// not masked by the function.
    pub fn unstable_set(&mut self, i: usize, x: usize) -> Option<()> {
        if i >= self.l() {
            None
        } else {
            self.set(i, x);
            Some(())
        }
    }

    /// Assigns the identity permutation to `self`
    pub fn ident_assign(&mut self) {
        for i in 0..self.l() {
            self.set(i, i);
        }
    }

    /// Swap entries `i0` and `i1`. Returns `None` if `i0 >= self.l()` or
    /// `i1 >= self.l()`. Equivalent to swapping rows of the matrix form.
    pub fn swap(&mut self, i0: usize, i1: usize) -> Option<()> {
        // the check is performed by the `get` calls
        if i0 == i1 {
            return Some(())
        }
        let tmp0 = self.get(i0)?;
        let tmp1 = self.get(i1)?;
        self.set(i0, tmp1);
        self.set(i1, tmp0);
        Some(())
    }

    /// Swap the entries that have values `e0` and `e1`. Returns `None` if
    /// `e0 >= self.l()` or `e1 >= self.l()`. Equivalent to swapping columns of
    /// the matrix form.
    pub fn t_swap(&mut self, e0: usize, e1: usize) -> Option<()> {
        if (e0 >= self.l()) || (e1 >= self.l()) {
            return None
        }
        if e0 == e1 {
            return Some(())
        }
        for i0 in 0..self.l() {
            let e = self.get(i0).unwrap();
            if e == e0 {
                for i1 in (i0 + 1)..self.l() {
                    if self.get(i1).unwrap() == e1 {
                        self.swap(i0, i1).unwrap();
                        return Some(())
                    }
                }
            } else if e == e1 {
                for i1 in (i0 + 1)..self.l() {
                    if self.get(i1).unwrap() == e0 {
                        self.swap(i0, i1).unwrap();
                        return Some(())
                    }
                }
            }
        }
        None
    }

    /// Performs an unbiased permutation of `self` using `rng`
    pub fn rand_assign_with<R: rand_xoshiro::rand_core::RngCore>(&mut self, rng: &mut R) {
        // prevent previous state from affecting this
        self.ident_assign();
        for i in 0..self.l() {
            self.swap(i, (rng.next_u64() as usize) % self.l()).unwrap();
        }
    }

    /// Copies the permutation of `rhs` to `self`
    pub fn copy_assign(&mut self, rhs: &Self) -> Option<()> {
        if self.n() != rhs.n() {
            None
        } else {
            self.lut.copy_assign(&rhs.lut)
        }
    }

    /// Inversion, equivalent to matrix transpose. Returns `None` if `self.n()
    /// != rhs.n()`.
    pub fn inv_assign(&mut self, rhs: &Self) -> Option<()> {
        if self.n() != rhs.n() {
            None
        } else {
            for i in 0..self.l() {
                self.set(rhs.get(i).unwrap(), i);
            }
            Some(())
        }
    }

    /// Inversion, equivalent to matrix transpose.
    pub fn inv(&self) -> Self {
        let mut res = Self::ident(self.nz_n()).unwrap();
        res.inv_assign(self).unwrap();
        res
    }

    /// Assigns the composition of permutation `lhs` followed by `rhs` to
    /// `self`. Returns `None` if `self.n() != lhs.n()` or `self.n() !=
    /// rhs.n()`.
    pub fn mul_copy_assign(&mut self, lhs: &Self, rhs: &Self) -> Option<()> {
        let n = self.n();
        if (n != lhs.n()) || (n != rhs.n()) {
            return None
        }
        for i in 0..self.l() {
            let e = rhs.get(lhs.get(i).unwrap()).unwrap();
            self.set(i, e);
        }
        Some(())
    }

    /// Returns the composition of permutation `lhs` followed by `rhs`. Returns
    /// `None` if `self.n() != rhs.n()`.
    pub fn mul(&self, rhs: &Self) -> Option<Self> {
        if self.n() != rhs.n() {
            return None
        }
        let mut res = Self::ident(self.nz_n()).unwrap();
        res.mul_copy_assign(self, rhs).unwrap();
        Some(res)
    }

    /// Adds a LUT index bit at position `i`, where 0 adds a bit at bit position
    /// 0 and moves the other indexes upwards, and `self.n()` adds a bit at
    /// the end. The value of the new bit does not modulate the behavior of the
    /// table with respect to the original index bits, and the new output bit is
    /// just a copy of the input. Returns `None` if `i > self.n()` or if
    /// `self.n() != (rhs.n() + 1)`.
    pub fn double_assign(&mut self, rhs: &Self, i: usize) -> Option<()> {
        if (i > self.n()) || (self.n() != (rhs.n() + 1)) {
            return None
        }
        for j in 0..self.l() {
            // remove the `i`th bit of `j`
            let projected_j = if i == 0 {
                j >> 1
            } else {
                let lo = j & (MAX >> (BITS - i));
                let hi = j & (MAX << (i + 1));
                lo | (hi >> 1)
            };
            let e = rhs.get(projected_j).unwrap();
            // insert the `i`th bit of `j`
            let projected_e = if i == 0 {
                (j & 1) | (e << 1)
            } else {
                let lo = e & (MAX >> (BITS - i));
                let hi = e & (MAX << i);
                lo | (j & (1 << i)) | (hi << 1)
            };
            self.set(j, projected_e);
        }
        Some(())
    }

    /// Returns `None` if `i > self.n()` or if memory overflow occurs
    pub fn double(&self, i: usize) -> Option<Self> {
        if i > self.n() {
            return None
        }
        let mut res = Self::ident(NonZeroUsize::new(self.n() + 1)?)?;
        res.double_assign(self, i).unwrap();
        Some(res)
    }

    /// Removes a LUT index bit at position `i` and uses entries that had bit
    /// `i` set to `b` for the new LUT. Returns `None` if `i >= rhs.n()` or
    /// `(self.n() + 1) != rhs.n()`.
    pub fn halve_assign(&mut self, rhs: &Self, i: usize, b: bool) -> Option<()> {
        if (i >= rhs.n()) || ((self.n() + 1) != rhs.n()) {
            return None
        }
        let mut k = 0;
        for j in 0..rhs.l() {
            // see if `i`th bit is equal to `b`
            let e = rhs.get(j).unwrap();
            if ((e & (1 << i)) != 0) == b {
                // remove the `i`th bit of `e`
                let projected_e = if i == 0 {
                    e >> 1
                } else {
                    let lo = e & (MAX >> (BITS - i));
                    let hi = e & (MAX << (i + 1));
                    lo | (hi >> 1)
                };
                self.set(k, projected_e);
                // works because of monotonicity
                k += 1;
            }
        }
        Some(())
    }

    /// Removes a LUT index bit at position `i` and uses indexes that had bit
    /// `b` for the new LUT. Returns `None` if `i >= self.n()` or `self.n() <
    /// 2`.
    pub fn halve(&self, i: usize, b: bool) -> Option<Self> {
        if (i >= self.n()) || (self.n() < 2) {
            return None
        }
        let mut res = Self::ident(NonZeroUsize::new(self.n() - 1).unwrap()).unwrap();
        res.halve_assign(self, i, b).unwrap();
        Some(res)
    }

    /// Writes `self` as a string table representation to `s`
    pub fn write_table<W: Write>(&self, s: &mut W) {
        let mut awi_i = ExtAwi::zero(self.nz_n());
        let mut out = ExtAwi::zero(self.nz_n());
        let mut buf = [0u8; BITS];
        let mut pad = ExtAwi::zero(self.nz_n());
        for i in 0..self.l() {
            awi_i.usize_assign(i);
            Self::lut(&mut out, self, &awi_i).unwrap();
            awi_i
                .to_bytes_radix(false, &mut buf, 2, false, &mut pad)
                .unwrap();
            unsafe {
                write!(
                    s,
                    "\n{}|",
                    std::str::from_utf8_unchecked(&buf[(BITS - self.n())..])
                )
                .unwrap();
            }
            out.to_bytes_radix(false, &mut buf, 2, false, &mut pad)
                .unwrap();
            unsafe {
                write!(
                    s,
                    "{}",
                    std::str::from_utf8_unchecked(&buf[(BITS - self.n())..])
                )
                .unwrap();
            }
        }
        writeln!(s).unwrap();
    }

    pub fn to_string_table(&self) -> String {
        let mut s = String::new();
        self.write_table(&mut s);
        s
    }

    /// Sends `self.write_table` to stdout
    pub fn dbg_table(&self) {
        let mut s = String::new();
        self.write_table(&mut s);
        println!("{}", s);
    }

    /// `self` as a string matrix representation
    pub fn to_mat_string(&self) -> String {
        // the entry is the number of zeroes horizontally
        let l = self.l();
        let mut mat = vec!['\u{00B7}'; (l + 2) * (l + 1)];
        mat[0] = ' ';
        let hex_char = |i: usize| {
            let x = (i as u8) % 16;
            if x < 10 {
                char::from(b'0' + x)
            } else {
                char::from(b'a' + (x - 10))
            }
        };
        for i in 0..l {
            mat[i + 1] = hex_char(i);
        }
        for j in 0..l {
            mat[(j + 1) * (l + 2)] = hex_char(j);
            mat[(j + 2) * (l + 2) - 1] = '\n';
        }
        mat[l + 1] = '\n';
        for i in 0..l {
            let j = self.get(i).unwrap();
            mat[((i + 1) * (l + 2)) + (j + 1)] = '1';
        }
        mat.into_iter().collect()
    }

    /// Sends `self.dbg_mat_string` to stdout
    pub fn dbg_mat_string(&self) {
        let mat = self.to_mat_string();
        println!("{}", mat);
    }
}

impl fmt::Debug for Perm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = String::new();
        self.write_table(&mut s);
        f.write_str(&s)
    }
}
