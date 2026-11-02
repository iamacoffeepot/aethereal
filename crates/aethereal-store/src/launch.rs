//! Launching a stored binary: a copy of it to run, its build held while the asker lives (ADR-0040).

use crate::{StoreHash, StoreName};
use aethereal_data::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use core::fmt;

/// The refusal of text that names no binary, with the rule it breaks.
const REFUSED: DecodeError = DecodeError::Refused {
    name: "Binary",
    rule: "a binary is the 64 lowercase hexadecimal digits of a hash, or a name of 1 to 128 of ASCII letters, digits, \
           `_`, `-`, and `.`",
};

/// Copy the entry `binary` names to a launch copy, a file to run, and hold the entry for as long as the asker lives:
/// asked of [`Store`](crate::Store), and answered [`Launched`]. The hold is a [`Hold`](crate::Hold)'s, and counts as the
/// entry's use. The launch copy is removed when the asker ends, and every one is removed as the store begins.
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.launch")]
pub struct Launch {
    /// The binary to launch.
    pub binary: Binary,
}

/// A stored binary, as an asker names it: by a name pointing at it, or by its hash. A name never spells a hash, so it
/// is one string, on the wire and in JSON (ADR-0040): the hash's 64 lowercase hexadecimal digits when it is one, and
/// the name otherwise. Text that spells neither is refused where it is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Binary {
    /// The entry the name points at.
    Named(StoreName),
    /// The entry of the hash.
    Hash(StoreHash),
}

impl Binary {
    /// The binary `text` names: a hash when it spells one, and a name otherwise.
    ///
    /// # Errors
    ///
    /// [`DecodeError::Refused`], naming `Binary` and its rule, when `text` spells neither.
    pub fn parse(text: impl Into<String>) -> Result<Self, DecodeError> {
        let text = text.into();

        let Some(hash) = StoreHash::from_hex(&text) else {
            return StoreName::parse(text).map(Self::Named).map_err(|_| REFUSED);
        };

        Ok(Self::Hash(hash))
    }
}

/// The one string that names it: the hash's 64 lowercase hexadecimal digits, or the name.
impl fmt::Display for Binary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Named(name) => name.fmt(f),
            Self::Hash(hash) => hash.fmt(f),
        }
    }
}

impl Schema for Binary {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for Binary {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.to_string().encode(out)
    }
}

impl<B: Boundary> Decode<B> for Binary {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Self::parse(String::decode(input)?)
    }
}

impl Plain for Binary {}

/// What became of a [`Launch`].
#[derive(Debug, Clone, PartialEq, Eq, Plain)]
#[kind(name = "aether.store.launched")]
pub enum Launched {
    /// The launch copy is made, and the entry held until the asker ends.
    Ready {
        /// The launch copy's absolute path on the host's machine: `running/<name>-<n>` beneath the store's directory,
        /// marked executable. `<name>` is the name the launch asked by; for a hash, the first name pointing at it in
        /// the order of their bytes, or the hash's first 12 hexadecimal digits when none does. `<n>` is the store's
        /// count of launches since it began.
        program: String,
        /// The hash of the entry it copies.
        hash: StoreHash,
    },
    /// The store holds no entry of that name or hash, so nothing is held.
    NotStored,
    /// The host could not make the launch copy. The entry is held until the asker ends all the same.
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use aethereal_data::{Budget, from_bytes, to_bytes};

    /// `text` written as a string and read back as a binary.
    fn decoded(text: &str) -> Result<Binary, DecodeError> {
        let Ok(bytes) = to_bytes(&String::from(text)) else {
            panic!("a string is written");
        };

        from_bytes(&bytes, &mut Budget::new(1 << 10))
    }

    #[test]
    fn a_binary_is_a_hash_when_its_string_spells_one_a_name_otherwise_and_refused_when_neither() {
        // Catches a hash read as a name, which would launch whatever a name of those digits points at, or never find
        // the entry; digits that are not a hash's (one short, or uppercase) taken for one; text that is neither let
        // through, as a name with a `/` that climbs out of the store; and a binary that does not write back as the
        // string it was read from.
        let hex = "0123456789abcdef".repeat(4);
        let Some(hash) = StoreHash::from_hex(&hex) else {
            panic!("the digits spell a hash");
        };
        assert_eq!(decoded(&hex), Ok(Binary::Hash(hash)));
        for name in ["engine", &hex[1..], hex.to_uppercase().as_str()] {
            let Ok(expected) = StoreName::parse(name) else {
                panic!("`{name}` is a name");
            };
            assert_eq!(decoded(name), Ok(Binary::Named(expected)), "the binary `{name}`");
        }
        for bad in ["", "a/b", "a b"] {
            assert_eq!(decoded(bad), Err(REFUSED), "the binary `{bad}`");
        }
        for text in [hex.as_str(), "engine"] {
            let Ok(binary) = decoded(text) else {
                panic!("`{text}` names a binary");
            };
            let written = to_bytes(&binary);
            let string = written.ok().and_then(|bytes| from_bytes::<String>(&bytes, &mut Budget::new(1 << 10)).ok());
            assert_eq!(string.as_deref(), Some(text), "the binary `{text}` writes as its string");
        }
    }
}
