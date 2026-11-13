//! A response's body, decoded from the bytes a client has read as its framing says (RFC 9112 §6, §7.1): by its
//! length, in chunks, or until the connection closes. It decodes whatever has arrived, and keeps where it stopped, so a
//! chunk's size line or its data may be cut anywhere between reads.

use super::parse::Framing;
use crate::head::{self, CRLF};
use crate::{Failure, Limit};
use aethereal_data::Plain;

/// What is left to read of a response's body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Body {
    /// `left` bytes, by its `Content-Length`.
    Length { left: u64 },
    /// Chunks, read up to where `at` says.
    Chunked { at: Chunk },
    /// Every byte until the connection closes.
    Close,
}

/// Where a chunked body has been read to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
pub enum Chunk {
    /// Before a chunk's size line.
    Size,
    /// In a chunk's data, `left` bytes of it still to come.
    Data { left: u64 },
    /// After a chunk's data, before the line ending that follows it.
    End,
    /// After the last chunk, among the trailer's fields, until the empty line that ends them.
    Trailer,
}

/// What a body's read came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// More of the body is to come.
    More,
    /// The body ended where its framing says.
    Done,
}

/// What a body's read took of the bytes it was given, and whether the body ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decoded {
    pub consumed: usize,
    pub progress: Progress,
}

/// Where a line at the front of some bytes ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    /// Its end has not arrived.
    Partial,
    /// It is `length` bytes, its line ending after them.
    Whole { length: usize },
}

impl Body {
    /// The body a head's `framing` frames: one with none is read as a length of nothing, which ends at once.
    pub const fn framed(framing: Framing) -> Self {
        match framing {
            Framing::Empty => Self::Length { left: 0 },
            Framing::Length(left) => Self::Length { left },
            Framing::Chunked => Self::Chunked { at: Chunk::Size },
            Framing::Close => Self::Close,
        }
    }

    /// Reads what it can of the body from the front of `buffer`, appending the body's bytes to `out`, and says how
    /// many bytes of `buffer` it took and whether the body has ended. A chunk's size line or a trailer's field past
    /// `most_line` bytes is too large.
    ///
    /// # Errors
    ///
    /// Why the body cannot be read on: a chunk's line that does not parse, data not followed by its line ending, or a
    /// line past `most_line`.
    pub fn decode(&mut self, buffer: &[u8], out: &mut Vec<u8>, most_line: u32) -> Result<Decoded, Failure> {
        let mut at = 0;
        loop {
            let rest = &buffer[at..];
            match *self {
                Self::Length { left } => {
                    let taking = taken(rest, left);
                    out.extend_from_slice(&rest[..taking]);
                    let left = left - taking as u64;
                    *self = Self::Length { left };
                    let progress = if left == 0 {
                        Progress::Done
                    } else {
                        Progress::More
                    };
                    return Ok(Decoded { consumed: at + taking, progress });
                }
                Self::Close => {
                    out.extend_from_slice(rest);
                    return Ok(Decoded { consumed: buffer.len(), progress: Progress::More });
                }
                Self::Chunked { at: Chunk::Size } => {
                    let Line::Whole { length } = line(rest, most_line)? else {
                        return Ok(Decoded { consumed: at, progress: Progress::More });
                    };
                    let size = size_of(&rest[..length])?;
                    at += length + CRLF.len();
                    let next = if size == 0 {
                        Chunk::Trailer
                    } else {
                        Chunk::Data { left: size }
                    };
                    *self = Self::Chunked { at: next };
                }
                Self::Chunked { at: Chunk::Data { left } } => {
                    let taking = taken(rest, left);
                    out.extend_from_slice(&rest[..taking]);
                    at += taking;
                    let left = left - taking as u64;
                    if left > 0 {
                        *self = Self::Chunked { at: Chunk::Data { left } };
                        return Ok(Decoded { consumed: at, progress: Progress::More });
                    }
                    *self = Self::Chunked { at: Chunk::End };
                }
                Self::Chunked { at: Chunk::End } => {
                    if rest.len() < CRLF.len() {
                        return Ok(Decoded { consumed: at, progress: Progress::More });
                    }
                    if !rest.starts_with(CRLF) {
                        return Err(Failure::Malformed);
                    }
                    at += CRLF.len();
                    *self = Self::Chunked { at: Chunk::Size };
                }
                Self::Chunked { at: Chunk::Trailer } => {
                    let Line::Whole { length } = line(rest, most_line)? else {
                        return Ok(Decoded { consumed: at, progress: Progress::More });
                    };
                    at += length + CRLF.len();
                    if length == 0 {
                        // Ended: read again, it is a length of nothing, which takes nothing more.
                        *self = Self::Length { left: 0 };
                        return Ok(Decoded { consumed: at, progress: Progress::Done });
                    }
                    // A trailer's field is checked and passed over: nothing a client hands on is read from it.
                    head::header(&rest[..length]).map_err(|_| Failure::Malformed)?;
                }
            }
        }
    }
}

/// How many of `rest` a body with `left` bytes to come takes.
fn taken(rest: &[u8], left: u64) -> usize {
    rest.len().min(usize::try_from(left).unwrap_or(usize::MAX))
}

/// Where the line at the front of `rest` ends, within `most` bytes. A carriage return or line feed inside it does not
/// parse.
fn line(rest: &[u8], most: u32) -> Result<Line, Failure> {
    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let searched = &rest[..rest.len().min(most.saturating_add(CRLF.len()))];
    let Some(length) = head::find(searched, CRLF) else {
        let is_past = rest.len() >= most.saturating_add(CRLF.len());
        let partial = if is_past {
            Err(Failure::TooLarge(Limit::Head))
        } else {
            Ok(Line::Partial)
        };
        return partial;
    };
    let is_bare = rest[..length].iter().any(|&byte| byte == b'\r' || byte == b'\n');
    if is_bare {
        return Err(Failure::Malformed);
    }

    Ok(Line::Whole { length })
}

/// The size a chunk's size line gives, in hexadecimal digits, past which its extensions are passed over.
fn size_of(line: &[u8]) -> Result<u64, Failure> {
    let digits = line.iter().take_while(|byte| byte.is_ascii_hexdigit()).count();
    let (size, extensions) = line.split_at(digits);
    let extended = extensions.first().is_none_or(|&byte| byte == b';' || byte == b' ' || byte == b'\t');
    let clean = extensions.iter().all(|&byte| byte == b'\t' || !byte.is_ascii_control());
    let is_sized = digits > 0 && extended && clean;
    if !is_sized {
        return Err(Failure::Malformed);
    }
    let size = str::from_utf8(size).map_err(|_| Failure::Malformed)?;

    u64::from_str_radix(size, 16).map_err(|_| Failure::Malformed)
}

#[cfg(test)]
mod tests;
