//! A request's head, written as the bytes a client sends its connection before the body (ADR-0035, ADR-0042).

use crate::head::is_token;
use crate::{Header, Method, Outgoing, Switch, Unwritable};

/// What of a request its head is written from: all but its body.
#[derive(Debug, Clone, Copy)]
pub struct Lines<'r> {
    pub method: Method,
    pub path: &'r str,
    pub query: &'r str,
    pub headers: &'r [Header],
}

impl<'r> From<&'r Outgoing> for Lines<'r> {
    fn from(request: &'r Outgoing) -> Self {
        Self { method: request.method, path: &request.path, query: &request.query, headers: &request.headers }
    }
}

/// The headers a client writes itself, and refuses from its asker: they frame the request and the connection, so one
/// the asker wrote could make the server read the request otherwise than the client writes it.
const FRAMING: [&str; 5] = ["host", "content-length", "transfer-encoding", "connection", "upgrade"];

/// Room enough for a head's request line, its `Host`, `Content-Length`, `Connection`, and `Upgrade` beside what they
/// name, and the blank line that ends it.
const FRAME_BYTES: usize = 128;

/// The line ending HTTP writes, and the bytes of a header's `: `.
const SEPARATORS_BYTES: usize = 4;

/// The room the head of `request` to `host` takes, switching as `switch` says, to write it into a buffer that holds
/// it.
pub fn room(request: Lines<'_>, host: &str, switch: &Switch) -> usize {
    let lines: usize =
        request.headers.iter().map(|header| header.name.len() + header.value.len() + SEPARATORS_BYTES).sum();
    let protocol = match switch {
        Switch::Stay => 0,
        Switch::To { protocol } => protocol.len(),
    };

    FRAME_BYTES + request.path.len() + request.query.len() + host.len() + protocol + lines
}

/// Writes the head of `request`, whose body is `body_length` bytes, to the server at `host` into `head`, switching
/// as `switch` says: its request line, `Host`, the asker's headers, `Content-Length` when the request has a body or
/// its method expects one, and `Connection: Upgrade` and `Upgrade` when it asks to switch; then the blank line. The
/// body is not written here: the client writes it after the head as a part of its own.
///
/// # Errors
///
/// Why the request would not write: its target, one of its headers, or the protocol it switches to. Nothing is then
/// written that matters, as the head is not sent.
pub fn head(
    request: Lines<'_>,
    body_length: usize,
    host: &str,
    switch: &Switch,
    head: &mut Vec<u8>,
) -> Result<(), Unwritable> {
    let Lines { method, path, query, headers } = request;
    let is_target = is_path(path) && is_query(query);
    if !is_target {
        return Err(Unwritable::Target);
    }
    let unwritten = headers.iter().find(|header| !writes(header));
    if let Some(header) = unwritten {
        return Err(Unwritable::Header { name: header.name.clone() });
    }
    let is_protocol = match switch {
        Switch::Stay => true,
        Switch::To { protocol } => !protocol.is_empty() && is_token(protocol.as_bytes()),
    };
    if !is_protocol {
        return Err(Unwritable::Protocol);
    }

    let token = method.token();
    let mark = if query.is_empty() {
        ""
    } else {
        "?"
    };
    head.extend_from_slice(format!("{token} {path}{mark}{query} HTTP/1.1\r\nhost: {host}\r\n").as_bytes());
    for Header { name, value } in headers {
        head.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    let has_body = body_length > 0 || expects_body(method);
    if has_body {
        head.extend_from_slice(format!("content-length: {body_length}\r\n").as_bytes());
    }
    if let Switch::To { protocol } = switch {
        head.extend_from_slice(format!("connection: upgrade\r\nupgrade: {protocol}\r\n").as_bytes());
    }
    head.extend_from_slice(b"\r\n");

    Ok(())
}

/// Whether `path` is a target's path in origin form: `/`, then visible bytes up to the query and the fragment.
fn is_path(path: &str) -> bool {
    let is_rooted = path.starts_with('/');
    let is_visible = path.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'?' && byte != b'#');

    is_rooted && is_visible
}

/// Whether `query` is a target's query: visible bytes up to the fragment.
fn is_query(query: &str) -> bool {
    query.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'#')
}

