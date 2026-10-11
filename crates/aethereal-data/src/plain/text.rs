//! `String` as plain data (ADR-0004).

use crate::plain::{read_count, take_bytes, write_text};
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use alloc::string::String;
use core::str::from_utf8;

impl Schema for String {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for String {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_text(out.bytes(), self)
    }
}

impl<B: Boundary> Decode<B> for String {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let count = read_count(input.input())?;
        let length = count as usize;
        input.budget().charge(length)?;
        let bytes = take_bytes(input.input(), length)?;
        let text = from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
        Ok(text.into())
    }
}

impl Plain for String {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};

    #[test]
    fn string_is_count_then_utf8() {
        // Catches a string codec that writes bytes without their count.
        assert_eq!(to_bytes(&String::from("hi")), Ok(alloc::vec![0x02, 0x00, 0x00, 0x00, b'h', b'i']));
        assert_eq!(
            from_bytes::<String>(&[0x02, 0x00, 0x00, 0x00, b'h', b'i'], &mut Budget::new(16)),
            Ok(String::from("hi"))
        );
    }

    #[test]
    fn invalid_utf8_is_refused() {
        // Catches a decoder that builds a string from unchecked bytes.
        assert_eq!(
            from_bytes::<String>(&[0x01, 0x00, 0x00, 0x00, 0xFF], &mut Budget::new(16)),
            Err(DecodeError::InvalidUtf8)
        );
    }
}
