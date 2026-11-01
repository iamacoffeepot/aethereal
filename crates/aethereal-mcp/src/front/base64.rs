//! Base64 (RFC 4648, section 4), which `Mcp-Name` may be written in.

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
