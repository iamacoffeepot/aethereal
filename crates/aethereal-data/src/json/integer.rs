//! An integer's bytes, value, and text, both ways.

use super::JsonFault;
use crate::plain::read_array;
use crate::{DecodeError, Integer};
use alloc::vec::Vec;

/// Whether the integer is written as a string of digits: the two 64-bit kinds.
pub(super) fn is_wide(kind: Integer) -> bool {
    match kind {
        Integer::U64 | Integer::I64 => true,
        Integer::U8 | Integer::U16 | Integer::U32 | Integer::I8 | Integer::I16 | Integer::I32 => false,
    }
}

/// Reads the integer at the front of `input` (ADR-0002: its width, little-endian).
pub(super) fn take(kind: Integer, input: &mut &[u8]) -> Result<i128, DecodeError> {
    match kind {
        Integer::U8 => {
            let bytes = read_array(input)?;
            let value = u8::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::U16 => {
            let bytes = read_array(input)?;
            let value = u16::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::U32 => {
            let bytes = read_array(input)?;
            let value = u32::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::U64 => {
            let bytes = read_array(input)?;
            let value = u64::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::I8 => {
            let bytes = read_array(input)?;
            let value = i8::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::I16 => {
            let bytes = read_array(input)?;
            let value = i16::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::I32 => {
            let bytes = read_array(input)?;
            let value = i32::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
        Integer::I64 => {
            let bytes = read_array(input)?;
            let value = i64::from_le_bytes(bytes);
            Ok(i128::from(value))
        }
    }
}

/// Appends `value` at the width of `kind`; `OutOfRange` when it does not fit.
pub(super) fn append(kind: Integer, value: i128, out: &mut Vec<u8>) -> Result<(), JsonFault> {
    match kind {
        Integer::U8 => {
            let narrow = u8::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::U16 => {
            let narrow = u16::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::U32 => {
            let narrow = u32::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::U64 => {
            let narrow = u64::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::I8 => {
            let narrow = i8::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::I16 => {
            let narrow = i16::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::I32 => {
            let narrow = i32::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
        Integer::I64 => {
            let narrow = i64::try_from(value).map_err(|_| JsonFault::OutOfRange)?;
            out.extend_from_slice(&narrow.to_le_bytes());
            Ok(())
        }
    }
}

/// The value of a JSON number's text; `NotAnInteger` when it has a fraction or an exponent.
pub(super) fn from_number(text: &str) -> Result<i128, JsonFault> {
    let has_dot = text.contains('.');
    let has_lower = text.contains('e');
    let has_upper = text.contains('E');
    let has_fraction = has_dot || has_lower || has_upper;
    if has_fraction {
        return Err(JsonFault::NotAnInteger);
    }

    let parsed = text.parse::<i128>().map_err(|_| JsonFault::OutOfRange)?;
    Ok(parsed)
}

/// The value of a string of digits; `NotDecimal` unless it is `0` or an optional `-`, a digit `1`-`9`, then digits.
pub(super) fn from_digits(text: &str) -> Result<i128, JsonFault> {
    let bytes = text.as_bytes();
    let empty = bytes.is_empty();
    if empty {
        return Err(JsonFault::NotDecimal);
    }

    let Some(first) = bytes.first() else {
        return Err(JsonFault::NotDecimal);
    };
    let negative = *first == b'-';
    if negative {
        let Some(rest) = bytes.get(1..) else {
            return Err(JsonFault::NotDecimal);
        };
        let empty_rest = rest.is_empty();
        if empty_rest {
            return Err(JsonFault::NotDecimal);
        }
        let Some(second) = rest.first() else {
            return Err(JsonFault::NotDecimal);
        };
        let second_value = *second;
        let low = second_value >= b'1';
        let high = second_value <= b'9';
        let nonzero = low && high;
        if !nonzero {
            return Err(JsonFault::NotDecimal);
        }
        let digits = all_digits(rest);
        if !digits {
            return Err(JsonFault::NotDecimal);
        }
    } else {
        let first_value = *first;
        let is_zero = first_value == b'0';
        if is_zero {
            let single = bytes.len() == 1;
            if !single {
                return Err(JsonFault::NotDecimal);
            }
        } else {
            let low = first_value >= b'1';
            let high = first_value <= b'9';
            let nonzero = low && high;
            if !nonzero {
                return Err(JsonFault::NotDecimal);
            }
            let digits = all_digits(bytes);
            if !digits {
                return Err(JsonFault::NotDecimal);
            }
        }
    }

    let parsed = text.parse::<i128>().map_err(|_| JsonFault::OutOfRange)?;
    Ok(parsed)
}

/// Whether every byte is a decimal digit.
fn all_digits(bytes: &[u8]) -> bool {
    for byte in bytes {
        let value = *byte;
        let low = value >= b'0';
        let high = value <= b'9';
        let digit = low && high;
        if !digit {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_refuses_fraction_and_reads_minus_zero() {
        // Catches an integer read that goes through a float.
        assert_eq!(from_number("1.0"), Err(JsonFault::NotAnInteger));
        assert_eq!(from_number("1e2"), Err(JsonFault::NotAnInteger));
        assert_eq!(from_number("-0"), Ok(0));
    }

    #[test]
    fn append_refuses_outside_width() {
        // Catches a narrowing write that wraps instead of refusing.
        let mut out = Vec::new();
        assert_eq!(append(Integer::U8, 256, &mut out), Err(JsonFault::OutOfRange));
        assert_eq!(append(Integer::U8, -1, &mut out), Err(JsonFault::OutOfRange));
        assert_eq!(append(Integer::I8, -129, &mut out), Err(JsonFault::OutOfRange));
        assert_eq!(append(Integer::U64, 18_446_744_073_709_551_616, &mut out), Err(JsonFault::OutOfRange));
        assert_eq!(append(Integer::I64, -9_223_372_036_854_775_809, &mut out), Err(JsonFault::OutOfRange));
    }

    #[test]
    fn append_accepts_each_limit() {
        // Catches a range check that is off by one at either end.
        let limits = [
            (Integer::U8, 0, 255),
            (Integer::U16, 0, 65_535),
            (Integer::U32, 0, 4_294_967_295),
            (Integer::U64, 0, 18_446_744_073_709_551_615),
            (Integer::I8, -128, 127),
            (Integer::I16, -32_768, 32_767),
            (Integer::I32, -2_147_483_648, 2_147_483_647),
            (Integer::I64, -9_223_372_036_854_775_808, 9_223_372_036_854_775_807),
        ];
        for (kind, low, high) in limits {
            let mut low_bytes = Vec::new();
            let mut high_bytes = Vec::new();
            assert_eq!(append(kind, low, &mut low_bytes), Ok(()));
            assert_eq!(append(kind, high, &mut high_bytes), Ok(()));
        }
    }

    #[test]
    fn digits_refuse_what_the_writer_never_writes() {
        // Catches a digit read that leans on `parse` alone, which accepts `+` and leading zeros.
        assert_eq!(from_digits("+1"), Err(JsonFault::NotDecimal));
        assert_eq!(from_digits("01"), Err(JsonFault::NotDecimal));
        assert_eq!(from_digits("-0"), Err(JsonFault::NotDecimal));
        assert_eq!(from_digits(""), Err(JsonFault::NotDecimal));
        assert_eq!(from_digits("1.0"), Err(JsonFault::NotDecimal));
        assert_eq!(from_digits("-"), Err(JsonFault::NotDecimal));
        assert_eq!(from_digits("18446744073709551615"), Ok(18_446_744_073_709_551_615));
        assert_eq!(from_digits("-9223372036854775808"), Ok(-9_223_372_036_854_775_808));
    }

    #[test]
    fn take_keeps_the_sign_on_widening() {
        // Catches a widening read that zero-extends a negative value.
        let mut input: &[u8] = &[0x00, 0x80];
        assert_eq!(take(Integer::I16, &mut input), Ok(-32_768));
    }
}
