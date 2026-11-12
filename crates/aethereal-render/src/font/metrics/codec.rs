//! The bytes metrics are encoded as: numbers little-endian, and each list after its count, a 64-bit count, so that no
//! list a font could hold overflows it. A read past the end is refused, never a panic.

use super::MetricsMalformed;

/// Bytes being written.
#[derive(Debug, Default)]
pub(in crate::font) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    /// The bytes written.
    pub(in crate::font) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Writes `value`.
    pub(in crate::font) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    /// Writes `value`.
    pub(in crate::font) fn u16(&mut self, value: u16) {
        self.bytes.extend(value.to_le_bytes());
    }

    /// Writes `value`.
    pub(in crate::font) fn i16(&mut self, value: i16) {
        self.bytes.extend(value.to_le_bytes());
    }

    /// Writes `value`.
    pub(in crate::font) fn u32(&mut self, value: u32) {
        self.bytes.extend(value.to_le_bytes());
    }

    /// Writes a list's count.
    pub(in crate::font) fn count(&mut self, count: usize) {
        self.bytes.extend((count as u64).to_le_bytes());
    }
}

/// Bytes being read, from the front.
#[derive(Debug)]
pub(in crate::font) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    /// Reads `bytes` from their start.
    pub(in crate::font) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// The next `N` bytes.
    fn take<const N: usize>(&mut self) -> Result<[u8; N], MetricsMalformed> {
        let (taken, rest) = self.bytes.split_first_chunk::<N>().ok_or(MetricsMalformed::CutShort)?;
        self.bytes = rest;

        Ok(*taken)
    }

    /// The next byte.
    pub(in crate::font) fn u8(&mut self) -> Result<u8, MetricsMalformed> {
        self.take().map(u8::from_le_bytes)
    }

    /// The next unsigned 16-bit number.
    pub(in crate::font) fn u16(&mut self) -> Result<u16, MetricsMalformed> {
        self.take().map(u16::from_le_bytes)
    }

    /// The next signed 16-bit number.
    pub(in crate::font) fn i16(&mut self) -> Result<i16, MetricsMalformed> {
        self.take().map(i16::from_le_bytes)
    }

    /// The next unsigned 32-bit number.
    pub(in crate::font) fn u32(&mut self) -> Result<u32, MetricsMalformed> {
        self.take().map(u32::from_le_bytes)
    }

    /// The next list's count, of records each at least `bytes` bytes long, refused when the bytes left could not hold
    /// that many: so nothing is taken for a list longer than the bytes.
    pub(in crate::font) fn count(&mut self, bytes: usize) -> Result<usize, MetricsMalformed> {
        let short = MetricsMalformed::CutShort;
        let count = usize::try_from(u64::from_le_bytes(self.take()?)).map_err(|_| short)?;
        let needs = count.checked_mul(bytes).ok_or(short)?;
        if needs > self.bytes.len() {
            return Err(short);
        }

        Ok(count)
    }

    /// Ends the read.
    ///
    /// # Errors
    ///
    /// [`MetricsMalformed::Trailing`] when bytes are left.
    pub(in crate::font) const fn end(self) -> Result<(), MetricsMalformed> {
        if !self.bytes.is_empty() {
            return Err(MetricsMalformed::Trailing);
        }

        Ok(())
    }
}
