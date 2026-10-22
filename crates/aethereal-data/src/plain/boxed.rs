//! `Box<T>` as plain data: a box is its contents (ADR-0004).

use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use alloc::boxed::Box;

impl<T: Schema> Schema for Box<T> {
    const SHAPE: Shape = T::SHAPE;
    const SEQUENCE: Shape = T::SEQUENCE;
}

impl<B: Boundary, T: Encode<B>> Encode<B> for Box<T> {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.as_ref().encode(out)
    }
}

impl<B: Boundary, T: Decode<B>> Decode<B> for Box<T> {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        input.budget().charge(size_of::<T>())?;
        Ok(Self::new(T::decode(input)?))
    }
}

impl<T: Plain> Plain for Box<T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, KindId, from_bytes, to_bytes};
    use alloc::vec::Vec;

    #[derive(Debug, PartialEq, Plain)]
    #[kind(name = "aethereal.test.note")]
    struct Inline {
        id: u32,
        tags: Vec<u8>,
    }

    #[derive(Debug, PartialEq, Plain)]
    #[kind(name = "aethereal.test.note")]
    #[allow(clippy::vec_box, reason = "the test is that a boxed byte keeps its sequence `Bytes`")]
    struct Boxed {
        id: Box<u32>,
        tags: Vec<Box<u8>>,
    }

    #[test]
    fn a_boxed_field_changes_no_bytes() {
        // Catches a box that writes a marker or a count of its own.
        let inline = Inline { id: 7, tags: alloc::vec![1, 2] };
        let boxed = Boxed { id: Box::new(7), tags: alloc::vec![Box::new(1), Box::new(2)] };
        let bytes = to_bytes(&inline);
        assert_eq!(to_bytes(&boxed), bytes);
        let bytes = bytes.expect("the inline value encodes");
        assert_eq!(from_bytes::<Boxed>(&bytes, &mut Budget::new(64)), Ok(boxed));
    }

    #[test]
    fn a_boxed_field_changes_no_kind_id() {
        // Tripwire: boxing a field leaves the layout, and so the kind id, as it
        // was; a boxed byte in a sequence keeps the sequence `Bytes` (ADR-0004).
        assert_eq!(KindId::of::<Boxed>(), KindId::of::<Inline>());
    }

    #[test]
    fn a_box_is_charged_before_it_is_allocated() {
        // Catches a decode that allocates each box outside the budget: one
        // input byte here asks for a kilobyte on the heap.
        let bytes = to_bytes(&None::<[u8; 1024]>).expect("an empty option encodes");
        let refused = from_bytes::<Box<Option<[u8; 1024]>>>(&bytes, &mut Budget::new(16));
        assert_eq!(refused, Err(DecodeError::BudgetExceeded { requested: 1025, remaining: 16 }));
    }
}
