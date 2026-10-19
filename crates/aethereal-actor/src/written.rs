//! Bytes written at [`Within`], with a pin on each blob they refer to (ADR-0014).

#[cfg(feature = "std")]
use crate::BlobRef;
use crate::{Within, WithinReader, WithinWriter};
use aethereal_data::{Budget, Decode, DecodeError, Encode, EncodeError, Reader, Writer};
use alloc::vec::Vec;

/// A value written at [`Within`], and a pin on the blob of each reference in it, which keep the blobs for as long as
/// the bytes are kept (ADR-0014).
///
/// It is the only way to write or read at `Within`: a writer there keeps a pin on the blob of each reference it
/// writes, and a reader takes each reference it reads as a copy of the pin the bytes keep on its blob, refusing one
/// whose blob they keep none on. So a reference read back always names a kept blob, and every pin goes when what holds
/// it does.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Written {
    bytes: Vec<u8>,
    #[cfg(feature = "std")]
    pins: Vec<BlobRef>,
}

impl Written {
    /// `value`, written at [`Within`].
    ///
    /// # Errors
    ///
    /// Why `value` does not encode.
    pub fn of<T: Encode<Within> + ?Sized>(value: &T) -> Result<Self, EncodeError> {
        Self::writing(|out| value.encode(out))
    }

    /// What `write` writes at [`Within`]: for a value written in parts.
    ///
    /// # Errors
    ///
    /// What `write` fails with.
    pub(crate) fn writing(
        write: impl FnOnce(&mut Writer<'_, Within>) -> Result<(), EncodeError>,
    ) -> Result<Self, EncodeError> {
        #[cfg(feature = "std")]
        let mut pins = Vec::new();
        #[cfg(feature = "std")]
        let mut out = Writer::<Within>::new(WithinWriter::holding(&mut pins));
        #[cfg(not(feature = "std"))]
        let mut out = Writer::<Within>::new(WithinWriter::holding());
        write(&mut out)?;
        let bytes = out.into_bytes();

        Ok(Self {
            bytes,
            #[cfg(feature = "std")]
            pins,
        })
    }

    /// `bytes` that keep `pins`: what another part or a guest's host hands over as written. A reference in them read
    /// back is a copy of one of `pins`, so a pin they lack is a reference refused, never a blob let go of too soon.
    #[cfg(feature = "std")]
    #[must_use]
    pub const fn new(bytes: Vec<u8>, pins: Vec<BlobRef>) -> Self {
        Self { bytes, pins }
    }

    /// `bytes`, which refer to no blob.
    #[cfg(not(feature = "std"))]
    #[must_use]
    pub const fn new(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// The value read back as `T`, allocating at most `limit` bytes, all of the bytes read.
    ///
    /// # Errors
    ///
    /// Why they do not read back as `T`: [`DecodeError::Unheld`] when a reference in them names a blob they keep no pin
    /// on, and [`DecodeError::TrailingBytes`] when bytes are left over.
    pub fn read<T: Decode<Within>>(&self, limit: usize) -> Result<T, DecodeError> {
        self.reading(limit, |reader| {
            let value = T::decode(reader)?;
            let count = reader.input().len();
            let is_whole = count == 0;
            if !is_whole {
                return Err(DecodeError::TrailingBytes { count });
            }

            Ok(value)
        })
    }

    /// What `read` reads of the bytes, allocating at most `limit` bytes: for a value read in parts.
    ///
    /// # Errors
    ///
    /// What `read` fails with.
    pub(crate) fn reading<T>(
        &self,
        limit: usize,
        read: impl FnOnce(&mut Reader<'_, Within>) -> Result<T, DecodeError>,
    ) -> Result<T, DecodeError> {
        let mut budget = Budget::new(limit);
        #[cfg(feature = "std")]
        let offers = WithinReader::of(&self.pins);
        #[cfg(not(feature = "std"))]
        let offers = WithinReader::of();
        let mut reader = Reader::<Within>::new(&self.bytes, &mut budget, offers);

        read(&mut reader)
    }

    /// The bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The pins the bytes keep: one on the blob of each reference in them.
    #[cfg(feature = "std")]
    #[must_use]
    pub fn pins(&self) -> &[BlobRef] {
        &self.pins
    }
}
