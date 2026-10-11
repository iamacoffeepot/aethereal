//! `Option<T>` and `Result<T, E>` as plain data (ADR-0004).

use crate::plain::{read_array, read_selector, write_selector};
use crate::{
    Boundary, Decode, DecodeError, Encode, EncodeError, Fields, Plain, Reader, Schema, Shape, Variant, Writer,
};

impl<T: Schema> Schema for Option<T> {
    const SHAPE: Shape = Shape::Option(&T::SHAPE);
}

impl<B: Boundary, T: Encode<B>> Encode<B> for Option<T> {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        match self {
            None => {
                out.bytes().push(0);
                Ok(())
            }
            Some(value) => {
                out.bytes().push(1);
                value.encode(out)
            }
        }
    }
}

impl<B: Boundary, T: Decode<B>> Decode<B> for Option<T> {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let [marker] = read_array(input.input())?;
        match marker {
            0 => Ok(None),
            1 => {
                let value = T::decode(input)?;
                Ok(Some(value))
            }
            other => Err(DecodeError::InvalidOption(other)),
        }
    }
}

impl<T: Plain> Plain for Option<T> {}

impl<T: Schema, E: Schema> Schema for Result<T, E> {
    const SHAPE: Shape = Shape::Enum {
        name: "Result",
        variants: &[
            Variant { name: "Ok", selector: 0, fields: Fields::Unnamed(&[T::SHAPE]) },
            Variant { name: "Err", selector: 1, fields: Fields::Unnamed(&[E::SHAPE]) },
        ],
    };
}

impl<B: Boundary, T: Encode<B>, E: Encode<B>> Encode<B> for Result<T, E> {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        match self {
            Ok(value) => {
                write_selector(out.bytes(), 0);
                value.encode(out)?;
                Ok(())
            }
            Err(error) => {
                write_selector(out.bytes(), 1);
                error.encode(out)?;
                Ok(())
            }
        }
    }
}

impl<B: Boundary, T: Decode<B>, E: Decode<B>> Decode<B> for Result<T, E> {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let selector = read_selector(input.input())?;
        match selector {
            0 => {
                let value = T::decode(input)?;
                Ok(Ok(value))
            }
            1 => {
                let error = E::decode(input)?;
                Ok(Err(error))
            }
            other => Err(DecodeError::InvalidSelector { name: "Result", selector: other }),
        }
    }
}

impl<T: Plain, E: Plain> Plain for Result<T, E> {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};

    #[test]
    fn option_marker_bytes() {
        // Catches a marker written as count or selector width instead of one byte.
        let none: Option<u8> = None;
        assert_eq!(to_bytes(&none), Ok(alloc::vec![0]));
        assert_eq!(to_bytes(&Some(7u8)), Ok(alloc::vec![1, 7]));
        let decoded_none = from_bytes::<Option<u8>>(&[0], &mut Budget::new(16));
        assert_eq!(decoded_none, Ok(None));
        assert_eq!(from_bytes::<Option<u8>>(&[1, 7], &mut Budget::new(16)), Ok(Some(7)));
    }

    #[test]
    fn option_byte_two_is_refused() {
        // Catches a decoder that treats any nonzero marker as `Some`.
        assert_eq!(from_bytes::<Option<u8>>(&[2], &mut Budget::new(16)), Err(DecodeError::InvalidOption(2)));
    }

    #[test]
    fn result_uses_selectors_zero_and_one() {
        // Catches swapped or mistyped `Result` selectors.
        let ok: Result<u8, u8> = Ok(3);
        assert_eq!(to_bytes(&ok), Ok(alloc::vec![0, 0, 0, 0, 3]));
        let err: Result<u8, u8> = Err(4);
        assert_eq!(to_bytes(&err), Ok(alloc::vec![1, 0, 0, 0, 4]));
        assert_eq!(from_bytes::<Result<u8, u8>>(&[0, 0, 0, 0, 3], &mut Budget::new(16)), Ok(Ok(3)));
        assert_eq!(from_bytes::<Result<u8, u8>>(&[1, 0, 0, 0, 4], &mut Budget::new(16)), Ok(Err(4)));
    }

    #[test]
    fn result_selector_two_is_refused() {
        // Catches a decoder that reads past the declared variants.
        let refused = from_bytes::<Result<u8, u8>>(&[2, 0, 0, 0], &mut Budget::new(16));
        assert_eq!(refused, Err(DecodeError::InvalidSelector { name: "Result", selector: 2 }));
    }
}
