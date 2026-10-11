//! `Duration` as plain data (ADR-0004).

use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Field, Integer, Plain, Reader, Schema, Shape, Writer};
use core::time::Duration;

/// The nanoseconds in one second; `nanos` at or above this is refused (ADR-0004).
const NANOS_PER_SECOND: u32 = 1_000_000_000;

impl Schema for Duration {
    const SHAPE: Shape = Shape::Struct {
        name: "Duration",
        fields: &[
            Field { name: "secs", shape: Shape::Integer(Integer::U64) },
            Field { name: "nanos", shape: Shape::Integer(Integer::U32) },
        ],
    };
}

impl<B: Boundary> Encode<B> for Duration {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.as_secs().encode(out)?;
        self.subsec_nanos().encode(out)
    }
}

impl<B: Boundary> Decode<B> for Duration {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let secs = u64::decode(input)?;
        let nanos = u32::decode(input)?;
        let within_second = nanos < NANOS_PER_SECOND;
        if within_second {
            Ok(Self::new(secs, nanos))
        } else {
            Err(DecodeError::InvalidNanos(nanos))
        }
    }
}

impl Plain for Duration {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};

    #[test]
    fn duration_is_secs_then_nanos() {
        // Catches a field order swapped from the declared `secs`, `nanos`.
        let span = Duration::new(3, 4);
        assert_eq!(
            to_bytes(&span),
            Ok(alloc::vec![0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00])
        );
        let bytes = [0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00];
        assert_eq!(from_bytes::<Duration>(&bytes, &mut Budget::new(32)), Ok(span));
    }

    #[test]
    fn billion_nanos_are_refused() {
        // Catches a decoder that accepts a `nanos` of a whole second or more.
        let bytes = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xCA, 0x9A, 0x3B];
        assert_eq!(from_bytes::<Duration>(&bytes, &mut Budget::new(32)), Err(DecodeError::InvalidNanos(1_000_000_000)));
    }
}
