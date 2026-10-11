//! `Vec<T>`, `VecDeque<T>`, and `[T; N]` as plain data (ADR-0004).

use crate::plain::{charge_count, read_count, write_count};
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use alloc::collections::VecDeque;
use alloc::vec::Vec;

impl<T: Schema> Schema for Vec<T> {
    const SHAPE: Shape = T::SEQUENCE;
}

impl<B: Boundary, T: Encode<B>> Encode<B> for Vec<T> {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        write_count(out.bytes(), self.len())?;
        for item in self {
            item.encode(out)?;
        }
        Ok(())
    }
}

impl<B: Boundary, T: Decode<B>> Decode<B> for Vec<T> {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        let length = charge_count::<T>(input.budget(), count)?;
        (0..length).map(|_| T::decode(input)).collect()
    }
}

impl<T: Plain> Plain for Vec<T> {}

impl<T: Schema> Schema for VecDeque<T> {
    const SHAPE: Shape = T::SEQUENCE;
}

impl<B: Boundary, T: Encode<B>> Encode<B> for VecDeque<T> {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        write_count(out.bytes(), self.len())?;
        for item in self {
            item.encode(out)?;
        }
        Ok(())
    }
}

impl<B: Boundary, T: Decode<B>> Decode<B> for VecDeque<T> {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        let length = charge_count::<T>(input.budget(), count)?;
        (0..length).map(|_| T::decode(input)).collect()
    }
}

impl<T: Plain> Plain for VecDeque<T> {}

impl<T: Schema, const LENGTH: usize> Schema for [T; LENGTH] {
    const SHAPE: Shape = {
        assert!(LENGTH <= u32::MAX as usize);
        #[allow(clippy::cast_possible_truncation, reason = "the assertion above proves LENGTH fits in u32")]
        Shape::Array { element: &T::SHAPE, length: LENGTH as u32 }
    };
}

impl<B: Boundary, T: Encode<B>, const LENGTH: usize> Encode<B> for [T; LENGTH] {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        for item in self {
            item.encode(out)?;
        }
        Ok(())
    }
}

impl<B: Boundary, T: Decode<B>, const LENGTH: usize> Decode<B> for [T; LENGTH] {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let each = size_of::<T>().max(1);
        let cost = LENGTH.checked_mul(each);
        if let Some(bytes) = cost {
            input.budget().charge(bytes)?;
        } else {
            let remaining = input.budget().remaining();
            return Err(DecodeError::BudgetExceeded { requested: usize::MAX, remaining });
        }
        let mut items = Vec::new();
        for _ in 0..LENGTH {
            let item = T::decode(input)?;
            items.push(item);
        }
        from_vec(items)
    }
}

impl<T: Plain, const LENGTH: usize> Plain for [T; LENGTH] {}

/// Converts the temporary decode buffer into its array (ADR-0004).
///
/// The length always matches; the error arm names the refusal for the impossible mismatch.
fn from_vec<T, const LENGTH: usize>(items: Vec<T>) -> Result<[T; LENGTH], DecodeError> {
    items.try_into().map_err(|_| DecodeError::UnexpectedEnd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};

    #[test]
    fn vec_is_count_then_elements() {
        // Catches a sequence codec that writes elements without their count.
        let values = alloc::vec![0x0201u16, 0x0403u16];
        assert_eq!(to_bytes(&values), Ok(alloc::vec![0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04]));
        let decoded = from_bytes::<Vec<u16>>(&[0x02, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04], &mut Budget::new(16));
        assert_eq!(decoded, Ok(values));
    }

    #[test]
    fn array_has_no_count() {
        // Catches an array codec that writes a count like a sequence.
        let values = [0x0201u16, 0x0403u16];
        assert_eq!(to_bytes(&values), Ok(alloc::vec![0x01, 0x02, 0x03, 0x04]));
        assert_eq!(from_bytes::<[u16; 2]>(&[0x01, 0x02, 0x03, 0x04], &mut Budget::new(16)), Ok(values));
    }

    #[test]
    fn vec_u8_is_bytes_and_vec_u16_is_sequence() {
        // Catches `Vec<u8>` routed to `Sequence` instead of `Bytes` (ADR-0004).
        assert_eq!(<Vec<u8> as Schema>::SHAPE, Shape::Bytes);
        let shape = <Vec<u16> as Schema>::SHAPE;
        match shape {
            Shape::Sequence(element) => assert_eq!(*element, Shape::Integer(crate::Integer::U16)),
            other => panic!("expected a sequence, found {other:?}"),
        }
    }

    #[test]
    fn huge_vec_count_is_refused_on_budget() {
        // Catches a decoder that allocates from an untrusted count before charging.
        let refused = from_bytes::<Vec<u64>>(&[0xFF, 0xFF, 0xFF, 0xFF], &mut Budget::new(16));
        match refused {
            Err(DecodeError::BudgetExceeded { .. }) => {}
            other => panic!("expected a budget refusal, found {other:?}"),
        }
        let refused_units = from_bytes::<Vec<()>>(&[0xFF, 0xFF, 0xFF, 0x7F], &mut Budget::new(16));
        match refused_units {
            Err(DecodeError::BudgetExceeded { .. }) => {}
            other => panic!("expected a budget refusal, found {other:?}"),
        }
    }

    #[test]
    fn short_input_is_refused() {
        // Catches a decoder that pads input that ends early.
        assert_eq!(
            from_bytes::<Vec<u16>>(&[0x01, 0x00, 0x00, 0x00, 0x01], &mut Budget::new(16)),
            Err(DecodeError::UnexpectedEnd)
        );
    }
}
