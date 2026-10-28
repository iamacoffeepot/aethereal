//! How a connection cuts what it reads into frames (ADR-0033).

use aethereal_data::Plain;

/// How a connection cuts the bytes it reads into frames: data, not code, so a request carries no code (ADR-0033).
/// What a protocol frames by its own rules, as HTTP does, it reads as chunks and parses itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Plain)]
#[kind(name = "aether.tcp.framing")]
pub enum Framing {
    /// Whatever arrived, in frames of at most `most_bytes`.
    Chunks { most_bytes: u32 },
    /// A big-endian `u32` length, then that many bytes; a frame longer than `most_bytes` hangs up the connection. Each
    /// write is one frame, its length written first.
    LengthPrefixed { most_bytes: u32 },
}

/// The most bytes of a length prefix.
const PREFIX: usize = 4;

/// What a connection's buffer holds at its front, as a framing reads it.
pub enum Framed {
    /// Not a whole frame yet.
    More,
    /// A whole frame: `skip` bytes to pass over, then `bytes` bytes of frame.
    Frame { skip: usize, bytes: usize },
    /// A frame longer than the framing allows.
    TooLarge,
}

impl Framing {
    /// The most bytes a connection keeps of what it has read and not yet framed.
    pub(crate) fn most(self) -> usize {
        match self {
            Self::Chunks { most_bytes } => usize::try_from(most_bytes).unwrap_or(usize::MAX),
            Self::LengthPrefixed { most_bytes } => {
                usize::try_from(most_bytes).unwrap_or(usize::MAX).saturating_add(PREFIX)
            }
        }
    }

    /// What the front of `buffered` holds.
    pub(crate) fn frame(self, buffered: &[u8]) -> Framed {
        match self {
            Self::Chunks { most_bytes } => {
                let most = usize::try_from(most_bytes).unwrap_or(usize::MAX).max(1);
                let bytes = buffered.len().min(most);
                if bytes == 0 {
                    return Framed::More;
                }
                Framed::Frame { skip: 0, bytes }
            }
            Self::LengthPrefixed { most_bytes } => {
                let Some(prefix) = buffered.first_chunk::<PREFIX>() else {
                    return Framed::More;
                };
                let length = u32::from_be_bytes(*prefix);
                if length > most_bytes {
                    return Framed::TooLarge;
                }
                let bytes = usize::try_from(length).unwrap_or(usize::MAX);
                let is_whole = buffered.len() - PREFIX >= bytes;
                if !is_whole {
                    return Framed::More;
                }
                Framed::Frame { skip: PREFIX, bytes }
            }
        }
    }

    /// `bytes`, as the framing writes them: under `LengthPrefixed`, their length first.
    pub(crate) fn written(self, bytes: Vec<u8>) -> Vec<u8> {
        match self {
            Self::Chunks { .. } => bytes,
            Self::LengthPrefixed { .. } => {
                let length = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
                let mut written = Vec::with_capacity(PREFIX + bytes.len());
                written.extend_from_slice(&length.to_be_bytes());
                written.extend_from_slice(&bytes);
                written
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where a framing finds the front of `buffered`'s frame, as `(skip, bytes)`, or what stops it.
    fn framed(framing: Framing, buffered: &[u8]) -> Result<(usize, usize), &'static str> {
        match framing.frame(buffered) {
            Framed::Frame { skip, bytes } => Ok((skip, bytes)),
            Framed::More => Err("more"),
            Framed::TooLarge => Err("too large"),
        }
    }

    #[test]
    fn a_length_prefixed_frame_is_whole_only_once_its_length_has_arrived_and_never_past_its_limit() {
        // Catches a frame handed out before all its bytes arrived, a part of a prefix read as a length, a length read
        // little-endian, and a frame past the limit kept waiting for, so a peer fills the buffer and the connection
        // stalls instead of hanging up.
        let framing = Framing::LengthPrefixed { most_bytes: 8 };

        assert_eq!(framed(framing, &[0, 0]), Err("more"));
        assert_eq!(framed(framing, &[0, 0, 0, 3, b'a', b'b']), Err("more"));
        assert_eq!(framed(framing, &[0, 0, 0, 3, b'a', b'b', b'c', b'd']), Ok((4, 3)));
        assert_eq!(framed(framing, &[0, 0, 0, 9]), Err("too large"));
        assert_eq!(framing.written(b"abc".to_vec()), [0, 0, 0, 3, b'a', b'b', b'c']);
    }

    #[test]
    fn chunks_are_cut_at_their_limit() {
        // Catches a chunk handed out past its limit, and an empty buffer handed out as an empty frame.
        let framing = Framing::Chunks { most_bytes: 2 };

        assert_eq!(framed(framing, &[]), Err("more"));
        assert_eq!(framed(framing, b"abc"), Ok((0, 2)));
    }
}
