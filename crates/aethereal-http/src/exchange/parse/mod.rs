//! A request's head, parsed from the bytes an exchange has read, with every limit on it checked (ADR-0035).

use crate::body::{Body, Broken, Chunk};
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
    /// How its body is framed, by its `Content-Length` or its `Transfer-Encoding`.
    pub framing: Framing,
    /// Whether the connection closes once this request is answered: it asked to, or it is HTTP/1.0.
    pub closes: bool,
    /// Whether the client waits for `100 Continue` before it sends the body.
    pub expects_continue: bool,
}

/// How a request's body is framed (RFC 9112 §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Framing {
    /// It has none: neither a `Transfer-Encoding` nor a `Content-Length`, or a `Content-Length` of 0.
    Empty,
    /// Its `Content-Length`, more than 0 and within `most_body_bytes`.
    Length(u64),
    /// `Transfer-Encoding: chunked`, its decoded length bounded by `most_body_bytes` as it is read.
    Chunked,
}

impl Framing {
    /// The body it frames: one with none is read as a length of nothing, which ends at once.
    pub const fn body(self) -> Body {
        match self {
            Self::Empty => Body::Length { left: 0 },
            Self::Length(left) => Body::Length { left },
            Self::Chunked => Body::Chunked { at: Chunk::Size },
        }
    }
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
    /// A method no server takes, or a transfer coding other than `chunked` alone: `501`.
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

    let framing = framing(&headers, version, limits)?;
    let asks_close = head::lists(&headers, "connection", "close");
    let is_old = version == Version::Http10;
    let expects_continue =
        headers.iter().any(|header| header.name == "expect" && header.value.eq_ignore_ascii_case("100-continue"));

    Ok(Head { method, path, query, headers, framing, closes: asks_close || is_old, expects_continue })
}

/// A head, or a line of one, that does not parse is answered `400`.
impl From<Malformed> for Refusal {
    fn from(_: Malformed) -> Self {
        Self::Malformed
    }
}

/// A body that breaks its chunked framing does not parse, and one whose chunk's line runs past the most bytes of a head
/// is too large.
impl From<Broken> for Refusal {
    fn from(broken: Broken) -> Self {
        match broken {
            Broken::Malformed => Self::Malformed,
            Broken::LineTooLarge => Self::BodyTooLarge,
        }
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

/// How the body of a request of `version` with `headers` is framed (RFC 9112 §6.1, §6.3), as `limits` allow it. A body
/// past `most_body_bytes` by its `Content-Length` is refused before it is read. A `Transfer-Encoding` beside a
/// `Content-Length`, or in HTTP/1.0, which has none, frames the body two ways, which is how a request is smuggled, and
/// does not parse. A transfer coding other than `chunked` alone is one the exchange does not decode.
fn framing(headers: &[Header], version: Version, limits: &Limits) -> Result<Framing, Refusal> {
    let codings: Vec<&Header> = headers.iter().filter(|header| header.name == "transfer-encoding").collect();
    let length = head::content_length(headers)?;

    match (codings.as_slice(), length) {
        ([], None | Some(0)) => Ok(Framing::Empty),
        ([], Some(length)) if length > u64::from(limits.most_body_bytes) => Err(Refusal::BodyTooLarge),
        ([], Some(length)) => Ok(Framing::Length(length)),
        (_, Some(_)) => Err(Refusal::Malformed),
        (_, None) if version == Version::Http10 => Err(Refusal::Malformed),
        ([coding], None) if coding.value.eq_ignore_ascii_case("chunked") => Ok(Framing::Chunked),
        (_, None) => Err(Refusal::Unimplemented),
    }
}

#[cfg(test)]
mod tests;
