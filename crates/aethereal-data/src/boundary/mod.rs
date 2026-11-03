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
use alloc::vec::Vec;

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

    /// Appends the bytes of each of `items` in turn, as a sequence's elements are written (ADR-0002). A type whose
    /// bytes are its memory writes them all at once.
    #[doc(hidden)]
    fn encode_each(items: &[Self], out: &mut Writer<'_, B>) -> Result<(), EncodeError>
    where
        Self: Sized,
    {
        for item in items {
            item.encode(out)?;
        }
        Ok(())
    }
}

/// A type that may be an element of a sequence, a `Vec` or a `VecDeque`, at the boundary `B` (ADR-0042).
///
/// Every type is an element wherever it crosses but a byte: `u8` is an element at [`Bare`] and at the boundary of an
/// actor's own state, and not between actors. So a byte vector may be in an actor's state, a setting, and a frame to
/// another engine, and a kind that holds one has no codec for mail or a config, which hold `Bytes`, a blob, instead.
/// The derives state it for every type they write, and a hand-written codec states it beside its `Encode`. It says
/// nothing of how a type is written, and a fixed array, `[u8; N]`, crosses as its elements do.
///
/// ```
/// use aethereal_data::{Bare, Encode, Writer};
///
/// let mut out = Writer::<Bare>::new(());
/// assert_eq!(vec![7u8, 8].encode(&mut out), Ok(()));
/// ```
#[diagnostic::on_unimplemented(
    message = "a `Vec` or `VecDeque` of `{Self}` does not cross `{B}`",
    label = "not an element of a sequence at `{B}`",
    note = "bytes that cross between actors are a blob: hold `Bytes`, put with `turn.put` and read with `turn.read` \
            (ADR-0042)",
    note = "a type with a hand-written codec states `Element` beside its `Encode`"
)]
pub trait Element<B: Boundary> {}

/// A value that can be read at the boundary `B` (ADR-0004).
///
/// Implemented once per type, over every boundary that offers what the type needs.
pub trait Decode<B: Boundary>: Sized {
    /// Reads one value from the front of `input`, advancing it past the bytes read (ADR-0002).
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError>;

    /// Reads `length` values in turn, as a sequence's elements are read, once the budget is charged for them
    /// (ADR-0002). A type whose bytes are its memory reads them all at once.
    #[doc(hidden)]
    fn decode_each(input: &mut Reader<'_, B>, length: usize) -> Result<Vec<Self>, DecodeError> {
        (0..length).map(|_| Self::decode(input)).collect()
    }
}