/// Whether an asker's header is written: its name is a token the client does not write itself, and its value holds
/// only visible bytes, spaces, and tabs, so no value writes a header of its own.
fn writes(header: &Header) -> bool {
    let is_token = !header.name.is_empty() && is_token(header.name.as_bytes());
    let is_framing = FRAMING.iter().any(|framing| header.name.eq_ignore_ascii_case(framing));
    let is_clean = header.value.bytes().all(|byte| byte == b'\t' || (b' '..=b'~').contains(&byte));

    is_token && !is_framing && is_clean
}

/// Whether a request of `method` is written with a `Content-Length` when it has no body: a method that sends one
/// (RFC 9110 §8.6), so a server does not wait for a body, or refuse it as of no length.
const fn expects_body(method: Method) -> bool {
    matches!(method, Method::Post | Method::Put | Method::Patch)
}

#[cfg(test)]
mod tests {
    use super::{Lines, head, room};
    use crate::{Header, Method, Switch, Unwritable};

    /// A request of `method` to `path` and `query`, with `headers`.
    const fn request<'r>(method: Method, path: &'r str, query: &'r str, headers: &'r [Header]) -> Lines<'r> {
        Lines { method, path, query, headers }
    }

    /// The head `request`, of a body of `body_length` bytes, writes to `here`, switching as `switch` says.
    fn written(request: Lines<'_>, body_length: usize, switch: &Switch) -> Result<Vec<u8>, Unwritable> {
        let mut written = Vec::with_capacity(room(request, "here", switch));
        head(request, body_length, "here", switch, &mut written)?;
        assert!(written.capacity() == room(request, "here", switch), "the head fit the room it was given");

        Ok(written)
    }

    #[test]
    fn a_head_names_its_host_and_body_and_asks_to_switch_when_told() {
        // Catches a query written without its `?` or a `?` written with none, a request without `Host`, which an
        // HTTP/1.1 server refuses, a `POST` with no body written without `Content-Length`, which a server may refuse,
        // a `Content-Length` that is not the body's, and a switch asked without both the headers that ask it.
        let headers = [Header::new("Accept", "application/json")];
        let get = request(Method::Get, "/containers/json", "all=1", &headers);
        let post = request(Method::Post, "/containers/7/attach", "", &[]);
        let switch = Switch::To { protocol: String::from("tcp") };

        let expected = b"GET /containers/json?all=1 HTTP/1.1\r\nhost: here\r\nAccept: application/json\r\n\r\n";
        assert_eq!(written(get, 0, &Switch::Stay), Ok(expected.to_vec()));
        let expected = b"GET /containers/json?all=1 HTTP/1.1\r\nhost: here\r\nAccept: application/json\r\ncontent-length: 3\r\n\r\n";
        assert_eq!(written(get, 3, &Switch::Stay), Ok(expected.to_vec()));
        let expected = b"POST /containers/7/attach HTTP/1.1\r\nhost: here\r\ncontent-length: 0\r\nconnection: upgrade\r\nupgrade: tcp\r\n\r\n";
        assert_eq!(written(post, 0, &switch), Ok(expected.to_vec()));
    }

    #[test]
    fn a_request_that_would_write_what_its_asker_did_not_mean_is_refused() {
        // Catches a header the client frames written twice, one in a case it does not check, so the server reads the
        // body or the connection otherwise than the client does; a value that writes a header of its own; a target
        // whose path holds its query or a space that would end the request line; and a protocol that is not a token.
        let framing = [Header::new("Content-Length", "5")];
        let spliced = [Header::new("x", "a\r\nhost: elsewhere")];
        let refused = [
            (request(Method::Get, "/", "", &framing), Switch::Stay),
            (request(Method::Get, "/", "", &spliced), Switch::Stay),
            (request(Method::Get, "/a?b", "", &[]), Switch::Stay),
            (request(Method::Get, "/a b", "", &[]), Switch::Stay),
            (request(Method::Get, "a", "", &[]), Switch::Stay),
            (request(Method::Get, "/", "", &[]), Switch::To { protocol: String::from("tcp\r\nx: y") }),
        ];
        let why = [
            Unwritable::Header { name: String::from("Content-Length") },
            Unwritable::Header { name: String::from("x") },
            Unwritable::Target,
            Unwritable::Target,
            Unwritable::Target,
            Unwritable::Protocol,
        ];

        for ((request, switch), why) in refused.iter().zip(why) {
            assert_eq!(head(*request, 0, "here", switch, &mut Vec::new()), Err(why), "{request:?}");
        }
    }
}
