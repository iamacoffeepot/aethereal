//! A response's head, parsed from the bytes a client has read, with every limit on it checked before anything goes
//! further (ADR-0035): its status line, its headers, and how its body is framed.

use crate::head::{self, Ends, Malformed};
use crate::{Failure, Header, Limit};

/// A response's head, parsed: what the client hands on of it, and what it reads of it to frame the body and the
/// connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub headers: Vec<Header>,
    pub framing: Framing,
    /// Whether the connection closes once the response is read: the server said so, it is HTTP/1.0, or the body runs
    /// until the connection closes.
    pub closes: bool,
}

/// How a response's body is framed (RFC 9112 §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    /// It has none: its status is informational, `204`, or `304`, or it answers a `HEAD`.
    Empty,
    /// Its `Content-Length`.
    Length(u64),
    /// `Transfer-Encoding: chunked`.
    Chunked,
    /// Neither: it runs until the connection closes.
    Close,
}

/// What the front of a client's buffer holds.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Not a whole head yet.
    More,
    /// A whole head, which with the empty lines before it took `consumed` bytes.
    Head { head: Head, consumed: usize },
    /// A head the client cannot take: it closes.
    Failed(Failure),
}

/// The HTTP versions a client reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Version {
    Http10,
    Http11,
}

/// The head at the front of `buffer`, past any empty lines before it, of at most `most_bytes`: the head of a response
/// to a `HEAD` when `head_only`, which has no body whatever it says.
pub fn head(buffer: &[u8], most_bytes: u32, head_only: bool) -> Parsed {
    match head::ends(buffer, most_bytes) {
        Ends::More => Parsed::More,
        Ends::TooLarge => Parsed::Failed(Failure::TooLarge(Limit::Head)),
        Ends::Whole { text, consumed } => match parsed(&buffer[text], head_only) {
            Ok(head) => Parsed::Head { head, consumed },
            Err(Malformed) => Parsed::Failed(Failure::Malformed),
        },
    }
}

/// The head whose lines are `text`, its ending cut off.
fn parsed(text: &[u8], head_only: bool) -> Result<Head, Malformed> {
    let mut lines = head::lines(text)?;
    let (status, version) = status_of(lines.next().ok_or(Malformed)?)?;
    let headers = lines.map(head::header).collect::<Result<Vec<Header>, Malformed>>()?;
    let framing = framing(status, &headers, head_only)?;

    let says_close = head::lists(&headers, "connection", "close");
    let is_old = version == Version::Http10;
    let runs_to_close = framing == Framing::Close;

    Ok(Head { status, headers, framing, closes: says_close || is_old || runs_to_close })
}

/// A status line's code and its version: `HTTP/1.1 200 OK`. The reason phrase is passed over, and may be empty.
fn status_of(line: &[u8]) -> Result<(u16, Version), Malformed> {
    let (version, rest) = line.split_at_checked(b"HTTP/1.1".len()).ok_or(Malformed)?;
    let version = match version {
        b"HTTP/1.1" => Version::Http11,
        b"HTTP/1.0" => Version::Http10,
        _ => return Err(Malformed),
    };
    let (code, reason) = rest.strip_prefix(b" ").ok_or(Malformed)?.split_at_checked(3).ok_or(Malformed)?;
    let is_code = code.iter().all(u8::is_ascii_digit);
    if !is_code {
        return Err(Malformed);
    }
    let reason_follows = reason.is_empty() || reason.starts_with(b" ");
    let reason_is_text = reason.iter().all(|&byte| byte == b'\t' || !byte.is_ascii_control());
    let is_reason = reason_follows && reason_is_text;
    if !is_reason {
        return Err(Malformed);
    }
    let status = code.iter().fold(0, |status, &digit| status * 10 + u16::from(digit - b'0'));
    let is_status = (100..=599).contains(&status);
    if !is_status {
        return Err(Malformed);
    }

    Ok((status, version))
}

/// How the body of a response of `status` with `headers` is framed (RFC 9112 §6.3). A transfer coding other than
/// chunked alone, which the client does not decode, or one beside a `Content-Length`, which is how a response is
/// smuggled, does not parse.
fn framing(status: u16, headers: &[Header], head_only: bool) -> Result<Framing, Malformed> {
    let is_informational = (100..200).contains(&status);
    let has_none = head_only || is_informational || status == 204 || status == 304;
    if has_none {
        return Ok(Framing::Empty);
    }
    let codings: Vec<&Header> = headers.iter().filter(|header| header.name == "transfer-encoding").collect();
    let length = head::content_length(headers)?;

    match (codings.as_slice(), length) {
        ([], Some(length)) => Ok(Framing::Length(length)),
        ([], None) => Ok(Framing::Close),
        ([coding], None) if coding.value.eq_ignore_ascii_case("chunked") => Ok(Framing::Chunked),
        _ => Err(Malformed),
    }
}

#[cfg(test)]
mod tests;
