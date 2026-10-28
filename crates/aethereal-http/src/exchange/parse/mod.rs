//! A request's head, parsed from the bytes an exchange has read, with every limit on it checked (ADR-0035).

use crate::{Header, Limits, Method, Response, Status};
use aethereal_data::Plain;

/// The line ending HTTP writes.
const CRLF: &[u8] = b"\r\n";

/// What ends a request's head.
const END: &[u8] = b"\r\n\r\n";

/// A request's head, parsed: what the handler is given of it, and what the exchange reads of it to frame the body
/// and the connection.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub struct Head {
    pub method: Method,
    pub path: String,
    pub query: String,
    pub headers: Vec<Header>,
    /// The body's bytes, by its `Content-Length`: 0 when it has none.
    pub length: u32,
    /// Whether the connection closes once this request is answered: it asked to, or it is HTTP/1.0.
    pub closes: bool,
    /// Whether the client waits for `100 Continue` before it sends the body.
    pub expects_continue: bool,
}

/// What the front of an exchange's buffer holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Not a whole head yet.
    More,
    /// A whole head, which with the line endings before it took `consumed` bytes.
    Head { head: Head, consumed: usize },
    /// A head the exchange refuses, and answers before it closes.
    Refused(Refusal),
}

/// Why a request is refused before it reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// It does not parse: `400`.
    Malformed,
    /// Its head is past `most_head_bytes`: `431`.
    HeadTooLarge,
    /// Its body is past `most_body_bytes`: `413`.
    BodyTooLarge,
    /// A method no server takes, or a `Transfer-Encoding`: `501`.
    Unimplemented,
    /// An HTTP version other than 1.0 and 1.1: `505`.
    Version,
}

impl Refusal {
    /// The response it is answered with.
    pub fn response(self) -> Response {
        let (status, text) = match self {
            Self::Malformed => (Status::BadRequest, "the request does not parse"),
            Self::HeadTooLarge => (Status::RequestHeaderFieldsTooLarge, "the request's head is too large"),
            Self::BodyTooLarge => (Status::ContentTooLarge, "the request's body is too large"),
            Self::Unimplemented => (Status::NotImplemented, "the request's method or transfer coding is not served"),
            Self::Version => (Status::HttpVersionNotSupported, "the request's HTTP version is not served"),
        };

        Response::new(status, "text/plain; charset=utf-8", text.as_bytes().to_vec())
    }
}

/// The head at the front of `buffer`, past any empty lines before it, as `limits` allow it.
pub fn head(buffer: &[u8], limits: &Limits) -> Parsed {
    let start = leading_lines(buffer);
    let most = usize::try_from(limits.most_head_bytes).unwrap_or(usize::MAX);
    let rest = &buffer[start..];
    let searched = &rest[..rest.len().min(most)];
    let Some(end) = find(searched, END) else {
        let is_past = rest.len() >= most;
        let parsed = if is_past {
            Parsed::Refused(Refusal::HeadTooLarge)
        } else {
            Parsed::More
        };
        return parsed;
    };

    match parsed(&rest[..end], limits) {
        Ok(head) => Parsed::Head { head, consumed: start + end + END.len() },
        Err(refusal) => Parsed::Refused(refusal),
    }
}

/// How many bytes of empty lines `buffer` starts with, which a server passes over before a request (RFC 9112 §2.2).
fn leading_lines(buffer: &[u8]) -> usize {
    let mut start = 0;
    while buffer[start..].starts_with(CRLF) {
        start += CRLF.len();
    }

    start
}

