//! Boundaries: what a value is crossing when it is encoded or decoded (ADR-0004).
//!
//! A boundary is a type, and the codec takes it as a parameter. A type has one
//! `Encode` impl and one `Decode` impl, each written over every boundary, so its
//! bytes are the same wherever it is encoded.

mod bare;
mod reader;
#[cfg(test)]
mod tests;
mod writer;

use crate::{DecodeError, EncodeError};

pub use bare::Bare;
pub use reader::Reader;
pub use writer::Writer;

/// What a value crosses when it is encoded or decoded (ADR-0004).
///
/// Every writer holds the bytes written so far, and every reader the bytes
/// still to read and the allocation limit. A boundary states only what it
/// offers beside them: an offer is a value the writer or reader holds, and a
/// bound value is bounded by a trait on it.
pub trait Boundary {
    /// What this boundary's writer holds beside the bytes written so far.
    type WriterOffers<'a>;
    /// What this boundary's reader holds beside the bytes still to read and the budget.
    type ReaderOffers<'a>;
}

/// A value that can be written at the boundary `B` (ADR-0004).
///
/// Implemented once per type, over every boundary that offers what the type needs.
pub trait Encode<B: Boundary> {
    /// Appends this value's bytes to `out` (ADR-0002).
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError>;
}

/// A value that can be read at the boundary `B` (ADR-0004).
///
/// Implemented once per type, over every boundary that offers what the type needs.
pub trait Decode<B: Boundary>: Sized {
    /// Reads one value from the front of `input`, advancing it past the bytes read (ADR-0002).
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError>;
}
