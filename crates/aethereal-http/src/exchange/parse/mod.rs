//! A request's head, parsed from the bytes an exchange has read, with every limit on it checked (ADR-0035).

use crate::head::{self, Ends, Malformed};
use crate::{Header, Limits, Method, Status};
use aethereal_data::Plain;

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
    /// The status it is answered with, and the text of the answer's body.
    pub const fn said(self) -> (Status, &'static str) {
        match self {
            Self::Malformed => (Status::BadRequest, "the request does not parse"),
            Self::HeadTooLarge => (Status::RequestHeaderFieldsTooLarge, "the request's head is too large"),
            Self::BodyTooLarge => (Status::ContentTooLarge, "the request's body is too large"),
            Self::Unimplemented => (Status::NotImplemented, "the request's method or transfer coding is not served"),
            Self::Version => (Status::HttpVersionNotSupported, "the request's HTTP version is not served"),
        }
    }
}

/// The head at the front of `buffer`, past any empty lines before it, as `limits` allow it.
pub fn head(buffer: &[u8], limits: &Limits) -> Parsed {
    match head::ends(buffer, limits.most_head_bytes) {
        Ends::More => Parsed::More,
        Ends::TooLarge => Parsed::Refused(Refusal::HeadTooLarge),
        Ends::Whole { text, consumed } => match parsed(&buffer[text], limits) {
            Ok(head) => Parsed::Head { head, consumed },
            Err(refusal) => Parsed::Refused(refusal),
        },
    }
}

/// The head whose lines are `text`, its ending cut off.
fn parsed(text: &[u8], limits: &Limits) -> Result<Head, Refusal> {
    let mut lines = head::lines(text)?;
    let request_line = lines.next().ok_or(Refusal::Malformed)?;
    let (method, target, version) = request_line_of(request_line)?;
    let headers = lines.map(head::header).collect::<Result<Vec<Header>, Malformed>>()?;
    let (path, query) = target.split_once('?').unwrap_or((target.as_str(), ""));
    let (path, query) = (String::from(path), String::from(query));

    let is_coded = headers.iter().any(|header| header.name == "transfer-encoding");
    if is_coded {
        return Err(Refusal::Unimplemented);
    }
    let length = length(&headers, limits)?;
    let asks_close = head::lists(&headers, "connection", "close");
    let is_old = version == Version::Http10;
    let expects_continue =
        headers.iter().any(|header| header.name == "expect" && header.value.eq_ignore_ascii_case("100-continue"));

    Ok(Head { method, path, query, headers, length, closes: asks_close || is_old, expects_continue })
}

/// A head, or a line of one, that does not parse is answered `400`.
impl From<Malformed> for Refusal {
    fn from(_: Malformed) -> Self {
        Self::Malformed
    }
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
    let is_token = head::is_token(method);
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

/// The body's length by its one `Content-Length`, as `limits` allow it: 0 when there is none.
fn length(headers: &[Header], limits: &Limits) -> Result<u32, Refusal> {
    let Some(length) = head::content_length(headers)? else {
        return Ok(0);
    };
    let is_past = length > u64::from(limits.most_body_bytes);
    if is_past {
        return Err(Refusal::BodyTooLarge);
    }

    u32::try_from(length).map_err(|_| Refusal::BodyTooLarge)
}

#[cfg(test)]
mod tests;
