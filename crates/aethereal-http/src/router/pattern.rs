//! A route's pattern: `/` and then segments, each a literal or a `{name}` capture covering the whole segment.

use super::Unroutable;
use super::walk::Step;

/// A pattern, read into its segments.
#[derive(Debug)]
pub struct Pattern<'a> {
    segments: Vec<Segment<'a>>,
}

/// One segment of a pattern.
#[derive(Debug, Clone, Copy)]
pub enum Segment<'a> {
    /// The one path segment that is this text, as sent.
    Literal(&'a str),
    /// Any one path segment that is not empty, captured under this name.
    Capture(&'a str),
}

impl<'a> Pattern<'a> {
    /// The segments of `pattern`, refused `Malformed` when it does not start with `/`, a capture is not a whole
    /// segment, or a capture's name is empty or repeated.
    pub fn read(pattern: &'a str) -> Result<Self, Unroutable> {
        let malformed = || Unroutable::Malformed { pattern: pattern.to_owned() };
        let segments: Vec<Segment<'a>> = pattern
            .strip_prefix('/')
            .ok_or_else(malformed)?
            .split('/')
            .map(Segment::read)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(malformed)?;

        let read = Self { segments };
        if read.repeats_a_name() {
            return Err(malformed());
        }

        Ok(read)
    }

    pub fn segments(&self) -> &[Segment<'a>] {
        &self.segments
    }

    /// Whether the pattern's captures are `names`, in their order.
    pub fn captures_are(&self, names: &[&str]) -> bool {
        self.names().eq(names.iter().copied())
    }

    /// Where each capture is among the pattern's segments, in order.
    pub fn capture_positions(&self) -> Vec<usize> {
        self.segments.iter().enumerate().filter(|(_, segment)| segment.is_capture()).map(|(at, _)| at).collect()
    }

    fn names(&self) -> impl Iterator<Item = &'a str> {
        self.segments.iter().filter_map(Segment::name)
    }

    fn repeats_a_name(&self) -> bool {
        self.names().enumerate().any(|(at, name)| self.names().take(at).any(|before| before == name))
    }
}

impl<'a> Segment<'a> {
    /// The segment `text` is: none when braces in it are not one whole capture with a name.
    fn read(text: &'a str) -> Option<Self> {
        let braced = text.strip_prefix('{').and_then(|inner| inner.strip_suffix('}'));
        match braced {
            Some(name) if is_name(name) => Some(Self::Capture(name)),
            None if is_plain(text) => Some(Self::Literal(text)),
            Some(_) | None => None,
        }
    }

    /// What a walk takes this segment as: its text, or any segment that is not empty.
    pub const fn step(&self) -> Step<'a> {
        match *self {
            Self::Literal(text) => Step::Text(text),
            Self::Capture(_) => Step::Any,
        }
    }

    const fn is_capture(&self) -> bool {
        matches!(self, Self::Capture(_))
    }

    const fn name(&self) -> Option<&'a str> {
        match *self {
            Self::Literal(_) => None,
            Self::Capture(name) => Some(name),
        }
    }
}

/// Whether `text` holds no brace, as a literal segment's text does.
fn is_plain(text: &str) -> bool {
    !text.contains(['{', '}'])
}

/// Whether `name` names a capture: it is not empty, and holds no brace.
fn is_name(name: &str) -> bool {
    !name.is_empty() && is_plain(name)
}
