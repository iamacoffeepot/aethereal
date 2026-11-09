//! A font's bytes, read bounds-checked and big-endian: a read past the end of a table is that table's refusal, never
//! a panic.

use super::{FontRefused, Table};

/// A table's bytes, or a part of them, and the table they are read for, which a read past their end names.
#[derive(Debug, Clone, Copy)]
pub(super) struct Data<'a> {
    bytes: &'a [u8],
    table: Table,
}

impl<'a> Data<'a> {
    /// `bytes`, read for `table`.
    pub(super) const fn new(bytes: &'a [u8], table: Table) -> Self {
        Self { bytes, table }
    }

    /// How many bytes there are.
    pub(super) const fn len(self) -> usize {
        self.bytes.len()
    }

    /// Whether there are no bytes.
    pub(super) const fn is_empty(self) -> bool {
        self.bytes.is_empty()
    }

    /// The refusal of the table these bytes are read for, as malformed.
    pub(super) const fn malformed(self) -> FontRefused {
        FontRefused::Malformed(self.table)
    }

    /// The `N` bytes at `at`.
    pub(super) fn array<const N: usize>(self, at: usize) -> Result<[u8; N], FontRefused> {
        let refused = self.malformed();
        let end = at.checked_add(N).ok_or(refused)?;
        self.bytes.get(at..end).and_then(|bytes| bytes.try_into().ok()).ok_or(refused)
    }

    /// The byte at `at`.
    pub(super) fn u8(self, at: usize) -> Result<u8, FontRefused> {
        let refused = self.malformed();
        self.bytes.get(at).copied().ok_or(refused)
    }

    /// The signed byte at `at`.
    pub(super) fn i8(self, at: usize) -> Result<i8, FontRefused> {
        self.array(at).map(i8::from_be_bytes)
    }

    /// The unsigned 16-bit number at `at`.
    pub(super) fn u16(self, at: usize) -> Result<u16, FontRefused> {
        self.array(at).map(u16::from_be_bytes)
    }

    /// The signed 16-bit number at `at`.
    pub(super) fn i16(self, at: usize) -> Result<i16, FontRefused> {
        self.array(at).map(i16::from_be_bytes)
    }

    /// The unsigned 32-bit number at `at`.
    pub(super) fn u32(self, at: usize) -> Result<u32, FontRefused> {
        self.array(at).map(u32::from_be_bytes)
    }

    /// The 32-bit number at `at`, as an offset or a count.
    pub(super) fn offset32(self, at: usize) -> Result<usize, FontRefused> {
        self.u32(at).and_then(|value| usize::try_from(value).map_err(|_| self.malformed()))
    }

    /// The 16-bit number at `at`, as an offset or a count.
    pub(super) fn offset16(self, at: usize) -> Result<usize, FontRefused> {
        self.u16(at).map(usize::from)
    }

    /// The `length` bytes from `at`.
    pub(super) fn slice(self, at: usize, length: usize) -> Result<Self, FontRefused> {
        let refused = self.malformed();
        let end = at.checked_add(length).ok_or(refused)?;
        let bytes = self.bytes.get(at..end).ok_or(refused)?;
        Ok(Self { bytes, table: self.table })
    }

    /// The bytes from `at` to the end.
    pub(super) fn from(self, at: usize) -> Result<Self, FontRefused> {
        let refused = self.malformed();
        let bytes = self.bytes.get(at..).ok_or(refused)?;
        Ok(Self { bytes, table: self.table })
    }

    /// The position `index` records of `size` bytes past `start`, or the table's refusal if it overflows.
    pub(super) fn at(self, start: usize, index: usize, size: usize) -> Result<usize, FontRefused> {
        let refused = self.malformed();
        index.checked_mul(size).and_then(|offset| offset.checked_add(start)).ok_or(refused)
    }
}
