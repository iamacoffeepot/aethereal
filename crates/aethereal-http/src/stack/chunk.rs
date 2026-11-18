//! A chunked body's framing: the line before each chunk, and what ends the body (RFC 9112 §7.1). A line is written for
//! every part of a streamed body, so it is written straight into the buffer it is put from, and the end is a constant.

/// The line ending HTTP writes.
const LINE_END: &[u8] = b"\r\n";

/// The digits of a size in hex, by their value.
const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// The most bytes the line before a chunk takes: the line ending of the chunk before it, a size of up to 64 bits in
/// hex, and its own line ending.
pub const LINE_BYTES: usize = 2 + 16 + 2;

/// What ends a chunked body with no chunk before it: the last chunk and its empty trailer.
const LAST: &[u8] = b"0\r\n\r\n";

/// What ends a chunked body after a chunk: that chunk's line ending, then the last chunk and its empty trailer.
const LAST_AFTER: &[u8] = b"\r\n0\r\n\r\n";

/// Writes the line before a chunk of `length` bytes, more than none, into `line`: its size in hex, after the line
/// ending of the chunk before it when one `follows`. It takes at most [`LINE_BYTES`].
pub fn line(length: usize, follows: bool, line: &mut Vec<u8>) {
    if follows {
        line.extend_from_slice(LINE_END);
    }
    let digits = (usize::BITS - length.leading_zeros()).div_ceil(4).max(1);
    for place in (0..digits).rev() {
        line.push(HEX_DIGITS[(length >> (place * 4)) & 0xf]);
    }
    line.extend_from_slice(LINE_END);
}

/// What ends a chunked body: the last chunk and its empty trailer, after the line ending of the chunk before it when
/// one `follows`.
pub const fn last(follows: bool) -> &'static [u8] {
    if follows {
        LAST_AFTER
    } else {
        LAST
    }
}

#[cfg(test)]
mod tests {
    use super::{LINE_BYTES, last, line};

    #[test]
    fn chunk_lines_end_the_chunk_before_them_and_the_last_ends_the_body() {
        // Catches a chunk's size written in decimal, the line ending after a chunk's data lost or written before the
        // first chunk, and a body ended without its empty trailer, any of which a client reads as a broken body.
        let mut body = Vec::new();
        line(26, false, &mut body);
        body.extend_from_slice(b"abcdefghijklmnopqrstuvwxyz");
        line(1, true, &mut body);
        body.extend_from_slice(b"!");
        body.extend_from_slice(last(true));

        assert_eq!(body, b"1a\r\nabcdefghijklmnopqrstuvwxyz\r\n1\r\n!\r\n0\r\n\r\n");
        assert_eq!(last(false), b"0\r\n\r\n");
    }

    #[test]
    fn a_chunks_size_takes_as_many_hex_digits_as_it_needs() {
        // Catches a size written with a digit too few or too many, or its digits out of order, where a size gains a
        // digit and at the largest size, and a line past the room the stack gives it. `format!`'s hex is the oracle.
        for length in [1, 0xf, 0x10, 0xff, 0x100, 0x00ab_cdef, usize::MAX] {
            let mut written = Vec::new();
            line(length, true, &mut written);

            assert_eq!(written, format!("\r\n{length:x}\r\n").into_bytes(), "{length:#x}");
            assert!(written.len() <= LINE_BYTES, "{length:#x} fits its room");
        }
    }
}
