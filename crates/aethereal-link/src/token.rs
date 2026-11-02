//! What an engine proves it is with: a token its hub minted, good for one `Hello` (ADR-0040).

use aethereal_data::{Key, Plain};
use core::fmt;

/// A token: 16 bytes of the hub's host's randomness, minted by `Engines` and good for one `Hello` (ADR-0040). JSON
/// writes it as 32 hexadecimal digits (ADR-0007), as `--hub-token` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Plain)]
pub struct Token(pub [u8; 16]);

/// A table of tokens is written in the order of their bytes.
impl Key for Token {}

/// The 32 lowercase hexadecimal digits of the token, as `--hub-token` takes it.
impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }

        Ok(())
    }
}
