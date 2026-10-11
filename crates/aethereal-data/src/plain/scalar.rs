//! `bool`, the integers, the floats, `char`, and the `NonZero` integers (ADR-0004).

use crate::plain::read_array;
use crate::{Budget, DecodeError, EncodeError, Float, Integer, Plain, Schema, Shape};
use alloc::vec::Vec;
use core::num::{NonZeroI8, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroU8, NonZeroU16, NonZeroU32, NonZeroU64};

macro_rules! integer {
    ($t:ty, $kind:expr) => {
        impl Schema for $t {
            const SHAPE: Shape = Shape::Integer($kind);
        }

        integer_codec!($t);
    };
    ($t:ty, $kind:expr, $sequence:expr) => {
        impl Schema for $t {
            const SHAPE: Shape = Shape::Integer($kind);
            const SEQUENCE: Shape = $sequence;
        }

        integer_codec!($t);
    };
}

macro_rules! integer_codec {
    ($t:ty) => {
        impl Plain for $t {
            fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
                out.extend_from_slice(&self.to_le_bytes());
                Ok(())
            }

            fn decode(input: &mut &[u8], _budget: &mut Budget) -> Result<Self, DecodeError> {
                Ok(Self::from_le_bytes(read_array(input)?))
            }
        }
    };
}

integer!(u8, Integer::U8, Shape::Bytes);
integer!(u16, Integer::U16);
integer!(u32, Integer::U32);
integer!(u64, Integer::U64);
integer!(i8, Integer::I8);
integer!(i16, Integer::I16);
integer!(i32, Integer::I32);
integer!(i64, Integer::I64);

macro_rules! float {
    ($t:ty, $kind:expr) => {
        impl Schema for $t {
            const SHAPE: Shape = Shape::Float($kind);
        }

        impl Plain for $t {
            fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
                out.extend_from_slice(&self.to_le_bytes());
                Ok(())
            }

            fn decode(input: &mut &[u8], _budget: &mut Budget) -> Result<Self, DecodeError> {
                Ok(Self::from_le_bytes(read_array(input)?))
            }
        }
    };
}

float!(f32, Float::F32);
float!(f64, Float::F64);

impl Schema for bool {
    const SHAPE: Shape = Shape::Boolean;
}

impl Plain for bool {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        out.push(u8::from(*self));
        Ok(())
    }

    fn decode(input: &mut &[u8], _budget: &mut Budget) -> Result<Self, DecodeError> {
        let [marker] = read_array(input)?;
        match marker {
            0 => Ok(false),
            1 => Ok(true),
            other => Err(DecodeError::InvalidBoolean(other)),
        }
    }
}

impl Schema for char {
    const SHAPE: Shape = Shape::Integer(Integer::U32);
}

impl Plain for char {
    fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
        let value = u32::from(*self);
        out.extend_from_slice(&value.to_le_bytes());
        Ok(())
    }

    fn decode(input: &mut &[u8], _budget: &mut Budget) -> Result<Self, DecodeError> {
        let value = u32::from_le_bytes(read_array(input)?);
        Self::from_u32(value).ok_or(DecodeError::InvalidChar(value))
    }
}

macro_rules! nonzero {
    ($t:ty, $inner:ty, $kind:expr) => {
        impl Schema for $t {
            const SHAPE: Shape = Shape::Integer($kind);
        }

        impl Plain for $t {
            fn encode(&self, out: &mut Vec<u8>) -> Result<(), EncodeError> {
                out.extend_from_slice(&self.get().to_le_bytes());
                Ok(())
            }

            fn decode(input: &mut &[u8], budget: &mut Budget) -> Result<Self, DecodeError> {
                let raw = <$inner>::decode(input, budget)?;
                Self::new(raw).ok_or(DecodeError::Zero)
            }
        }
    };
}

