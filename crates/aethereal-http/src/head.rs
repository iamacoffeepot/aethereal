//! What a request's head and a response's head share, parsed with every limit checked before anything goes further
//! (ADR-0035): where a head ends, its lines, its headers, and its body's length. An exchange reads a request line
//! before them, and a client a status line.

use crate::Header;
use core::ops::Range;

/// The line ending HTTP writes.
pub const CRLF: &[u8] = b"\r\n";

/// What ends a head.
const END: &[u8] = b"\r\n\r\n";

/// Why a head, or a line of one, does not parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Malformed;

/// Where the head at the front of a buffer ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ends {
    /// Not a whole head yet.
    More,
    /// No head ends within the most bytes a head may take.
    TooLarge,
    /// A whole head: its lines are `text`, its ending cut off, and it took `consumed` bytes with the empty lines
    /// before it.
    Whole { text: Range<usize>, consumed: usize },
}

/// Where the head at the front of `buffer` ends, past any empty lines before it, within `most` bytes.
pub fn ends(buffer: &[u8], most: u32) -> Ends {
    let start = leading_lines(buffer);
    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let rest = &buffer[start..];
    let searched = &rest[..rest.len().min(most)];
    let Some(end) = find(searched, END) else {
        let is_past = rest.len() >= most;
        let ends = if is_past {
            Ends::TooLarge
        } else {
            Ends::More
        };
        return ends;
    };

    Ends::Whole { text: start..start + end, consumed: start + end + END.len() }
}

/// How many bytes of empty lines `buffer` starts with, which a reader passes over before a head (RFC 9112 §2.2).
fn leading_lines(buffer: &[u8]) -> usize {
    let mut start = 0;
    while buffer[start..].starts_with(CRLF) {
        start += CRLF.len();
    }

    start
}

/// The lines of a head whose text is `text`, its ending cut off, each without its line ending: none when a line feed
/// stands without the carriage return before it.
pub fn lines(text: &[u8]) -> Result<impl Iterator<Item = &[u8]>, Malformed> {
    let bare_line_feed = text.iter().enumerate().any(|(at, &byte)| byte == b'\n' && (at == 0 || text[at - 1] != b'\r'));
    if bare_line_feed {
        return Err(Malformed);
    }

    // Every line feed ends a line written with CRLF, as checked above, so each line is cut at its line feed and loses
    // the carriage return before it.
    Ok(text.split(|&byte| byte == b'\n').map(|line| line.strip_suffix(b"\r").unwrap_or(line)))
}

/// One header line, its name lowercased and the whitespace around its value cut.
pub fn header(line: &[u8]) -> Result<Header, Malformed> {
    let folded = line.first().is_some_and(|&byte| byte == b' ' || byte == b'\t');
    if folded {
        return Err(Malformed);
    }
    let colon = line.iter().position(|&byte| byte == b':').ok_or(Malformed)?;
    let (name, value) = (&line[..colon], &line[colon + 1..]);
    let is_named = !name.is_empty() && is_token(name);
    if !is_named {
        return Err(Malformed);
    }
    let value = value.trim_ascii();
    let is_visible = value.iter().all(|&byte| byte == b'\t' || (b' '..=b'~').contains(&byte));
    if !is_visible {
        return Err(Malformed);
    }
    let name = String::from_utf8(name.to_ascii_lowercase()).map_err(|_| Malformed)?;
    let value = String::from_utf8(value.to_vec()).map_err(|_| Malformed)?;

    Ok(Header { name, value })
}

/// The body's length by its one `Content-Length`, the most a `u64` holds when it is longer: none when the head has no
/// `Content-Length`.
pub fn content_length(headers: &[Header]) -> Result<Option<u64>, Malformed> {
    let lengths: Vec<&Header> = headers.iter().filter(|header| header.name == "content-length").collect();
    let length = match lengths.as_slice() {
        [] => return Ok(None),
        [length] => length,
        _ => return Err(Malformed),
    };
    let is_number = !length.value.is_empty() && length.value.bytes().all(|byte| byte.is_ascii_digit());
    if !is_number {
        return Err(Malformed);
    }

    // Only digits are left, so the parse fails only on a number past what a `u64` holds.
    Ok(Some(length.value.parse::<u64>().unwrap_or(u64::MAX)))
}

/// Whether any header named `name` lists `option` among its comma-separated values, in any case.
pub fn lists(headers: &[Header], name: &str, option: &str) -> bool {
    let named = headers.iter().filter(|header| header.name == name);
    let mut options = named.flat_map(|header| header.value.split(','));

    options.any(|listed| listed.trim().eq_ignore_ascii_case(option))
}

/// Whether `bytes` are all characters a token takes (RFC 9110 §5.6.2).
pub fn is_token(bytes: &[u8]) -> bool {
    bytes.iter().all(|&byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

/// Where `needle` first stands in `haystack`.
pub fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}
