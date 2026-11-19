//! What a CGI program writes before its body (RFC 3875 §6), read by a handler that runs one behind its route: header
//! lines ended by an empty line, of which `Status` is the response's status and every other is a header of the
//! response. A request's head and a response's share how a head ends and how its lines and headers parse, with every
//! limit checked before anything goes further, and a CGI program's head is read the same way.

use crate::http1::head::{self, Ends, Malformed};
use crate::stack::ResponseHead;
use crate::{Header, Status};

/// The header a CGI program names its response's status by.
const STATUS: &str = "status";

/// How much of a CGI program's head its output holds so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CgiHead {
    /// No empty line yet: more of the output is needed.
    More,
    /// No head ends within the most bytes a head may take.
    TooLarge,
    /// A line that is not a header, a line feed without its carriage return, a folded line, `Status` named more than
    /// once, or a `Status` whose code is not one a response is written with.
    Malformed,
    /// A whole head: the response's head, its status `Status` named or `200` when none did, and how many bytes of the
    /// output the head took, its empty line included. What follows is the body's.
    Whole { head: ResponseHead, consumed: usize },
}

/// How much of a CGI program's head `output`, what it has written so far, holds, within `most_bytes`.
#[must_use]
pub fn head(output: &[u8], most_bytes: u32) -> CgiHead {
    let (text, consumed) = match head::ends(output, most_bytes) {
        Ends::More => return CgiHead::More,
        Ends::TooLarge => return CgiHead::TooLarge,
        Ends::Whole { text, consumed } => (text, consumed),
    };
    let parsed = response_head(&output[text]);

    parsed.map_or(CgiHead::Malformed, |head| CgiHead::Whole { head, consumed })
}

/// The response's head a CGI program's header lines, `text`, say: its `Status` taken out of its headers.
fn response_head(text: &[u8]) -> Result<ResponseHead, Malformed> {
    let headers = head::lines(text)?.map(head::header).collect::<Result<Vec<Header>, Malformed>>()?;
    let (named, headers): (Vec<Header>, Vec<Header>) = headers.into_iter().partition(|header| header.name == STATUS);
    let status = match named.as_slice() {
        [] => Status::Ok,
        [named] => status(&named.value)?,
        _ => return Err(Malformed),
    };

    Ok(ResponseHead { status, headers })
}

/// The status a `Status` header's value names: its three digits, then a reason phrase or nothing.
fn status(value: &str) -> Result<Status, Malformed> {
    let (code, reason) = value.split_at_checked(3).ok_or(Malformed)?;
    let is_code = code.bytes().all(|byte| byte.is_ascii_digit());
    let is_parted = reason.is_empty() || reason.starts_with(' ');
    let reads = is_code && is_parted;
    if !reads {
        return Err(Malformed);
    }

    code.parse().ok().and_then(Status::of_code).ok_or(Malformed)
}

#[cfg(test)]
mod tests;
