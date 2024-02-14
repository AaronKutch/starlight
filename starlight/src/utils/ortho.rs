use core::fmt;
use std::ops::{Index, IndexMut, Neg};

/// Represents an orthogonal direction on a grid
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Ortho {
    /// Negative .0 orthogonal direction
    Neg0 = 0,
    /// Positive .0 orthogonal direction
    Pos0 = 1,
    /// Negative .1 orthogonal direction
    Neg1 = 2,
    /// Positive .1 orthogonal direction
    Pos1 = 3,
}

impl Ortho {
    pub fn to_u8(self) -> u8 {
        self as u8
    }

    pub fn try_from_u8(x: u8) -> Option<Self> {
        match x {
            0 => Some(Self::Neg0),
            1 => Some(Self::Pos0),
            2 => Some(Self::Neg1),
            3 => Some(Self::Pos1),
            _ => None,
        }
    }

    pub fn to_usize(self) -> usize {
        self as usize
    }

    pub fn try_from_usize(x: usize) -> Option<Self> {
        match x {
            0 => Some(Self::Neg0),
            1 => Some(Self::Pos0),
            2 => Some(Self::Neg1),
            3 => Some(Self::Pos1),
            _ => None,
        }
    }
}

impl From<Ortho> for u8 {
    fn from(value: Ortho) -> Self {
        value.to_u8()
    }
}

impl TryFrom<u8> for Ortho {
    type Error = u8;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::try_from_u8(value).ok_or(value)
    }
}

impl From<Ortho> for usize {
    fn from(value: Ortho) -> Self {
        value.to_usize()
    }
}

impl TryFrom<usize> for Ortho {
    type Error = usize;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        Self::try_from_usize(value).ok_or(value)
    }
}

impl Neg for Ortho {
    type Output = Self;

    /// Inverts the direction
    fn neg(self) -> Self::Output {
        match self {
            Ortho::Neg0 => Ortho::Neg1,
            Ortho::Pos0 => Ortho::Pos1,
            Ortho::Neg1 => Ortho::Neg0,
            Ortho::Pos1 => Ortho::Pos0,
        }
    }
}

impl fmt::Display for Ortho {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// An array of 4 elements for each of the 4 orthogonal directions
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OrthoArray<T>(pub [T; 4]);

impl<T> OrthoArray<T> {
    pub fn get(&self, ortho: Ortho) -> &T {
        &self.0[ortho.to_usize()]
    }

    pub fn get_mut(&mut self, ortho: Ortho) -> &mut T {
        &mut self.0[ortho.to_usize()]
    }
}

impl<T> Index<Ortho> for OrthoArray<T> {
    type Output = T;

    fn index(&self, i: Ortho) -> &T {
        self.get(i)
    }
}

impl<T> IndexMut<Ortho> for OrthoArray<T> {
    fn index_mut(&mut self, i: Ortho) -> &mut T {
        self.get_mut(i)
    }
}

impl<T> From<[T; 4]> for OrthoArray<T> {
    fn from(value: [T; 4]) -> Self {
        Self(value)
    }
}

impl<T> From<OrthoArray<T>> for [T; 4] {
    fn from(value: OrthoArray<T>) -> Self {
        value.0
    }
}
