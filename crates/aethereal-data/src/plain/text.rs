//! `String` as plain data (ADR-0004).

use crate::plain::{read_count, take_bytes, write_text};
use crate::{Budget, DecodeError, EncodeError, Plain, Schema, Shape};
use alloc::string::String;
use alloc::vec::Vec;
use core::str::from_utf8;

impl Schema for String {
    const SHAPE: Shape = Shape::String;
}

impl Plain for String {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        write_text(out, self)
    }

    fn decode(input: &mut &[u8], budget: &mut Budget) -> Result<Self, DecodeError> {
        let count = read_count(input)?;
        let length = count as usize;
        budget.charge(length)?;
        let bytes = take_bytes(input, length)?;
        let text = from_utf8(bytes).map_err(|_| DecodeError::InvalidUtf8)?;
        Ok(text.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_bytes, to_bytes};

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
