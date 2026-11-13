//! A response's head, written as the bytes an exchange sends its connection before the body (ADR-0035, ADR-0042).

use crate::head::is_token;
use crate::{Header, Status};

/// What an exchange writes before it reads a body whose client waits to be told to send it.
pub const CONTINUE: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";

/// The headers the exchange writes itself, and drops from a handler's response.
const FRAMING: [&str; 3] = ["content-length", "transfer-encoding", "connection"];

/// Room enough for a head's status line, its `Content-Length` and `Connection`, and the blank line that ends it.
const FRAME_BYTES: usize = 96;

/// The line ending HTTP writes, and the bytes of a header's `: `.
const SEPARATORS_BYTES: usize = 4;

/// What became of a response's head as it was written.
pub struct Written {
    /// Each header dropped, by name: one the exchange writes itself, or one whose name or value would not write.
    pub dropped: Vec<String>,
    /// Whether the body is written after the head: the status has one, and it does not answer a `HEAD`.
    pub sends_body: bool,
}

/// The room a head with `headers` takes, to write it into a buffer that holds it.
pub fn room(headers: &[Header]) -> usize {
    let lines: usize = headers.iter().map(|header| header.name.len() + header.value.len() + SEPARATORS_BYTES).sum();

    FRAME_BYTES + lines
}

/// Writes the head of a response of `status` with `headers` and a body of `body_length` bytes into `head`: its
/// status line, its headers, `Content-Length` and, when the connection `closes`, `Connection: close`, then the blank
/// line. The body is not written here: the exchange writes it after the head as a part of its own, unless the
/// response answers a `HEAD`, which is `head_only`.
pub fn head(
    status: Status,
    headers: &[Header],
    body_length: usize,
    closes: bool,
    head_only: bool,
    head: &mut Vec<u8>,
) -> Written {
    let has_body = status.has_body();
    let (code, reason) = status.line();

    head.extend_from_slice(format!("HTTP/1.1 {code} {reason}\r\n").as_bytes());
    let mut dropped = Vec::new();
    for header in headers {
        let writes = writes(&header.name, &header.value);
        if !writes {
            dropped.push(header.name.clone());
            continue;
        }
        head.extend_from_slice(format!("{}: {}\r\n", header.name, header.value).as_bytes());
    }
    if has_body {
        head.extend_from_slice(format!("content-length: {body_length}\r\n").as_bytes());
    }
    if closes {
        head.extend_from_slice(b"connection: close\r\n");
    }
    head.extend_from_slice(b"\r\n");

    Written { dropped, sends_body: has_body && !head_only }
}

/// Whether a handler's header `name: value` is written: its name is a token the exchange does not write itself, and
/// its value holds no line ending or other control byte, which would let a handler's value write a header of its
/// own.
fn writes(name: &str, value: &str) -> bool {
    let is_token = !name.is_empty() && is_token(name.as_bytes());
    let is_framing = FRAMING.iter().any(|framing| name.eq_ignore_ascii_case(framing));
    let is_clean = value.bytes().all(|byte| byte == b'\t' || !byte.is_ascii_control());

    is_token && !is_framing && is_clean
}

#[cfg(test)]
mod tests {
    use super::{head, room};
    use crate::{Header, Status};

    #[test]
    fn a_head_names_its_body_without_holding_it_and_a_head_only_answer_sends_none() {
        // Catches a `Content-Length` that is not the body's, a head written without its blank line, a body sent in
        // answer to a `HEAD` or with a status that has none, and a head that outgrows the room it is given.
        let headers = [Header::new("content-type", "text/plain")];
        let mut written = Vec::with_capacity(room(&headers));

        let ok = head(Status::Ok, &headers, 5, true, false, &mut written);

        let expected = b"HTTP/1.1 200 OK\r\ncontent-type: text/plain\r\ncontent-length: 5\r\nconnection: close\r\n\r\n";
        assert_eq!(written, expected);
        assert!(written.capacity() == room(&headers), "the head fit the room it was given");
        assert!(ok.sends_body);
        assert!(!head(Status::Ok, &headers, 5, false, true, &mut Vec::new()).sends_body);
        assert!(!head(Status::NoContent, &[], 0, false, false, &mut Vec::new()).sends_body);
    }
}
