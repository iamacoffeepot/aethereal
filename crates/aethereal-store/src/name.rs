//! The name a binary is checked in under (ADR-0039).

use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Key, Plain, Reader, Schema, Shape, Writer};
use core::fmt;

/// The most bytes a name holds.
const MOST_BYTES: usize = 128;

/// A name in the store: 1 to 128 of ASCII letters, digits, `_`, `-`, and `.` (ADR-0039). It points at the hash last
/// checked in under it. A name that does not conform is refused where it is made and where it is read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreName(String);

impl StoreName {
    /// The name `text` spells.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidStoreName`] when `text` does not conform.
    pub fn parse(text: impl Into<String>) -> Result<Self, DecodeError> {
        let text = text.into();
        let is_name = spells(&text);
        if !is_name {
            return Err(DecodeError::InvalidStoreName);
        }

        Ok(Self(text))
    }

    /// The name's text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `text` spells a name: the one statement of the rule.
fn spells(text: &str) -> bool {
    let has_text = !text.is_empty();
    let fits = text.len() <= MOST_BYTES;
    let is_plain = text.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));

    has_text && fits && is_plain
}

impl fmt::Display for StoreName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Schema for StoreName {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for StoreName {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.0.encode(out)
    }
}

impl<B: Boundary> Decode<B> for StoreName {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Self::parse(String::decode(input)?)
    }
}

impl Plain for StoreName {}

/// Names are kept in the order of their bytes.
impl Key for StoreName {}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_data::{Budget, from_bytes, to_bytes};

    /// `text` written as a string and read back as a name.
    fn decoded(text: &str) -> Result<StoreName, DecodeError> {
        let Ok(bytes) = to_bytes(&String::from(text)) else {
            panic!("a string is written");
        };

        from_bytes(&bytes, &mut Budget::new(1 << 10))
    }

    #[test]
    fn a_name_that_does_not_conform_is_refused_where_it_is_read() {
        // Catches a check made only where a name is made, so a name read from a caller or the names file could hold
        // a `/` that climbs out of the store, a space, or no text; and a rule off by one at its length.
        let longest = "a".repeat(MOST_BYTES);
        for good in ["hub", "aether-headless_0.1.2", "A", longest.as_str()] {
            assert_eq!(decoded(good).map(|name| name.0), Ok(String::from(good)), "the name `{good}`");
        }
        let too_long = "a".repeat(MOST_BYTES + 1);
        for bad in ["", "a/b", "../hub", "a b", "é", too_long.as_str()] {
            assert_eq!(decoded(bad), Err(DecodeError::InvalidStoreName), "the name `{bad}`");
        }
    }
}
