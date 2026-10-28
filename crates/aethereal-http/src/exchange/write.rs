//! A response, written as the bytes an exchange sends its connection (ADR-0035).

use super::parse::is_token;
use crate::Response;

/// What an exchange writes before it reads a body whose client waits to be told to send it.
pub const CONTINUE: &[u8] = b"HTTP/1.1 100 Continue\r\n\r\n";

/// The headers the exchange writes itself, and drops from a handler's response.
const FRAMING: [&str; 3] = ["content-length", "transfer-encoding", "connection"];

/// A response as written, and the headers of it that were not.
pub struct Written {
    pub bytes: Vec<u8>,
    /// Each header dropped, by name: one the exchange writes itself, or one whose name or value would not write.
    pub dropped: Vec<String>,
}

/// `response` as bytes: its status line, its headers, `Content-Length` and, when the connection `closes`, `Connection:
/// close`, then its body, unless it answers a `HEAD`, which is `head_only`.
pub fn written(response: Response, closes: bool, head_only: bool) -> Written {
    let Response { status, headers, body } = response;
    let has_body = status.has_body();
    let (code, reason) = status.line();

    let mut bytes = format!("HTTP/1.1 {code} {reason}\r\n").into_bytes();
    let mut dropped = Vec::new();
    for header in headers {
        let writes = writes(&header.name, &header.value);
        if !writes {
            dropped.push(header.name);
            continue;
        }
        bytes.extend_from_slice(format!("{}: {}\r\n", header.name, header.value).as_bytes());
    }
    if has_body {
        bytes.extend_from_slice(format!("content-length: {}\r\n", body.len()).as_bytes());
    }
    if closes {
        bytes.extend_from_slice(b"connection: close\r\n");
    }
    bytes.extend_from_slice(b"\r\n");
    let sends_body = has_body && !head_only;
    if sends_body {
        bytes.extend_from_slice(&body);
    }

    Written { bytes, dropped }
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
