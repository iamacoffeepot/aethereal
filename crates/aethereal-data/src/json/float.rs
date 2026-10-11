//! A float's bytes and text, both ways.

use super::{JsonFault, JsonForm};
use crate::plain::read_array;
use crate::{DecodeError, Float};
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

/// Writes the float at the front of `input` as its JSON form.
pub(super) fn write(kind: Float, input: &mut &[u8], out: &mut String) -> Result<(), DecodeError> {
    match kind {
        Float::F32 => {
            let bytes = read_array(input)?;
            let value = f32::from_le_bytes(bytes);
            let nan = value.is_nan();
            if nan {
                out.push_str("\"NaN\"");
                return Ok(());
            }
            let infinite = value.is_infinite();
            if infinite {
                let negative = value < 0.0;
                if negative {
                    out.push_str("\"-Infinity\"");
                } else {
                    out.push_str("\"Infinity\"");
                }
                return Ok(());
            }
            let _ = write!(out, "{value}");
            Ok(())
        }
        Float::F64 => {
            let bytes = read_array(input)?;
            let value = f64::from_le_bytes(bytes);
            let nan = value.is_nan();
            if nan {
                out.push_str("\"NaN\"");
                return Ok(());
            }
            let infinite = value.is_infinite();
            if infinite {
                let negative = value < 0.0;
                if negative {
                    out.push_str("\"-Infinity\"");
                } else {
                    out.push_str("\"Infinity\"");
                }
                return Ok(());
            }
            let _ = write!(out, "{value}");
            Ok(())
        }
    }
}

/// Appends the bytes of the float a JSON number's text states; `OutOfRange` when it is not finite.
pub(super) fn from_number(kind: Float, text: &str, out: &mut Vec<u8>) -> Result<(), JsonFault> {
    match kind {
        Float::F32 => {
            let parsed = text.parse::<f32>().map_err(|_| JsonFault::OutOfRange)?;
            let finite = parsed.is_finite();
            if !finite {
                return Err(JsonFault::OutOfRange);
            }
            out.extend_from_slice(&parsed.to_le_bytes());
            Ok(())
        }
        Float::F64 => {
            let parsed = text.parse::<f64>().map_err(|_| JsonFault::OutOfRange)?;
            let finite = parsed.is_finite();
            if !finite {
                return Err(JsonFault::OutOfRange);
            }
            out.extend_from_slice(&parsed.to_le_bytes());
            Ok(())
        }
    }
}

