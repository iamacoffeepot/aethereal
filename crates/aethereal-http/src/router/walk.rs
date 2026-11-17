//! What a walk down the trie reads, a segment at a time: a request's path, or a new route's pattern.

use super::pattern::Segment;

/// What one step of a walk takes: a path segment, as sent, or any segment that is not empty, as a capture does.
#[derive(Debug, Clone, Copy)]
pub enum Step<'a> {
    Text(&'a str),
    Any,
}

/// What a cursor reads next.
pub enum Next<'a, C> {
    /// One more segment, and the cursor past it.
    Segment(Step<'a>, C),
    /// Nothing: the walk ends here.
    End,
}

/// Where a walk is in what it reads.
pub trait Cursor: Copy {
    /// The segment at the cursor, and the cursor past it; or the end.
    fn next(&self) -> Next<'_, Self>;
}

/// What is left of a path to walk, after the `/` that starts each segment.
#[derive(Debug, Clone, Copy)]
pub enum Rest<'p> {
    /// The segments not yet walked: at least one, which may be empty.
    Segments(&'p str),
    /// Every segment has been walked.
    Done,
}

impl Cursor for Rest<'_> {
    fn next(&self) -> Next<'_, Self> {
        match *self {
            Self::Segments(segments) => {
                let next = segments.split_once('/');
                match next {
                    Some((segment, after)) => Next::Segment(Step::Text(segment), Self::Segments(after)),
                    None => Next::Segment(Step::Text(segments), Self::Done),
                }
            }
            Self::Done => Next::End,
        }
    }
}

impl Cursor for &[Segment<'_>] {
    fn next(&self) -> Next<'_, Self> {
        let segments: Self = *self;
        match segments.split_first() {
            Some((segment, after)) => Next::Segment(segment.step(), after),
            None => Next::End,
        }
    }
}
