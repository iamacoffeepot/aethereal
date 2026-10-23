//! A file's path within a namespace (ADR-0032).

use crate::plain::write_text;
use crate::{Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Shape, Writer};
use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt;

/// The most bytes one segment of a path holds.
const MAX_SEGMENT_BYTES: usize = 255;
/// The most bytes a path holds.
const MAX_BYTES: usize = 1024;

/// Where a file is within a namespace: one or more segments joined by single `/`, each of `a`–`z`, `0`–`9`, `.`,
/// `_`, and `-`, none `.` or `..`, at most 255 bytes a segment and 1024 a path (ADR-0032).
///
/// No path climbs out of its namespace, and one names a single file on a host that ignores case and on one that does
/// not. A path that does not conform is refused where it is made and where it is read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FilePath(Cow<'static, str>);

impl FilePath {
    /// The path written as `path`, for a constant.
    ///
    /// # Panics
    ///
    /// Panics when `path` does not conform; in a constant that stops the build.
    ///
    /// ```rust
    /// use aethereal_data::FilePath;
    ///
    /// const GRASS: FilePath = FilePath::new("tiles/grass.png");
    /// ```
    ///
    /// ```compile_fail
    /// use aethereal_data::FilePath;
    ///
    /// const UP: FilePath = FilePath::new("../secrets");
    /// ```
    #[must_use]
    pub const fn new(path: &'static str) -> Self {
        assert!(
            Self::spells(path.as_bytes()),
            "a file path is segments of `a`-`z`, `0`-`9`, `.`, `_`, `-` joined by `/`, none `.` or `..`"
        );
        Self(Cow::Borrowed(path))
    }

    /// The path `text` spells, made at run time.
    ///
    /// # Errors
    ///
    /// [`DecodeError::InvalidFilePath`] when `text` does not conform.
    pub fn parse(text: impl Into<String>) -> Result<Self, DecodeError> {
        let text = text.into();
        let is_path = Self::spells(text.as_bytes());
        if !is_path {
            return Err(DecodeError::InvalidFilePath);
        }

        Ok(Self(Cow::Owned(text)))
    }

    /// Whether `bytes` spell a file's path: the one statement of the rule.
    const fn spells(bytes: &[u8]) -> bool {
        let has_text = !bytes.is_empty();
        let fits = bytes.len() <= MAX_BYTES;
        let sized = has_text && fits;
        if !sized {
            return false;
        }
        let mut start = 0;
        let mut index = 0;
        while index <= bytes.len() {
            let ends_segment = index == bytes.len() || bytes[index] == b'/';
            if ends_segment {
                let is_segment = segment_spells(bytes, start, index);
                if !is_segment {
                    return false;
                }
                start = index + 1;
            } else {
                let byte = bytes[index];
                let allowed =
                    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'_' || byte == b'-';
                if !allowed {
                    return false;
                }
            }
            index += 1;
        }

        true
    }

    /// The path's text.
    #[must_use]
    pub const fn as_str(&self) -> &str {
        match &self.0 {
            Cow::Borrowed(text) => text,
            Cow::Owned(text) => text.as_str(),
        }
    }
}

/// Whether the bytes of `bytes` from `start` to `end` are one segment: not empty, within its length, and not `.` or
/// `..`. The bytes themselves are checked as they are walked.
const fn segment_spells(bytes: &[u8], start: usize, end: usize) -> bool {
    let length = end - start;
    let has_text = length > 0;
    let fits = length <= MAX_SEGMENT_BYTES;
    let is_here = length == 1 && bytes[start] == b'.';
    let is_up = length == 2 && bytes[start] == b'.' && bytes[start + 1] == b'.';

    has_text && fits && !is_here && !is_up
}

impl fmt::Display for FilePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Schema for FilePath {
    const SHAPE: Shape = Shape::String;
}

impl<B: Boundary> Encode<B> for FilePath {
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        write_text(out.bytes(), self.as_str())
    }
}

impl<B: Boundary> Decode<B> for FilePath {
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Self::parse(String::decode(input)?)
    }
}

impl Plain for FilePath {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::samples::decode_text;
    use alloc::string::ToString;
    use alloc::vec;

    #[test]
    fn conforming_paths_decode_and_print() {
        // Catches a decoder or printer that disagrees with `spells`, and a limit one byte short.
        let longest_segment = "a".repeat(255);
        let longest = ["b".repeat(255), "c".repeat(255), "d".repeat(255), "e".repeat(254), String::from("f")].join("/");
        let longest = longest.as_str();
        let paths = ["a", "tiles/grass.png", "a/b/c", ".hidden", "a..b", "v1.2_x-y", &longest_segment, longest];
        for path in paths {
            let Ok(decoded) = decode_text::<FilePath>(path) else {
                panic!("the path `{path}` decodes");
            };
            assert_eq!(decoded.to_string(), path);
        }
    }

    #[test]
    fn nonconforming_paths_are_refused() {
        // Catches a path that climbs out of its namespace, names a root, differs only by case, or passes a limit.
        let long_segment = "a".repeat(256);
        let too_long = vec!["a".repeat(255); 5].join("/");
        let paths = [
            "",
            "/",
            "/a",
            "a/",
            "a//b",
            ".",
            "..",
            "../a",
            "a/..",
            "a/./b",
            "A",
            "a/B.png",
            "a b",
            "a\\b",
            "c:",
            "\u{e9}",
            &long_segment,
            &too_long,
        ];
        for path in paths {
            assert_eq!(decode_text::<FilePath>(path), Err(DecodeError::InvalidFilePath), "the path `{path}`");
        }
    }

    #[test]
    fn a_parsed_path_equals_a_constant_path() {
        // Catches comparisons that tell borrowed text from owned text.
        const GRASS: FilePath = FilePath::new("tiles/grass.png");

        assert_eq!(FilePath::parse("tiles/grass.png"), Ok(GRASS));
    }
}
