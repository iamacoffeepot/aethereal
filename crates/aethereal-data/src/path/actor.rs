//! An actor's path (ADR-0008).

use crate::path::PathSegment;
use crate::path::cut::cut;
use crate::plain::write_text;
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Key, Plain, Reader, Schema, Shape, Writer};
use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use core::error::Error;
use core::fmt;

/// The mark between one segment and the next.
const SEGMENT_MARK: u8 = b'/';

/// Where an actor stands: a root segment, then the segment of each actor beneath it in turn (ADR-0008).
///
/// Its text is its segments joined by `/`, and that text is what it is stored, compared, and ordered as.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorPath(Cow<'static, str>);

impl ActorPath {
    /// The most segments a path holds.
    pub const MAX_DEPTH: usize = 32;

    /// The path written as `path`, for a constant.
    ///
    /// # Panics
    ///
    /// Panics when `path` does not conform or is deeper than [`Self::MAX_DEPTH`]; in a constant that stops the
    /// build.
    ///
    /// ```rust
    /// use aethereal_data::ActorPath;
    ///
    /// const LEFT: ActorPath = ActorPath::new("aether.window/aether.window.surface:left");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::ActorPath;
    ///
    /// const UNKEYED: ActorPath = ActorPath::new("aether.window/aether.window.surface:");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::ActorPath;
    ///
    /// const TOO_DEEP: ActorPath = ActorPath::new("a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a/a");
    /// ```
    #[must_use]
    pub const fn new(path: &'static str) -> Self {
        assert!(
            conforms(path.as_bytes()),
            "an actor path is 1 to 32 segments joined by `/`, each a kind name, or a kind name, `:`, and a key name"
        );

        Self(Cow::Borrowed(path))
    }

    /// The path of the root `root` names.
    #[must_use]
    pub fn from_root(root: &PathSegment) -> Self {
        Self(Cow::Owned(root.to_string()))
    }

    /// The path of the child `segment` names beneath this path's actor.
    pub fn child(&self, segment: &PathSegment) -> Result<Self, PathTooDeep> {
        let is_full = self.depth() >= Self::MAX_DEPTH;
        if is_full {
            Err(PathTooDeep)
        } else {
            Ok(Self(Cow::Owned(format!("{self}{}{segment}", char::from(SEGMENT_MARK)))))
        }
    }

    /// The first segment: the root this path's actor stands beneath, or is.
    #[must_use]
    pub fn root(&self) -> PathSegment {
        let text = self.text();
        let root = text.split_once(char::from(SEGMENT_MARK)).map_or(text, |(root, _)| root);

        PathSegment::from_conforming(root)
    }

    /// The last segment: this path's own actor.
    #[must_use]
    pub fn last(&self) -> PathSegment {
        let text = self.text();
        let last = text.rsplit_once(char::from(SEGMENT_MARK)).map_or(text, |(_, last)| last);

        PathSegment::from_conforming(last)
    }

    /// The path of the actor this one stands directly beneath; a root has none.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        let (parent, _) = self.text().rsplit_once(char::from(SEGMENT_MARK))?;

        Some(Self(Cow::Owned(String::from(parent))))
    }

    /// Whether this path's actor stands beneath `other`'s, at any depth. No path is beneath itself.
    #[must_use]
    pub fn is_beneath(&self, other: &Self) -> bool {
        self.text().strip_prefix(other.text()).is_some_and(|deeper| deeper.starts_with(char::from(SEGMENT_MARK)))
    }

    /// Every segment, from the root to the last.
    pub fn segments(&self) -> impl Iterator<Item = PathSegment> + '_ {
        self.text().split(char::from(SEGMENT_MARK)).map(PathSegment::from_conforming)
    }

    /// How many segments the path holds.
    fn depth(&self) -> usize {
        self.text().split(char::from(SEGMENT_MARK)).count()
    }

    /// The path's text.
    const fn text(&self) -> &str {
        match &self.0 {
            Cow::Borrowed(text) => text,
            Cow::Owned(text) => text.as_str(),
        }
    }
}

