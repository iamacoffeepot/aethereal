//! Base64 (RFC 4648, section 4): what `Basic` credentials are written in (RFC 7617), and MCP's `Mcp-Name` may be, and
//! an image's bytes are.

/// The digits, each where its bits put it.
const DIGITS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The length of `bytes` bytes written in padded Base64.
#[must_use]
pub const fn encoded_len(bytes: usize) -> usize {
    bytes.div_ceil(3) * 4
}

/// `bytes` written in padded Base64 onto the end of `out`.
pub fn encode(bytes: &[u8], out: &mut String) {
    out.reserve(encoded_len(bytes.len()));
    for group in bytes.chunks(3) {
        let bits = group.iter().fold(0_u32, |bits, byte| bits << 8 | u32::from(*byte)) << (8 * (3 - group.len()));
        for at in 0..4 {
            // Six bits pick one of the 64 digits.
            let digit = DIGITS.get((bits >> (18 - 6 * at) & 0x3F) as usize).copied().unwrap_or(b'=');
            let written = at <= group.len();
            let shown = if written {
                char::from(digit)
            } else {
                '='
            };
            out.push(shown);
        }
    }
}

/// The bits a digit stands for: none when it is no digit.
const fn sextet(digit: u8) -> Option<u32> {
    let value = match digit {
        b'A'..=b'Z' => digit - b'A',
        b'a'..=b'z' => digit - b'a' + 26,
        b'0'..=b'9' => digit - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };

    Some(value as u32)
}

/// The bytes `text` states: none when it is not padded Base64.
#[must_use]
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(4) {
        return None;
    }
    let body = text.trim_end_matches('=').as_bytes();
    let padding = digits.len() - body.len();
    if padding > 2 {
        return None;
    }
    let mut out = Vec::with_capacity(body.len() / 4 * 3 + 2);
    for group in body.chunks(4) {
        let mut bits = 0;
        for digit in group {
            bits = bits << 6 | sextet(*digit)?;
        }
        let bytes = group.len() - 1;
        let packed = bits << (6 * (4 - group.len()));
        let written = packed.to_be_bytes();
        let unused = packed & (0x00FF_FFFF >> (8 * bytes));
        if unused != 0 {
            return None;
        }
        out.extend_from_slice(written.get(1..=bytes)?);
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode, encoded_len};

    #[test]
    fn it_reads_every_padding_and_refuses_what_is_not_it() {
        // Catches a group's bits shifted short, padding counted as data, and a stray digit or length taken.
        assert_eq!(decode(""), Some(Vec::new()));
        assert_eq!(decode("Zg=="), Some(b"f".to_vec()));
        assert_eq!(decode("Zm8="), Some(b"fo".to_vec()));
        assert_eq!(decode("Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(decode("Zm9vYg=="), Some(b"foob".to_vec()));
        assert_eq!(decode("Zm9"), None);
        assert_eq!(decode("Zm9*"), None);
        assert_eq!(decode("Z==="), None);
        assert_eq!(decode("Zh=="), None);
    }

    #[test]
    fn it_writes_every_padding_and_reads_back_what_it_wrote() {
        // Catches a digit taken from the wrong bits, padding left off or written in place of a digit, and a last group
        // of one or two bytes written as three. The vectors are RFC 4648's.
        let vectors = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (bytes, text) in vectors {
            let mut written = String::new();
            encode(bytes.as_bytes(), &mut written);
            assert_eq!(written, text);
            assert_eq!(written.len(), encoded_len(bytes.len()));
        }
        let every: Vec<u8> = (0..=255).collect();
        let mut written = String::new();
        encode(&every, &mut written);
        assert_eq!(decode(&written), Some(every));
    }
}
