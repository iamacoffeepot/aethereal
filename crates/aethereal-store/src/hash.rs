//! What names a stored binary: the hash of its bytes (ADR-0039).

use aethereal_data::{Key, Plain};
use core::fmt;

/// The BLAKE3 hash of a stored binary's bytes (ADR-0014, ADR-0039): one hash names one build. JSON writes it as 64
/// hexadecimal digits (ADR-0007), and the store's host names the entry's file with the same digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Plain)]
pub struct StoreHash(pub [u8; 32]);

/// A set of hashes, or a map keyed by them, is written in the order of their bytes.
impl Key for StoreHash {}

impl StoreHash {
    /// The hash the 64 lowercase hexadecimal digits of `text` spell: none when they spell none.
    #[must_use]
    pub fn from_hex(text: &str) -> Option<Self> {
        let digits = text.as_bytes();
        if digits.len() != 64 {
            return None;
        }
        let mut bytes = [0; 32];
        for (byte, pair) in bytes.iter_mut().zip(digits.chunks_exact(2)) {
            *byte = (digit(pair[0])? << 4) | digit(pair[1])?;
        }

        Some(Self(bytes))
    }
}

/// The value of one lowercase hexadecimal digit: none when `byte` is not one.
const fn digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// The 64 lowercase hexadecimal digits of the hash.
impl fmt::Display for StoreHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }

        Ok(())
    }
}