/// Whether `bytes` spell a path: 1 to [`ActorPath::MAX_DEPTH`] segments, with one mark between each two.
const fn conforms(bytes: &[u8]) -> bool {
    let mut rest = bytes;
    let mut depth = 0;
    while depth < ActorPath::MAX_DEPTH {
        let (segment, deeper) = cut(rest, SEGMENT_MARK);
        if !PathSegment::spells(segment) {
            return false;
        }
        let Some(beneath) = deeper else {
            return true;
        };
        rest = beneath;
        depth += 1;
    }

    false
}

impl fmt::Display for ActorPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.text())
    }
}

impl Schema for ActorPath {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for ActorPath {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_text(out.bytes(), self.text())
    }
}

impl<B: Boundary> Decode<B> for ActorPath {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        let text = String::decode(input)?;
        let is_path = conforms(text.as_bytes());
        if is_path {
            Ok(Self(Cow::Owned(text)))
        } else {
            Err(DecodeError::InvalidActorPath)
        }
    }
}

impl Plain for ActorPath {}

impl Key for ActorPath {}

/// A child was asked for beneath a path already [`ActorPath::MAX_DEPTH`] segments deep (ADR-0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathTooDeep;

impl fmt::Display for PathTooDeep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "actor path: a child would stand deeper than {} segments", ActorPath::MAX_DEPTH)
    }
}

impl Error for PathTooDeep {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::samples::{Side, decode_text, deep};
    use crate::{KindName, to_bytes};
    use alloc::collections::BTreeMap;
    use alloc::vec::Vec;
    use core::cmp::Ordering;

    const WINDOW: ActorPath = ActorPath::new("aether.window");
    const LEFT: ActorPath = ActorPath::new("aether.window/aether.window.surface:left");

    /// The segment of the kind `a` with no name, as `deep` writes it.
    fn plain_segment() -> PathSegment {
        PathSegment::new(KindName::new("a"), &())
    }

    /// The path `text` spells, decoded as it arrives from outside.
    fn path(text: &str) -> ActorPath {
        let Ok(decoded) = decode_text::<ActorPath>(text) else {
            panic!("the path `{text}` decodes");
        };

        decoded
    }

    #[test]
    fn conforming_paths_decode_and_print() {
        // Catches a decoder or printer that disagrees with `conforms`, and a depth or key limit one short.
        let longest_key = format!("a:{}", "k".repeat(64));
        let deepest = deep(32);
        let accepted = [
            "a",
            "aether.window",
            "aether.window/aether.window.surface:left",
            "a:0/b",
            "a.b:c.d/e_1:f-2",
            "a:k./b:k..",
            longest_key.as_str(),
            deepest.as_str(),
        ];
        for text in accepted {
            assert_eq!(path(text).to_string(), text);
        }
    }

    #[test]
    fn nonconforming_paths_are_refused() {
        // Catches a decoder that accepts a second spelling of a path, an empty part, or a path past a limit.
        let long_key = format!("a:{}", "k".repeat(65));
        let too_deep = deep(33);
        let refused = [
            "",
            "a:.",
            "a:..",
            "a:-x",
            "a:",
            ":a",
            "a//b",
            "a/",
            "/a",
            "/",
            ":",
            "A",
            "a/B",
            "a:K",
            "a:b:c",
            "a b",
            "a/b:",
            "1a",
            "\u{e9}",
            long_key.as_str(),
            too_deep.as_str(),
        ];
        for text in refused {
            assert_eq!(decode_text::<ActorPath>(text), Err(DecodeError::InvalidActorPath), "the path `{text}`");
        }
    }

    #[test]
    fn decoded_path_equals_constant_path() {
        // Catches comparisons that tell borrowed text from owned text, in equality and in order.
        let decoded = path("aether.window/aether.window.surface:left");
        assert_eq!(decoded, LEFT);
        assert_eq!(decoded.cmp(&LEFT), Ordering::Equal);
    }

    #[test]
    fn path_bytes_equal_string_bytes() {
        // Catches an encoder that writes what a string decoder would not read.
        assert_eq!(to_bytes(&LEFT), to_bytes(&String::from("aether.window/aether.window.surface:left")));
    }

