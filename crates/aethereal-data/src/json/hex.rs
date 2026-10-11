//! Bytes as lowercase hexadecimal, both ways.

use super::JsonFault;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write as _;

/// Appends each byte as two lowercase digits.
pub(super) fn write(bytes: &[u8], out: &mut String) {
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
}

/// Appends the bytes `text` states; `OddHexLength` or `HexDigit` otherwise.
pub(super) fn read(text: &str, out: &mut Vec<u8>) -> Result<(), JsonFault> {
    let bytes = text.as_bytes();
    let (pairs, rest) = bytes.as_chunks::<2>();
    let odd = !rest.is_empty();
    if odd {
        return Err(JsonFault::OddHexLength);
    }

    for pair in pairs {
        let [high_byte, low_byte] = *pair;
        let high = digit(high_byte)?;
        let low = digit(low_byte)?;
        let byte = (high << 4) | low;
        out.push(byte);
    }

    Ok(())
}

/// The value of one lowercase hexadecimal digit.
fn digit(byte: u8) -> Result<u8, JsonFault> {
    match byte {
        b'0'..=b'9' => {
            let value = byte - b'0';
            Ok(value)
        }
        b'a'..=b'f' => {
            let value = byte - b'a' + 10;
            Ok(value)
        }
        _ => Err(JsonFault::HexDigit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn bytes_round_trip_as_lowercase() {
        // Catches a writer that uppercases or a reader that rejects lowercase.
        let mut text = String::new();
        write(&[0x00, 0xab, 0xff], &mut text);
        assert_eq!(text, "00abff");
        let mut bytes = Vec::new();
        let result = read(&text, &mut bytes);
        assert_eq!(result, Ok(()));
        assert_eq!(bytes, vec![0x00, 0xab, 0xff]);
    }

    #[test]
    fn uppercase_is_not_hex() {
        // Catches a reader that accepts uppercase digits.
        let mut bytes = Vec::new();
        assert_eq!(read("0F", &mut bytes), Err(JsonFault::HexDigit));
    }

    #[test]
    fn odd_length_is_refused() {
        // Catches a reader that drops a lone nibble instead of refusing.
        let mut bytes = Vec::new();
        assert_eq!(read("abc", &mut bytes), Err(JsonFault::OddHexLength));
    }

    #[test]
    fn empty_is_no_bytes() {
        // Catches a reader that demands at least one pair.
        let mut bytes = Vec::new();
        assert_eq!(read("", &mut bytes), Ok(()));
        assert_eq!(bytes, vec![]);
    }
}
