use super::*;

/// The most bytes of a head the tests parse.
const MOST: u32 = 128;

/// What the front of `bytes` parses as, a response to a request other than `HEAD`.
fn front(bytes: &[u8]) -> Parsed {
    head(bytes, MOST, false)
}

/// The head `bytes` parses to, whole.
fn parsed(bytes: &[u8]) -> Head {
    let Parsed::Head { head, .. } = front(bytes) else {
        panic!("a whole head parses: {}", String::from_utf8_lossy(bytes));
    };

    head
}

#[test]
fn a_head_parses_to_its_status_and_lowercased_headers_and_says_where_it_ended() {
    // Catches a status read from the wrong bytes, a header name kept in the case it was sent so a lookup misses it,
    // and a consumed count that loses or keeps a byte of the body after it: bytes that arrived with the head are the
    // body's, or, after a switch, the connection's.
    let bytes = b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.raw-stream\r\nUpgrade: tcp\r\n\r\nraw";

    let Parsed::Head { head, consumed } = front(bytes) else {
        panic!("a whole head parses");
    };

    assert_eq!(head.status, 101);
    let headers = [Header::new("content-type", "application/vnd.docker.raw-stream"), Header::new("upgrade", "tcp")];
    assert_eq!(head.headers, headers);
    assert_eq!(&bytes[consumed..], b"raw");
}

#[test]
fn a_body_is_framed_as_the_head_says() {
    // Catches a body read by its length when it is chunked or the other way round, a response with no body (one to a
    // `HEAD`, a `204`, a `304`, an informational one) read as having one so the client waits for bytes that never come,
    // and a body with neither framing read as empty when it runs until the connection closes.
    let framed = [
        (b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\n".as_slice(), Framing::Length(12)),
        (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: Chunked\r\n\r\n", Framing::Chunked),
        (b"HTTP/1.1 200 OK\r\n\r\n", Framing::Close),
        (b"HTTP/1.1 204 No Content\r\nContent-Length: 12\r\n\r\n", Framing::Empty),
        (b"HTTP/1.1 304 Not Modified\r\nContent-Length: 12\r\n\r\n", Framing::Empty),
        (b"HTTP/1.1 100 Continue\r\n\r\n", Framing::Empty),
    ];

    for (bytes, framing) in framed {
        assert_eq!(parsed(bytes).framing, framing, "{}", String::from_utf8_lossy(bytes));
    }
    let Parsed::Head { head, .. } = head(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\n", MOST, true) else {
        panic!("the head of a response to a HEAD parses");
    };
    assert_eq!(head.framing, Framing::Empty, "a response to a HEAD has no body, whatever its length says");
}

#[test]
fn a_head_says_when_its_connection_closes() {
    // Catches `Connection: close` missed among other options, an HTTP/1.0 response kept alive, and a body that runs
    // until the connection closes kept alive after, so the next request would be written to a closed connection.
    assert!(parsed(b"HTTP/1.1 200 OK\r\nConnection: keep-alive, Close\r\nContent-Length: 0\r\n\r\n").closes);
    assert!(parsed(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n").closes);
    assert!(parsed(b"HTTP/1.1 200 OK\r\n\r\n").closes);
    assert!(!parsed(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n").closes);
}

#[test]
fn a_head_not_yet_whole_waits_and_one_past_the_limit_or_the_rules_fails() {
    // Catches a partial head parsed as whole; a head that never ends held without bound; a response smuggled past
    // the client by a length beside a coding, or a coding it does not decode read as the body; a doubled length; a
    // status that is not three digits, or a version it does not read; and a line spliced by a bare line feed.
    let long = [b"HTTP/1.1 200 OK\r\nX: ".as_slice(), &[b'a'; 128]].concat();
    let malformed = [
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 4\r\n\r\n".as_slice(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: gzip\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n",
        b"HTTP/1.1 20 OK\r\n\r\n",
        b"HTTP/1.1 2000 OK\r\n\r\n",
        b"HTTP/1.1 099 Early\r\n\r\n",
        b"HTTP/2 200 OK\r\n\r\n",
        b"HTTP/1.1 200 OK\nContent-Length: 0\r\n\r\n",
    ];

    assert_eq!(front(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n"), Parsed::More);
    assert_eq!(front(&long), Parsed::Failed(Failure::TooLarge(Limit::Head)));
    for bytes in malformed {
        assert_eq!(front(bytes), Parsed::Failed(Failure::Malformed), "{}", String::from_utf8_lossy(bytes));
    }
    assert_eq!(parsed(b"HTTP/1.1 200\r\nContent-Length: 0\r\n\r\n").status, 200, "a reason phrase may be left out");
}
