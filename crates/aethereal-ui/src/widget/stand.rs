//! Where a scroll stands among what it holds (ADR-0056: Lists and conditions are settings; Stock widgets): at an
//! item, by its key, some pixels into it; at its end; or at a place a jump named, which the next layout turns into an
//! item. No item's length is kept: the library works each out from the items it built, as it lays them out.
//!
//! ```text
//! Unplaced ──> first layout ──> End (kept to its end) | At(first item)
//! At(anchor) ── wheel, keys ──> into moves ── layout: past its item? ──> At(the item the view's start is in)
//! Item(n) | Share(s) ── layout ──> At(the item they name)
//! End ── layout ──> End kept to its end, else At(the item its view's start is in)
//! ```

use crate::tree::Segment;
use aethereal_data::Plain;

/// Where a scroll keeps to as what it holds grows or shrinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Keeps {
    /// The item at its view's start, by its key, so what it shows stays put as items are added or removed around it.
    #[default]
    Offset,
    /// Its end, while it stands there: what is added past its end slides into view, as a console's scrollback or a
    /// chat's does; slid back from its end, it keeps its item, and slid to its end again, keeps to it. One never laid
    /// out starts at its end.
    End,
}

/// Where a scroll stands among what it holds: its state's, which the library reads and settles as it lays it out.
#[derive(Debug, Clone, Copy, PartialEq, Default, Plain)]
pub enum Stand {
    /// It was never laid out: its first layout puts it at its end, kept to its end, or at its first item.
    #[default]
    Unplaced,
    /// At an item, as `anchor` says.
    At(Anchor),
    /// At the start of the item at this place in what it holds, whatever its key: where a jump lands, kept by key from
    /// the next layout.
    Item(u32),
    /// With this share of the whole it holds before its view's start, from 0 to 1, as its bar was dragged: a pixel
    /// share while it builds every item, and an item share while it builds only those it shows.
    Share(f32),
    /// At its end, the view's start at `start` as it was last laid out, from where Page Up slides it back.
    End { start: Anchor },
}

/// The item a scroll's view starts in: its place in what the scroll holds, its key, and how many pixels into it the
/// view starts, past its end or before its start while a slide goes past the items built, until the next layout.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
pub struct Anchor {
    pub item: u32,
    pub key: Segment,
    pub into: f32,
}

impl Anchor {
    /// The start of what a scroll holds: its first child, as its first layout finds it.
    pub const FIRST: Self = Self { item: 0, key: Segment::Place(0), into: 0.0 };

    /// It, `by` pixels further on.
    #[must_use]
    pub fn moved(self, by: f32) -> Self {
        Self { into: self.into + by, ..self }
    }
}

impl Stand {
    /// It slid `by` pixels on, as a page key slides it, from where its last layout settled it: from its item, or back
    /// from its end. A jump not yet laid out stands where it was.
    #[must_use]
    pub fn slid(self, by: f32) -> Self {
        match self {
            Self::At(anchor) => Self::At(anchor.moved(by)),
            Self::End { start } if by < 0.0 => Self::At(start.moved(by)),
            Self::End { .. } | Self::Unplaced | Self::Item(_) | Self::Share(_) => self,
        }
    }

    /// Its end, the view's start where it stands now.
    #[must_use]
    pub const fn ended(self) -> Self {
        match self {
            Self::At(start) | Self::End { start } => Self::End { start },
            Self::Unplaced | Self::Item(_) | Self::Share(_) => Self::End { start: Anchor::FIRST },
        }
    }
}

/// Where a scroll's view stands within what it holds, as shares of the whole along its axis, from 0 to 1: where it
/// starts, and how much it shows. By pixels while it built every item; by items while it built only those it shows,
/// its start the item its view starts in and its length the items it shows, each of the list's count.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: f32,
    pub shown: f32,
}

impl Span {
    /// The whole of what it holds shown: what every widget that is not a scroll is laid with.
    pub const WHOLE: Self = Self { start: 0.0, shown: 1.0 };
}

/// Where [`Screen::jump`](crate::Screen::jump) sends a scroll: to its end, or to the start of the item at a place in
/// what it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jump {
    End,
    Item(u32),
}