/// The head whose lines are `text`, its ending cut off.
fn parsed(text: &[u8], limits: &Limits) -> Result<Head, Refusal> {
    let bare_line_feed = text.iter().enumerate().any(|(at, &byte)| byte == b'\n' && (at == 0 || text[at - 1] != b'\r'));
    if bare_line_feed {
        return Err(Refusal::Malformed);
    }
    // Every line feed ends a line written with CRLF, as checked above, so each line is cut at its line feed and loses
    // the carriage return before it.
    let mut lines = text.split(|&byte| byte == b'\n').map(|line| line.strip_suffix(b"\r").unwrap_or(line));
    let request_line = lines.next().ok_or(Refusal::Malformed)?;
    let (method, target, version) = request_line_of(request_line)?;
    let headers = lines.map(header).collect::<Result<Vec<Header>, Refusal>>()?;
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let (path, query) = (String::from(path), String::from(query));

    let is_coded = headers.iter().any(|header| header.name == "transfer-encoding");
    if is_coded {
        return Err(Refusal::Unimplemented);
    }
    let length = length(&headers, limits)?;
    let asks_close =
        headers.iter().filter(|header| header.name == "connection").any(|header| names_close(&header.value));
    let is_old = version == Version::Http10;
    let expects_continue =
        headers.iter().any(|header| header.name == "expect" && header.value.eq_ignore_ascii_case("100-continue"));

    Ok(Head { method, path, query, headers, length, closes: asks_close || is_old, expects_continue })
}

/// The HTTP versions a server reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    Http10,
    Http11,
}

/// A request line's method, its target in origin form, and its version.
fn request_line_of(line: &[u8]) -> Result<(Method, String, Version), Refusal> {
    let mut parts = line.split(|&byte| byte == b' ');
    let (Some(method), Some(target), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refusal::Malformed);
    };
    let version = match version {
        b"HTTP/1.1" => Version::Http11,
        b"HTTP/1.0" => Version::Http10,
        other if other.starts_with(b"HTTP/") => return Err(Refusal::Version),
        _ => return Err(Refusal::Malformed),
    };
    let is_token = is_token(method);
    if !is_token {
        return Err(Refusal::Malformed);
    }
    let method = Method::named(method).ok_or(Refusal::Unimplemented)?;
    let is_origin_form = target.first() == Some(&b'/') && target.iter().all(u8::is_ascii_graphic);
    if !is_origin_form {
        return Err(Refusal::Malformed);
    }
    let target = String::from_utf8(target.to_vec()).map_err(|_| Refusal::Malformed)?;

    Ok((method, target, version))
}

/// One header line, its name lowercased and the whitespace around its value cut.
fn header(line: &[u8]) -> Result<Header, Refusal> {
    let folded = line.first().is_some_and(|&byte| byte == b' ' || byte == b'\t');
    if folded {
        return Err(Refusal::Malformed);
    }
    let colon = line.iter().position(|&byte| byte == b':').ok_or(Refusal::Malformed)?;
    let (name, value) = (&line[..colon], &line[colon + 1..]);
    let is_named = !name.is_empty() && is_token(name);
    if !is_named {
        return Err(Refusal::Malformed);
    }
    let value = value.trim_ascii();
    let is_visible = value.iter().all(|&byte| byte == b'\t' || (b' '..=b'~').contains(&byte));
    if !is_visible {
        return Err(Refusal::Malformed);
    }
    let name = String::from_utf8(name.to_ascii_lowercase()).map_err(|_| Refusal::Malformed)?;
    let value = String::from_utf8(value.to_vec()).map_err(|_| Refusal::Malformed)?;

    Ok(Header { name, value })
}

/// The body's length by its one `Content-Length`, as `limits` allow it: 0 when there is none.
fn length(headers: &[Header], limits: &Limits) -> Result<u32, Refusal> {
    let lengths: Vec<&Header> = headers.iter().filter(|header| header.name == "content-length").collect();
    let length = match lengths.as_slice() {
        [] => return Ok(0),
        [length] => length,
        _ => return Err(Refusal::Malformed),
    };
    let is_number = !length.value.is_empty() && length.value.bytes().all(|byte| byte.is_ascii_digit());
    if !is_number {
        return Err(Refusal::Malformed);
    }
    let length = length.value.parse::<u64>().map_err(|_| Refusal::BodyTooLarge)?;
    let is_past = length > u64::from(limits.most_body_bytes);
    if is_past {
        return Err(Refusal::BodyTooLarge);
    }

    u32::try_from(length).map_err(|_| Refusal::BodyTooLarge)
}

/// Whether a `Connection` value lists `close`.
fn names_close(value: &str) -> bool {
    value.split(',').any(|option| option.trim().eq_ignore_ascii_case("close"))
}

/// Whether `bytes` are all characters a token takes (RFC 9110 §5.6.2).
pub fn is_token(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// Where `needle` first stands in `haystack`.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

#[cfg(test)]
mod tests;
