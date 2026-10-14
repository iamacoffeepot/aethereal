//! One step of a path: an actor's kind, and its key where it has one (ADR-0008).

use crate::KindName;
use crate::path::cut::cut;
use crate::path::{ActorKey, KeyName};
use core::fmt;

/// The mark between a segment's kind and its key.
const KEY_MARK: u8 = b':';

/// One step of a path: the name of an actor's kind, and its key where the actor is instanced (ADR-0008).
///
/// It prints as `name:key` for an instanced actor and as `name` alone for a singleton.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathSegment {
    kind: KindName,
    /// The key's name. None for a singleton, which has no key: fixed when the segment is built.
    key: Option<KeyName>,
}

impl PathSegment {
    /// The segment of the actor of kind `kind` with the key `key`: `()` for a singleton.
    #[must_use]
    pub fn new<K: ActorKey>(kind: KindName, key: &K) -> Self {
        Self { kind, key: key.name() }
    }

    /// The segment of the instanced actor of kind `kind` whose key is named `key`.
    ///
    /// For the engine, which holds a key as the name an effect carries. An author's code builds a segment from
    /// the typed key, with [`Self::new`].
    #[must_use]
    pub const fn instanced(kind: KindName, key: KeyName) -> Self {
        Self { kind, key: Some(key) }
    }

    /// The name of the actor's kind.
    #[must_use]
    pub const fn kind(&self) -> &KindName {
        &self.kind
    }

    /// The name of the actor's key, or none for a singleton.
    #[must_use]
    pub const fn key(&self) -> Option<&KeyName> {
        self.key.as_ref()
    }

    /// Whether `bytes` spell a segment: a kind's name, then for an instanced actor the mark and the key's name.
    pub(super) const fn spells(bytes: &[u8]) -> bool {
        let (kind, named) = cut(bytes, KEY_MARK);
        let is_kind = KindName::spells(kind);

        match named {
            Some(key) => is_kind && KeyName::spells(key),
            None => is_kind,
        }
    }

    /// The segment `text` spells, for a caller that has already put `text` through [`Self::spells`].
    pub(super) fn from_conforming(text: &str) -> Self {
        match text.split_once(char::from(KEY_MARK)) {
            Some((kind, key)) => {
                Self { kind: KindName::from_conforming(kind), key: Some(KeyName::from_conforming(key)) }
            }
            None => Self { kind: KindName::from_conforming(text), key: None },
        }
    }
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            None => write!(f, "{}", self.kind),
            Some(key) => write!(f, "{}{}{key}", self.kind, char::from(KEY_MARK)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::TypedKey;
    use alloc::string::ToString;

    #[test]
    fn segment_prints_as_it_is_read() {
        // Catches a printer and a reader that disagree on where the key starts, as at a `.` inside a key.
        for text in ["aether.window", "aether.window.surface:left", "a:0", "a.b:c.d"] {
            assert!(PathSegment::spells(text.as_bytes()), "the segment `{text}`");
            assert_eq!(PathSegment::from_conforming(text).to_string(), text);
        }
    }

    #[test]
    fn read_segment_equals_built_segment() {
        // Catches a reader that puts the key in the kind, or reads an absent key as a present one.
        const SURFACE: KindName = KindName::new("aether.window.surface");
        assert_eq!(PathSegment::from_conforming("aether.window.surface:7"), PathSegment::new(SURFACE, &7u64));
        assert_eq!(PathSegment::from_conforming("aether.window.surface"), PathSegment::new(SURFACE, &()));
    }

    #[test]
    fn segment_key_reads_back_as_its_typed_value() {
        // Catches a segment that keeps a singleton's key as a name, or loses an instanced actor's.
        const SURFACE: KindName = KindName::new("aether.window.surface");
        let key = PathSegment::new(SURFACE, &7u64).key().and_then(u64::from_name);
        assert_eq!(key, Some(7));
        assert_eq!(PathSegment::new(SURFACE, &()).key(), None);
    }
}
