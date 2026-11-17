use super::*;

/// What the front of `bytes` parses as, a head of at most 128 bytes, and a body of at most 16.
fn front(bytes: &[u8]) -> Parsed {
    parse(bytes, 128, |_| 16)
}

#[test]
fn a_head_parses_to_its_method_path_query_and_lowercased_headers_and_says_where_it_ended() {
    // Catches a query left on the path, a header name kept in the case it was sent so a handler's lookup misses it,
    // whitespace left around a value, empty lines before a request read as its request line, and a consumed count
    // that loses or keeps a byte of the body that follows.
    let bytes = b"\r\nPOST /mcp?session=7 HTTP/1.1\r\nHost: here\r\nContent-Length:  4 \r\n\r\nbody";

    let Parsed::Head { head, consumed } = front(bytes) else {
        panic!("a whole head parses");
    };

    assert_eq!(head.method, Method::Post);
    assert_eq!((head.path.as_str(), head.query.as_str()), ("/mcp", "session=7"));
    assert_eq!(head.headers, [Header::new("host", "here"), Header::new("content-length", "4")]);
    assert_eq!(head.framing, Framing::Length(4));
    assert_eq!(&bytes[consumed..], b"body");
    assert!(!head.closes, "HTTP/1.1 keeps the connection");
}

#[test]
fn a_head_says_when_its_connection_closes_and_when_its_client_waits_to_send_the_body() {
    // Catches `Connection: close` missed among other options, HTTP/1.0 kept alive, and `100-continue` missed in
    // another case, so a client that waits for it stalls until the request's deadline.
    let closing = front(b"GET / HTTP/1.1\r\nConnection: keep-alive, Close\r\n\r\n");
    let old = front(b"GET / HTTP/1.0\r\n\r\n");
    let waiting = front(b"PUT / HTTP/1.1\r\nExpect: 100-Continue\r\nContent-Length: 1\r\n\r\n");

    let (Parsed::Head { head: closing, .. }, Parsed::Head { head: old, .. }, Parsed::Head { head: waiting, .. }) =
        (closing, old, waiting)
    else {
        panic!("each head parses");
    };
    assert!(closing.closes);
    assert!(old.closes);
    assert!(waiting.expects_continue);
}

#[test]
fn a_chunked_request_is_framed_by_its_chunks() {
    // Catches `chunked` missed in another case, so the request is refused, or framed as having no body, so its chunks
    // are read as the next request.
    let Parsed::Head { head, .. } = front(b"POST / HTTP/1.1\r\nTransfer-Encoding: Chunked\r\n\r\n") else {
        panic!("a chunked head parses");
    };

    assert_eq!(head.framing, Framing::Chunked);
}

#[test]
fn a_head_not_yet_whole_waits_and_one_past_the_limit_is_refused() {
    // Catches a partial head parsed as whole, and a head that never ends held without bound.
    let partial = front(b"GET / HTTP/1.1\r\nHost: here\r\n");
    let long = [b"GET / HTTP/1.1\r\nX: ".as_slice(), &[b'a'; 128]].concat();

    assert_eq!(partial, Parsed::More);
    assert_eq!(front(&long), Parsed::Refused(Refusal::HeadTooLarge));
}

#[test]
fn each_head_a_server_must_not_take_is_refused_with_its_reason() {
    // Catches each rule of the limits table let through: request smuggling by a `Transfer-Encoding` beside a
    // `Content-Length`, one in HTTP/1.0, or a doubled `Content-Length`; a coding a server does not decode, alone or
    // before `chunked`, read as if the body were not coded; a body past its limit read before it is refused; a header line that a folding or bare line feed
    // would splice, a target in absolute form, and a method or version answered as if malformed.
    let refused = [
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\nContent-Length: 1\r\n\r\n".as_slice(), Refusal::Malformed),
        (b"POST / HTTP/1.0\r\nTransfer-Encoding: chunked\r\n\r\n", Refusal::Malformed),
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n", Refusal::Unimplemented),
        (b"POST / HTTP/1.1\r\nTransfer-Encoding: gzip, chunked\r\n\r\n", Refusal::Unimplemented),
        (b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\n", Refusal::Malformed),
        (b"POST / HTTP/1.1\r\nContent-Length: +1\r\n\r\n", Refusal::Malformed),
        (b"POST / HTTP/1.1\r\nContent-Length: 17\r\n\r\n", Refusal::BodyTooLarge),
        (b"POST / HTTP/1.1\r\nContent-Length: 99999999999999999999999\r\n\r\n", Refusal::BodyTooLarge),
        (b"GET / HTTP/1.1\r\nX: a\r\n b\r\n\r\n", Refusal::Malformed),
        (b"GET / HTTP/1.1\nX: a\r\n\r\n", Refusal::Malformed),
        (b"GET / HTTP/1.1\r\nX : a\r\n\r\n", Refusal::Malformed),
        (b"GET / HTTP/1.1\r\nX: \x01\r\n\r\n", Refusal::Malformed),
        (b"GET http://here/ HTTP/1.1\r\n\r\n", Refusal::Malformed),
        (b"BREW / HTTP/1.1\r\n\r\n", Refusal::Unimplemented),
        (b"GET / HTTP/2.0\r\n\r\n", Refusal::Version),
        (b"GET  / HTTP/1.1\r\n\r\n", Refusal::Malformed),
    ];

    for (bytes, why) in refused {
        assert_eq!(front(bytes), Parsed::Refused(why), "{}", String::from_utf8_lossy(bytes));
    }
}
