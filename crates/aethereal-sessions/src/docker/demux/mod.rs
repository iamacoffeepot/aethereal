//! Docker's stream frames (ADR-0057): what a container with no terminal writes on an attach, and in its logs, each
//! chunk behind an 8-byte header, `[stream, 0, 0, 0, size as a big-endian u32]`, taken apart into the stream it came on.

#[cfg(test)]
mod tests;

use super::{DockerError, Std};
use aethereal_data::Plain;
use core::mem;

/// The bytes of a frame's header.
const HEADER_BYTES: usize = 8;

/// The most bytes of an error Docker writes into a stream that are kept: the rest is passed over.
const MOST_MESSAGE_BYTES: usize = 4096;

/// What a stream of frames has been read to, across the reads it arrives in: a read may end anywhere, within a header or
/// a payload, and the next takes up from there.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Demux {
    /// Between frames: the bytes of the next header read so far, fewer than eight.
    Header(Vec<u8>),
    /// Within a frame's payload, `left` bytes of it still to come, read into `into`.
    Payload { into: Sink, left: u32 },
}

/// Where a frame's payload goes.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) enum Sink {
    /// Out as output of one stream: stdout for frames Docker names stdout or stdin, as its own client reads them.
    Output(Std),
    /// Into the error Docker writes on its stream 3, as far as it is kept.
    Daemon(Vec<u8>),
}

/// A piece of output taken out of the frames, before it is put as a blob.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
pub(super) struct Piece {
    pub stream: Std,
    pub bytes: Vec<u8>,
}

impl Demux {
    /// At the start of a stream.
    pub(super) const fn new() -> Self {
        Self::Header(Vec::new())
    }

    /// Reads `bytes`, the next of the stream, adding what output they hold to `pieces`: payloads of one stream that
    /// follow each other join into one piece, so a read gives at most one piece for each change of stream.
    ///
    /// # Errors
    ///
    /// [`DockerError::Stream`] once a frame on Docker's error stream is whole, and [`DockerError::Malformed`] for a
    /// header naming a stream Docker has not: the stream can be read no further. The pieces before it are added.
    pub(super) fn push(&mut self, mut bytes: &[u8], pieces: &mut Vec<Piece>) -> Result<(), DockerError> {
        while !bytes.is_empty() {
            match self {
                Self::Header(header) => {
                    let wanted = HEADER_BYTES - header.len();
                    let (taken, rest) = bytes.split_at(wanted.min(bytes.len()));
                    header.extend_from_slice(taken);
                    bytes = rest;
                    if header.len() == HEADER_BYTES {
                        let read = mem::take(header);
                        *self = Self::framed(&read)?;
                    }
                }
                Self::Payload { into, left } => {
                    let wanted = usize::try_from(*left).unwrap_or(usize::MAX);
                    let (taken, rest) = bytes.split_at(wanted.min(bytes.len()));
                    bytes = rest;
                    *left -= u32::try_from(taken.len()).unwrap_or(*left);
                    into.take(taken, pieces);
                    if *left == 0 {
                        let into = mem::replace(into, Sink::Output(Std::Stdout));
                        *self = into.finished()?;
                    }
                }
            }
        }

        Ok(())
    }

    /// What follows the whole header `header`: the payload it announces, or the next header when it announces none.
    fn framed(header: &[u8]) -> Result<Self, DockerError> {
        let [stream, _, _, _, size @ ..] = header else {
            return Err(DockerError::Malformed);
        };
        let Ok(size) = <[u8; 4]>::try_from(size) else {
            return Err(DockerError::Malformed);
        };
        let into = match *stream {
            0 | 1 => Sink::Output(Std::Stdout),
            2 => Sink::Output(Std::Stderr),
            3 => Sink::Daemon(Vec::new()),
            _ => return Err(DockerError::Malformed),
        };
        let left = u32::from_be_bytes(size);
        if left == 0 {
            return into.finished();
        }

        Ok(Self::Payload { into, left })
    }
}

impl Sink {
    /// Takes `taken`, the next of a payload.
    fn take(&mut self, taken: &[u8], pieces: &mut Vec<Piece>) {
        match self {
            Self::Output(stream) => {
                let stream = *stream;
                match pieces.last_mut() {
                    Some(last) if last.stream == stream => last.bytes.extend_from_slice(taken),
                    _ => pieces.push(Piece { stream, bytes: taken.to_vec() }),
                }
            }
            Self::Daemon(message) => {
                let room = MOST_MESSAGE_BYTES.saturating_sub(message.len());
                message.extend_from_slice(taken.get(..room).unwrap_or(taken));
            }
        }
    }

    /// What follows once its payload is whole: the next header, or the error Docker wrote.
    fn finished(self) -> Result<Demux, DockerError> {
        match self {
            Self::Output(_) => Ok(Demux::new()),
            Self::Daemon(message) => {
                let message = String::from_utf8_lossy(&message).trim_end().to_owned();
                Err(DockerError::Stream { message })
            }
        }
    }
}
