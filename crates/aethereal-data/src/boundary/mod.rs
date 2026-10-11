//! Boundaries: what a value is crossing when it is encoded or decoded (ADR-0004).
//!
//! A boundary is a type, and the codec takes it as a parameter. A type has one
//! `Encode` impl and one `Decode` impl, each written over every boundary, so its
//! bytes are the same wherever it is encoded.

mod bare;
#[cfg(test)]
mod tests;

use crate::{Budget, DecodeError, EncodeError};
use alloc::vec::Vec;

pub use bare::{Bare, BareReader};

/// What a value crosses when it is encoded or decoded (ADR-0004).
///
/// What a boundary offers a bound value is a trait on its writer or its reader.
pub trait Boundary {
    /// What an encode at this boundary writes into.
    type Writer<'a>: Writer;
    /// What a decode at this boundary reads from.
    type Reader<'a>: Reader<'a>;
}

/// The bytes written so far, which every boundary's writer holds (ADR-0004).
pub trait Writer {
    /// The bytes written so far, to append to.
    fn bytes(&mut self) -> &mut Vec<u8>;
}

/// The bytes still to read and the allocation limit, which every boundary's reader holds (ADR-0004).
pub trait Reader<'input> {
    /// The bytes still to read; a decode advances them past what it reads.
    fn input(&mut self) -> &mut &'input [u8];
    /// The bytes this decode may still allocate (ADR-0002).
    fn budget(&mut self) -> &mut Budget;
}

/// A value that can be written at the boundary `B` (ADR-0004).
///
/// Implemented once per type, over every boundary that offers what the type needs.
pub trait Encode<B: Boundary> {
    /// Appends this value's bytes to `out` (ADR-0002).
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError>;
}

/// A value that can be read at the boundary `B` (ADR-0004).
///
/// Implemented once per type, over every boundary that offers what the type needs.
pub trait Decode<B: Boundary>: Sized {
    /// Reads one value from the front of `input`, advancing it past the bytes read (ADR-0002).
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError>;
}
