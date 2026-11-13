use super::*;

/// The most bytes of a chunk's line the tests read.
const MOST_LINE: u32 = 32;

/// A chunked body of two chunks, the first with an extension, and a trailer field after the last.
const CHUNKED: &[u8] = b"5;name=value\r\nhello\r\n7\r\n, world\r\n0\r\nx-trailer: passed\r\n\r\n";

/// What reading `body` from `buffer` came to: the body's bytes, how many of `buffer` it took, and whether it ended.
fn read(body: &mut Body, buffer: &[u8]) -> Result<(Vec<u8>, usize, Progress), Failure> {
    let mut out = Vec::new();
    let Decoded { consumed, progress } = body.decode(buffer, &mut out, MOST_LINE)?;

    Ok((out, consumed, progress))
}

#[test]
fn a_chunked_body_cut_anywhere_between_reads_decodes_the_same_and_ends_where_it_says() {
    // Catches a size line, a chunk's data, the line ending after it, or a trailer read as whole before it has all
    // arrived; bytes lost or read twice where one read ends and the next begins; and a body that ends before its last
    // chunk's trailer, so the trailer is read as the next response.
    for cut in 0..=CHUNKED.len() {
        let mut body = Body::Chunked { at: Chunk::Size };
        let mut buffer = CHUNKED[..cut].to_vec();
        let mut decoded = Vec::new();

        let Ok((first, consumed, _)) = read(&mut body, &buffer) else {
            panic!("the first {cut} bytes decode");
        };
        decoded.extend(first);
        buffer.drain(..consumed);
        buffer.extend_from_slice(&CHUNKED[cut..]);
        let Ok((rest, consumed, progress)) = read(&mut body, &buffer) else {
            panic!("the rest after {cut} bytes decodes");
        };
        decoded.extend(rest);

        assert_eq!(decoded, b"hello, world", "cut at {cut}");
        assert_eq!(progress, Progress::Done, "cut at {cut}");
        assert_eq!(consumed, buffer.len(), "cut at {cut}: the whole body is taken, and nothing after it");
    }
}

#[test]
fn a_body_by_its_length_takes_its_length_and_leaves_what_follows() {
    // Catches a body that takes the next response's bytes, or ends before its last byte.
    let mut body = Body::Length { left: 5 };

    assert_eq!(read(&mut body, b"hel"), Ok((b"hel".to_vec(), 3, Progress::More)));
    assert_eq!(read(&mut body, b"loHTTP/1.1"), Ok((b"lo".to_vec(), 2, Progress::Done)));
}

#[test]
fn a_chunked_body_that_breaks_its_framing_fails() {
    // Catches a size that is not hexadecimal read as some other number, a size past what a `u64` holds wrapped
    // around, data not followed by its line ending read on as the next size, and a size line that never ends held
    // without bound.
    let malformed = [b"g\r\n".as_slice(), b"\r\n", b"10000000000000000\r\n", b"5\r\nhelloX\r\n", b"5\nhello\r\n"];

    for bytes in malformed {
        let mut body = Body::Chunked { at: Chunk::Size };
        assert_eq!(read(&mut body, bytes), Err(Failure::Malformed), "{}", String::from_utf8_lossy(bytes));
    }
    let mut body = Body::Chunked { at: Chunk::Size };
    let endless = [b"5;".as_slice(), &[b'x'; 64]].concat();
    assert_eq!(read(&mut body, &endless), Err(Failure::TooLarge(Limit::Head)));
}