/// Appends the bytes for `"NaN"`, `"Infinity"`, or `"-Infinity"`; `Expected(JsonForm::Float)` for any other string.
pub(super) fn from_name(kind: Float, name: &str, out: &mut Vec<u8>) -> Result<(), JsonFault> {
    let nan = name == "NaN";
    let positive = name == "Infinity";
    let negative = name == "-Infinity";
    if nan {
        match kind {
            Float::F32 => {
                let quiet = f32::from_bits(0x7fc0_0000);
                out.extend_from_slice(&quiet.to_le_bytes());
            }
            Float::F64 => {
                let quiet = f64::from_bits(0x7ff8_0000_0000_0000);
                out.extend_from_slice(&quiet.to_le_bytes());
            }
        }
        return Ok(());
    }
    if positive {
        match kind {
            Float::F32 => {
                out.extend_from_slice(&f32::INFINITY.to_le_bytes());
            }
            Float::F64 => {
                out.extend_from_slice(&f64::INFINITY.to_le_bytes());
            }
        }
        return Ok(());
    }
    if negative {
        match kind {
            Float::F32 => {
                out.extend_from_slice(&f32::NEG_INFINITY.to_le_bytes());
            }
            Float::F64 => {
                out.extend_from_slice(&f64::NEG_INFINITY.to_le_bytes());
            }
        }
        return Ok(());
    }

    Err(JsonFault::Expected(JsonForm::Float))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn f32_tenth_writes_short() {
        // Catches widening to `f64`, which writes `0.10000000149011612`.
        let mut input: &[u8] = &[0xcd, 0xcc, 0xcc, 0x3d];
        let mut out = String::new();
        assert_eq!(write(Float::F32, &mut input, &mut out), Ok(()));
        assert_eq!(out, "0.1");
    }

    #[test]
    fn f32_reads_without_double_rounding() {
        // Catches reading through `f64`, which gives `0x3f80_0002`.
        let mut out = Vec::new();
        assert_eq!(from_number(Float::F32, "1.00000017881393432617187499", &mut out), Ok(()));
        assert_eq!(out, vec![0x01, 0x00, 0x80, 0x3f]);
    }

    #[test]
    fn negative_zero_both_ways() {
        // Catches a writer that loses the sign and a reader that misplaces it.
        let mut input32: &[u8] = &[0x00, 0x00, 0x00, 0x80];
        let mut text32 = String::new();
        assert_eq!(write(Float::F32, &mut input32, &mut text32), Ok(()));
        assert_eq!(text32, "-0");
        let mut bytes32 = Vec::new();
        assert_eq!(from_number(Float::F32, "-0", &mut bytes32), Ok(()));
        assert_eq!(bytes32, vec![0x00, 0x00, 0x00, 0x80]);

        let mut input64: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80];
        let mut text64 = String::new();
        assert_eq!(write(Float::F64, &mut input64, &mut text64), Ok(()));
        assert_eq!(text64, "-0");
        let mut bytes64 = Vec::new();
        assert_eq!(from_number(Float::F64, "-0", &mut bytes64), Ok(()));
        assert_eq!(bytes64, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80]);
    }

    #[test]
    fn f64_max_writes_long_and_reads_back() {
        // Catches an exponent the writer must never emit.
        let max = f64::MAX.to_le_bytes();
        let mut input: &[u8] = &max;
        let mut out = String::new();
        assert_eq!(write(Float::F64, &mut input, &mut out), Ok(()));
        let mut expected = String::from("17976931348623157");
        expected.push_str(&"0".repeat(292));
        assert_eq!(out, expected);
        let mut back = Vec::new();
        assert_eq!(from_number(Float::F64, &out, &mut back), Ok(()));
        assert_eq!(back, max.to_vec());
    }

    #[test]
    fn f64_tiny_subnormal_writes_long_and_reads_back() {
        // Catches a writer that shortens past what reads back as the same bits.
        let tiny = 1u64.to_le_bytes();
        let mut input: &[u8] = &tiny;
        let mut out = String::new();
        assert_eq!(write(Float::F64, &mut input, &mut out), Ok(()));
        let mut expected = String::from("0.");
        expected.push_str(&"0".repeat(323));
        expected.push('5');
        assert_eq!(out, expected);
        let mut back = Vec::new();
        assert_eq!(from_number(Float::F64, &out, &mut back), Ok(()));
        assert_eq!(back, tiny.to_vec());
    }

    #[test]
    fn f32_max_writes_long() {
        // Catches a 32-bit value formatted at the wrong width.
        let mut input: &[u8] = &[0xff, 0xff, 0x7f, 0x7f];
        let mut out = String::new();
        assert_eq!(write(Float::F32, &mut input, &mut out), Ok(()));
        let mut expected = String::from("34028235");
        expected.push_str(&"0".repeat(31));
        assert_eq!(out, expected);
    }

    #[test]
    fn whole_value_writes_no_fraction() {
        // Catches a writer that emits `2.0`.
        let mut input: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40];
        let mut out = String::new();
        assert_eq!(write(Float::F64, &mut input, &mut out), Ok(()));
        assert_eq!(out, "2");
    }

    #[test]
    fn too_large_is_out_of_range_and_too_small_is_zero() {
        // Catches a reader that wraps or refuses a value that rounds to zero.
        let mut out = Vec::new();
        assert_eq!(from_number(Float::F64, "1e400", &mut out), Err(JsonFault::OutOfRange));
        assert_eq!(from_number(Float::F32, "1e39", &mut out), Err(JsonFault::OutOfRange));
        let mut zero = Vec::new();
        assert_eq!(from_number(Float::F64, "1e-400", &mut zero), Ok(()));
        assert_eq!(zero, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn nan_and_infinities_both_ways() {
        // Catches a NaN check that looks at bits instead of `is_nan`.
        let mut dirty: &[u8] = &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x7f];
        let mut text = String::new();
        assert_eq!(write(Float::F64, &mut dirty, &mut text), Ok(()));
        assert_eq!(text, "\"NaN\"");
        let mut dirty32: &[u8] = &[0x01, 0x00, 0xc0, 0xff];
        let mut text32 = String::new();
        assert_eq!(write(Float::F32, &mut dirty32, &mut text32), Ok(()));
        assert_eq!(text32, "\"NaN\"");

        let mut quiet64 = Vec::new();
        assert_eq!(from_name(Float::F64, "NaN", &mut quiet64), Ok(()));
        assert_eq!(quiet64, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf8, 0x7f]);
        let mut quiet32 = Vec::new();
        assert_eq!(from_name(Float::F32, "NaN", &mut quiet32), Ok(()));
        assert_eq!(quiet32, vec![0x00, 0x00, 0xc0, 0x7f]);

        let mut positive: &[u8] = &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x7f];
        let mut positive_text = String::new();
        assert_eq!(write(Float::F64, &mut positive, &mut positive_text), Ok(()));
        assert_eq!(positive_text, "\"Infinity\"");
        let mut positive_back = Vec::new();
        assert_eq!(from_name(Float::F64, "Infinity", &mut positive_back), Ok(()));
        assert_eq!(positive_back, vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xf0, 0x7f]);

        let mut negative: &[u8] = &[0x00, 0x00, 0x80, 0xff];
        let mut negative_text = String::new();
        assert_eq!(write(Float::F32, &mut negative, &mut negative_text), Ok(()));
        assert_eq!(negative_text, "\"-Infinity\"");
        let mut negative_back = Vec::new();
        assert_eq!(from_name(Float::F32, "-Infinity", &mut negative_back), Ok(()));
        assert_eq!(negative_back, vec![0x00, 0x00, 0x80, 0xff]);
    }

    #[test]
    fn other_names_are_not_floats() {
        // Catches a name read that leans on `parse`, which accepts `inf` and `nan`.
        let mut out = Vec::new();
        let expected = Err(JsonFault::Expected(JsonForm::Float));
        assert_eq!(from_name(Float::F64, "nan", &mut out), expected);
        assert_eq!(from_name(Float::F64, "inf", &mut out), expected);
        assert_eq!(from_name(Float::F64, "infinity", &mut out), expected);
        assert_eq!(from_name(Float::F64, "Infinity ", &mut out), expected);
    }
}