nonzero!(NonZeroU8, u8, Integer::U8);
nonzero!(NonZeroU16, u16, Integer::U16);
nonzero!(NonZeroU32, u32, Integer::U32);
nonzero!(NonZeroU64, u64, Integer::U64);
nonzero!(NonZeroI8, i8, Integer::I8);
nonzero!(NonZeroI16, i16, Integer::I16);
nonzero!(NonZeroI32, i32, Integer::I32);
nonzero!(NonZeroI64, i64, Integer::I64);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{from_bytes, to_bytes};

    #[test]
    fn bool_bytes() {
        // Catches a swapped or multi-byte boolean encoding.
        assert_eq!(to_bytes(&false), Ok(alloc::vec![0]));
        assert_eq!(to_bytes(&true), Ok(alloc::vec![1]));
        assert_eq!(from_bytes::<bool>(&[0], &mut Budget::new(16)), Ok(false));
        assert_eq!(from_bytes::<bool>(&[1], &mut Budget::new(16)), Ok(true));
    }

    #[test]
    fn integers_are_little_endian() {
        // Catches a big-endian or variable-length integer encoding, one per width.
        assert_eq!(to_bytes(&0xABu8), Ok(alloc::vec![0xAB]));
        assert_eq!(to_bytes(&0x0201u16), Ok(alloc::vec![0x01, 0x02]));
        assert_eq!(to_bytes(&0x0403_0201u32), Ok(alloc::vec![0x01, 0x02, 0x03, 0x04]));
        assert_eq!(
            to_bytes(&0x0807_0605_0403_0201u64),
            Ok(alloc::vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08])
        );
        assert_eq!(to_bytes(&0x7Fi8), Ok(alloc::vec![0x7F]));
        assert_eq!(to_bytes(&0x0201i16), Ok(alloc::vec![0x01, 0x02]));
        assert_eq!(to_bytes(&0x0403_0201i32), Ok(alloc::vec![0x01, 0x02, 0x03, 0x04]));
        assert_eq!(
            to_bytes(&0x0807_0605_0403_0201i64),
            Ok(alloc::vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08])
        );
    }

    #[test]
    fn negative_integer_is_twos_complement() {
        // Catches a sign-magnitude encoding of negative integers.
        assert_eq!(to_bytes(&(-1i32)), Ok(alloc::vec![0xFF, 0xFF, 0xFF, 0xFF]));
        assert_eq!(from_bytes::<i32>(&[0xFF, 0xFF, 0xFF, 0xFF], &mut Budget::new(16)), Ok(-1));
    }

    #[test]
    fn nan_payload_bits_survive() {
        // Catches a float codec that canonicalizes NaN payloads.
        let value = f32::from_bits(0x7FC0_0001);
        assert_eq!(to_bytes(&value), Ok(alloc::vec![0x01, 0x00, 0xC0, 0x7F]));
        let decoded = from_bytes::<f32>(&[0x01, 0x00, 0xC0, 0x7F], &mut Budget::new(16)).map(f32::to_bits);
        assert_eq!(decoded, Ok(0x7FC0_0001));
        let wide = f64::from_bits(0x7FF8_0000_0000_0001);
        assert_eq!(to_bytes(&wide), Ok(alloc::vec![0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x7F]));
        let decoded_wide = from_bytes::<f64>(&[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF8, 0x7F], &mut Budget::new(16))
            .map(f64::to_bits);
        assert_eq!(decoded_wide, Ok(0x7FF8_0000_0000_0001));
    }

    #[test]
    fn char_is_its_code_point() {
        // Catches a `char` codec that writes UTF-8 instead of a `u32` code point.
        assert_eq!(to_bytes(&'A'), Ok(alloc::vec![0x41, 0x00, 0x00, 0x00]));
        assert_eq!(from_bytes::<char>(&[0x41, 0x00, 0x00, 0x00], &mut Budget::new(16)), Ok('A'));
    }

    #[test]
    fn nonzero_is_its_integer() {
        // Catches a `NonZero` codec that writes a marker beside the integer.
        let one = NonZeroU32::new(1);
        let is_some = one.is_some();
        assert!(is_some);
        if let Some(one) = one {
            assert_eq!(to_bytes(&one), Ok(alloc::vec![0x01, 0x00, 0x00, 0x00]));
            assert_eq!(from_bytes::<NonZeroU32>(&[0x01, 0x00, 0x00, 0x00], &mut Budget::new(16)), Ok(one));
        }
    }

    #[test]
    fn boolean_byte_two_is_refused() {
        // Catches a decoder that treats any nonzero byte as true.
        assert_eq!(from_bytes::<bool>(&[2], &mut Budget::new(16)), Err(DecodeError::InvalidBoolean(2)));
    }

    #[test]
    fn surrogate_char_is_refused() {
        // Catches a decoder that accepts surrogate code points as `char`.
        assert_eq!(
            from_bytes::<char>(&[0x00, 0xD8, 0x00, 0x00], &mut Budget::new(16)),
            Err(DecodeError::InvalidChar(0xD800))
        );
    }

    #[test]
    fn zero_for_nonzero_is_refused() {
        // Catches a decoder that accepts zero for a `NonZero` integer.
        assert_eq!(from_bytes::<NonZeroU8>(&[0], &mut Budget::new(16)), Err(DecodeError::Zero));
    }
}
