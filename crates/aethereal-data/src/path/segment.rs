//! One step of a path: an actor's kind and its key (ADR-0008).

use crate::KindName;
use crate::path::cut::cut;
use crate::path::{ChildKey, KeyName};
use core::fmt;

/// The mark between a segment's kind and a named key.
const KEY_MARK: u8 = b':';

/// One step of a path: the name of an actor's kind, and its key (ADR-0008).
///
/// It prints as `name:key` for a named key and as `name` alone for `ChildKey::Only`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PathSegment {
    kind: KindName,
    key: ChildKey,
}

impl PathSegment {
    /// The segment of the actor of kind `kind` with the key `key`.
    #[must_use]
    pub const fn new(kind: KindName, key: ChildKey) -> Self {
        Self { kind, key }
    }

    /// The name of the actor's kind.
    #[must_use]
    pub const fn kind(&self) -> &KindName {
        &self.kind
    }

    /// The actor's key.
    #[must_use]
    pub const fn key(&self) -> &ChildKey {
        &self.key
    }

    /// Whether `bytes` spell a segment: a kind's name, then for a named key the mark and the key's name.
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
                Self::new(KindName::from_conforming(kind), ChildKey::Named(KeyName::from_conforming(key)))
            }
            None => Self::new(KindName::from_conforming(text), ChildKey::Only),
        }
    }
}

impl fmt::Display for PathSegment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.key {
            ChildKey::Only => write!(f, "{}", self.kind),
            ChildKey::Named(key) => write!(f, "{}{}{key}", self.kind, char::from(KEY_MARK)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        // Catches a reader that puts the key in the kind, or reads an absent key as a named one.
        const SURFACE: KindName = KindName::new("aether.window.surface");
        let named = PathSegment::new(SURFACE, ChildKey::Named(KeyName::new("left")));
        assert_eq!(PathSegment::from_conforming("aether.window.surface:left"), named);
        assert_eq!(PathSegment::from_conforming("aether.window.surface"), PathSegment::new(SURFACE, ChildKey::Only));
    }
}