    #[test]
    fn built_path_equals_written_path() {
        // Catches a builder that writes a spelling `new` and the decoder would spell otherwise.
        let surface = PathSegment::new(KindName::new("aether.window.surface"), &Side::Left);
        let window = ActorPath::from_root(&PathSegment::new(KindName::new("aether.window"), &()));
        assert_eq!(window, WINDOW);
        assert_eq!(window.child(&surface), Ok(LEFT));
        assert_eq!(decode_text::<ActorPath>(&LEFT.to_string()), Ok(LEFT));
    }

    #[test]
    fn parts_are_read_at_the_right_end() {
        // Catches a root read from the last segment, a last read from the first, and segments out of order.
        let deepest = path("a:k/b/c.d:e");
        let texts: Vec<String> = deepest.segments().map(|segment| segment.to_string()).collect();
        assert_eq!(texts, ["a:k", "b", "c.d:e"]);
        assert_eq!(deepest.root().to_string(), "a:k");
        assert_eq!(deepest.last().to_string(), "c.d:e");
        assert_eq!(WINDOW.root(), WINDOW.last());
    }

    #[test]
    fn parent_stops_at_the_root() {
        // Catches a root that answers with a parent, and a parent that drops more or less than one segment.
        assert_eq!(WINDOW.parent(), None);
        assert_eq!(LEFT.parent(), Some(WINDOW));
        assert_eq!(path("a:k/b/c.d:e").parent(), Some(path("a:k/b")));
    }

    #[test]
    fn child_is_refused_past_the_deepest_path() {
        // Catches a depth check that is one off: a 32nd segment refused, or a 33rd accepted.
        let segment = plain_segment();
        let nearly = path(&deep(31));
        let deepest = path(&deep(32));
        assert_eq!(nearly.child(&segment), Ok(deepest.clone()));
        assert_eq!(deepest.child(&segment), Err(PathTooDeep));
        assert_eq!(deepest.segments().count(), ActorPath::MAX_DEPTH);
        assert_eq!(deepest.parent(), Some(nearly));
    }

    #[test]
    fn beneath_follows_segments_not_text() {
        // Catches a test by text prefix alone: `a.b` and `a:k` begin with `a` and stand nowhere near it.
        let base = path("a");
        assert!(path("a/b").is_beneath(&base));
        assert!(path("a/b/c:k").is_beneath(&base));
        assert!(!base.is_beneath(&base));
        assert!(!base.is_beneath(&path("a/b")));
        for text in ["a.b", "a:k", "a_b", "a0", "a.b/c", "a:k/b", "b/a"] {
            assert!(!path(text).is_beneath(&base), "the path `{text}`");
        }
        assert!(!path("a:k-x/b").is_beneath(&path("a:k")));
        assert!(path(&deep(32)).is_beneath(&base));
    }

    #[test]
    fn map_order_is_text_order_and_a_branch_is_one_run() {
        // Catches an order by depth or by parts, which would differ from a string's and split a branch in a map.
        let texts = ["b", "a_b", "a:k/b", "a:k-x", "a:k", "a0", "a/b/c", "a/b:k", "a/b", "a.b/c", "a.b", "a"];
        let paths: BTreeMap<ActorPath, u8> = texts.iter().map(|text| (path(text), 0)).collect();
        let strings: BTreeMap<String, u8> = texts.iter().map(|text| (String::from(*text), 0)).collect();
        assert_eq!(to_bytes(&paths), to_bytes(&strings));

        for (base, branch) in [("a", 3), ("a:k", 1), ("a/b", 1), ("b", 0)] {
            let base = path(base);
            let after_base = paths.keys().skip_while(|path| !path.is_beneath(&base));
            let mut after_branch = after_base.clone().skip_while(|path| path.is_beneath(&base));
            assert_eq!(after_base.take_while(|path| path.is_beneath(&base)).count(), branch, "beneath `{base}`");
            assert!(after_branch.all(|path| !path.is_beneath(&base)), "beneath `{base}`");
        }
    }
}
