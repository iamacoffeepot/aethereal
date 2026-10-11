//! A key's name (ADR-0008).

use crate::plain::write_text;
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt;

/// The most bytes a key's name holds.
const MAX_BYTES: usize = 64;

/// The name that tells one actor of a type from the others in its place (ADR-0008).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyName(Cow<'static, str>);

impl KeyName {
    /// The name written as `name`, for a constant.
    ///
    /// # Panics
    ///
    /// Panics when `name` does not conform; in a constant that stops the build.
    ///
    /// ```rust
    /// use aethereal_data::KeyName;
    ///
    /// const LEFT: KeyName = KeyName::new("left-2.b_c");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::KeyName;
    ///
    /// const FLAG: KeyName = KeyName::new("-left");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::KeyName;
    ///
    /// const UP: KeyName = KeyName::new("..");
    /// ```
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        assert!(
            Self::spells(name.as_bytes()),
            "a key name is 1 to 64 bytes: a lowercase letter or a digit, then lowercase letters, digits, `_`, `-`, and `.`"
        );

        Self(Cow::Borrowed(name))
    }

    /// Whether `bytes` spell a key's name: the one statement of the rule.
    pub(super) const fn spells(bytes: &[u8]) -> bool {
        let has_text = !bytes.is_empty();
        let fits = bytes.len() <= MAX_BYTES;
        let sized = has_text && fits;
        if !sized {
            return false;
        }
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            let is_head = byte.is_ascii_lowercase() || byte.is_ascii_digit();
            let is_tail = is_head || byte == b'_' || byte == b'-' || byte == b'.';
            let begins = index == 0 && is_head;
            let continues = index > 0 && is_tail;
            let allowed = begins || continues;
            if !allowed {
                return false;
            }
            index += 1;
        }

        true
    }

    /// The name `text` spells, for a caller that has already put `text` through [`Self::spells`].
    pub(super) fn from_conforming(text: &str) -> Self {
        Self(Cow::Owned(String::from(text)))
    }

    /// The name's text.
    const fn text(&self) -> &str {
        match &self.0 {
            Cow::Borrowed(text) => text,
            Cow::Owned(text) => text.as_str(),
        }
    }
}

impl fmt::Display for KeyName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl Schema for KeyName {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for KeyName {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_text(out.bytes(), self.text())
    }
}

impl<B: Boundary> Decode<B> for KeyName {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let text = String::decode(input)?;
        let is_name = Self::spells(text.as_bytes());
        if is_name {
            Ok(Self(Cow::Owned(text)))
        } else {
            Err(DecodeError::InvalidKeyName)
        }
    }
}

impl Plain for KeyName {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::samples::decode_text;
    use alloc::string::ToString;

    #[test]
    fn conforming_keys_decode_and_print() {
        // Catches a decoder or printer that disagrees with `spells`, and a length limit one byte short.
        let longest = "k".repeat(64);
        for name in ["a", "0", "main", "left-2.b_c", "a.", "a..", "9-", longest.as_str()] {
            let Ok(decoded) = decode_text::<KeyName>(name) else {
                panic!("the key `{name}` decodes");
            };
            assert_eq!(decoded.to_string(), name);
        }
    }

    #[test]
    fn nonconforming_keys_are_refused() {
        // Catches a decoder that accepts a key that reads as a relative path or a flag, or one past the limit.
        let too_long = "k".repeat(65);
        for name in ["", ".", "..", ".a", "-x", "_a", "A", "aB", "a b", "a/b", "a:b", "\u{e9}", too_long.as_str()] {
            assert_eq!(decode_text::<KeyName>(name), Err(DecodeError::InvalidKeyName), "the key `{name}`");
        }
    }

    #[test]
    fn decoded_key_equals_constant_key() {
        // Catches comparisons that tell borrowed text from owned text.
        const MAIN: KeyName = KeyName::new("main");
        assert_eq!(decode_text::<KeyName>("main"), Ok(MAIN));
        assert_eq!(KeyName::from_conforming("main"), MAIN);
    }
}
