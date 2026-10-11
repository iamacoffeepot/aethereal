//! A kind's name (ADR-0006).

use crate::plain::write_text;
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Schema, Shape, Writer};
use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt;

/// A kind's name: segments separated by `.` (ADR-0006).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KindName(Cow<'static, str>);

impl KindName {
    /// The name written as `name`, for a constant.
    ///
    /// # Panics
    ///
    /// Panics when `name` does not conform; in a constant that stops the build.
    ///
    /// ```rust
    /// use aethereal_data::KindName;
    ///
    /// const NAME: KindName = KindName::new("aether.audio.note_on");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::KindName;
    ///
    /// const NAME: KindName = KindName::new("aether.Audio");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::KindName;
    ///
    /// const NAME: KindName = KindName::new("aether..audio");
    /// ```
    #[must_use]
    pub const fn new(name: &'static str) -> Self {
        assert!(conforms(name), "a kind name is segments of lowercase letters, digits, and `_`, separated by `.`");

        Self(Cow::Borrowed(name))
    }

    /// The name's UTF-8 bytes, as an id hashes them.
    pub(super) const fn bytes(&self) -> &[u8] {
        self.text().as_bytes()
    }

    /// The name's text.
    const fn text(&self) -> &str {
        match &self.0 {
            Cow::Borrowed(text) => text,
            Cow::Owned(text) => text.as_str(),
        }
    }
}

/// Whether `name` is one or more segments separated by `.`, each a lowercase
/// letter followed by lowercase letters, digits, and `_`.
const fn conforms(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut in_segment = false;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let is_letter = byte.is_ascii_lowercase();
        let is_tail = is_letter || byte.is_ascii_digit() || byte == b'_';
        let begins = !in_segment && is_letter;
        let continues = in_segment && is_tail;
        let closes = in_segment && byte == b'.';
        let allowed = begins || continues || closes;
        if !allowed {
            return false;
        }
        in_segment = begins || continues;
        index += 1;
    }

    in_segment
}

impl fmt::Display for KindName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl Schema for KindName {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for KindName {
    fn encode(&self, out: &mut B::Writer<'_>) -> Result<(), EncodeError> {
        write_text(out.bytes(), self.text())
    }
}

impl<B: Boundary> Decode<B> for KindName {
    fn decode(input: &mut B::Reader<'_>) -> Result<Self, DecodeError> {
        let text = <String as Decode<B>>::decode(input)?;
        let is_name = conforms(&text);
        if is_name {
            Ok(Self(Cow::Owned(text)))
        } else {
            Err(DecodeError::InvalidKindName)
        }
    }
}

impl Plain for KindName {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Budget, from_bytes, to_bytes};
    use alloc::string::ToString;

    #[test]
    fn conforming_names_decode_and_print() {
        // Catches a decoder or printer that disagrees with `conforms`.
        for name in ["a", "aether.audio.note_on", "a1.b_2"] {
            let text = String::from(name);
            let Ok(bytes) = to_bytes(&text) else {
                panic!("a name encodes as a string");
            };
            let back = from_bytes::<KindName>(&bytes, &mut Budget::new(64));
            let Ok(decoded) = back else {
                panic!("a conforming name decodes");
            };
            assert_eq!(decoded.to_string(), text);
        }
    }

    #[test]
    fn nonconforming_names_are_refused() {
        // Catches a decoder that accepts text outside the name rule.
        for name in [".", "a.", ".a", "a..b", "A", "a.B", "1a", "a.1", "_a", "a-b", "a b", "\u{e9}", ""] {
            let text = String::from(name);
            let Ok(bytes) = to_bytes(&text) else {
                panic!("a name encodes as a string");
            };
            let refused = from_bytes::<KindName>(&bytes, &mut Budget::new(64));
            assert_eq!(refused, Err(DecodeError::InvalidKindName));
        }
    }

    #[test]
    fn decoded_name_equals_constant_name() {
        // Catches derived comparisons that tell borrowed text from owned text.
        const CONSTANT: KindName = KindName::new("aether.audio.note_on");
        let text = String::from("aether.audio.note_on");
        let Ok(bytes) = to_bytes(&text) else {
            panic!("a name encodes as a string");
        };
        let back = from_bytes::<KindName>(&bytes, &mut Budget::new(64));
        assert_eq!(back, Ok(CONSTANT));
    }

    #[test]
    fn name_bytes_equal_string_bytes() {
        // Catches an encoder that writes what a string decoder would not read.
        assert_eq!(to_bytes(&KindName::new("a.b")), to_bytes(&String::from("a.b")));
    }
}
