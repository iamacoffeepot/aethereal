//! `Writer`: what an encode at a boundary writes into (ADR-0004).

use crate::Boundary;
use alloc::vec::Vec;

/// The bytes written so far, and what the boundary `B` offers beside them (ADR-0004).
///
/// There is one writer type, over the boundary. A codec appends to
/// [`bytes`](Self::bytes); a bound value also calls what it needs of
/// [`offers`](Self::offers).
pub struct Writer<'a, B: Boundary> {
    bytes: Vec<u8>,
    offers: B::WriterOffers<'a>,
}

impl<'a, B: Boundary> Writer<'a, B> {
    /// A writer with no bytes written, holding what `B` offers.
    #[must_use]
    pub const fn new(offers: B::WriterOffers<'a>) -> Self {
        Self { bytes: Vec::new(), offers }
    }

    /// A writer that appends to `bytes`, holding what `B` offers: for writing into a buffer that is kept and
    /// reused.
    #[must_use]
    pub const fn appending(bytes: Vec<u8>, offers: B::WriterOffers<'a>) -> Self {
        Self { bytes, offers }
    }

    /// The bytes written so far, to append to.
    pub const fn bytes(&mut self) -> &mut Vec<u8> {
        &mut self.bytes
    }

    /// What the boundary offers a bound value being written.
    pub const fn offers(&mut self) -> &mut B::WriterOffers<'a> {
        &mut self.offers
    }

    /// The bytes written, once nothing more is to be.
    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}
