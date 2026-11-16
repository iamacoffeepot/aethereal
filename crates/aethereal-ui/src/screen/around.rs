//! Where each widget of a screen's tree stands as a layout lists it, handed on from its parent in tree order: where the
//! scrolls around it slid it and what they cut it to, in the screen's own coordinates, as input and popups find it;
//! where it is drawn and what it is cut to in the coordinates it is listed in; and whether a widget around it blocks
//! the pointer (ADR-0056: Lists and conditions are settings).
//!
//! What a scroll holds is listed within a shift ([`Slot::Shift`]), in the scroll's content's coordinates, where its
//! offset is not counted: the shift moves it back by the offset and cuts it to the scroll's content box, so a row that
//! stays in view is drawn where it was drawn, and only the shift moves as the scroll slides; and it is cut to all the
//! scroll may slide through, which its extent does not bound, so it is drawn where it was as the scroll's content grows.
//!
//! ```text
//! node ──> stand: laid (screen) ── less the shifts around it ──> drawing (listed) ──> its children handed on:
//!          a scroll: each slid by its offset in the screen, drawn as it was laid in its shift, cut to all it slides through
//!          else:     each where it stands
//! ```

use super::Slot;
use crate::layout::Boxed;
use crate::tree::{Flat, Segment};
use crate::widget::{Arrange, Laid, Span};
use aethereal_math::{Rect2, Vec2};

/// Where each node of a tree being listed stands, as its parent hands it on in tree order: how far the scrolls around
/// it slid it, and what they cut it to, in the screen's own coordinates; how far the coordinates it is listed in are
/// from the screen's, and what it is cut to in them; how far it is drawn from where it was laid out, which only an open
/// popup's content is; and whether a widget around it blocks the pointer.
pub(super) struct Around {
    shifts: Vec<Vec2>,
    clips: Vec<Rect2>,
    moved: Vec<Vec2>,
    cuts: Vec<Rect2>,
    lifted: Vec<Vec2>,
    covered: Vec<bool>,
}

/// Where a node stands: laid in the screen's own coordinates, as input and popups find it, and as it is drawn in the
/// coordinates it is listed in; and what it does to what is within it.
pub(super) struct Stood {
    pub laid: Laid,
    pub drawing: Laid,
    pub holding: Holding,
}

/// How far a scroll slides what it holds along its axis, in pixels, as its screen settled it at a layout, and where its
/// view stands within what it holds, as its bar shows it: none, and the whole, for any other widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Slid {
    pub offset: f32,
    pub bar: Span,
}

impl Slid {
    /// What a widget that is not a scroll slides: nothing.
    pub(super) const NONE: Self = Self { offset: 0.0, bar: Span::WHOLE };
}

/// What a node does to where what is within it stands, in the screen's own coordinates: nothing; or, as a scroll,
/// slides it by `slide` and cuts it to `cut`, its content box within what it is cut to.
#[derive(Debug, Clone, Copy)]
pub(super) enum Holding {
    Flows,
    Slides { slide: Vec2, cut: Rect2 },
}

impl Around {
    /// `count` nodes, each in the screen's own coordinates, cut to `screen`, and covered by nothing, until its parent
    /// hands it on.
    pub(super) fn new(count: usize, screen: Rect2) -> Self {
        Self {
            shifts: vec![Vec2::ZERO; count],
            clips: vec![screen; count],
            moved: vec![Vec2::ZERO; count],
            cuts: vec![screen; count],
            lifted: vec![Vec2::ZERO; count],
            covered: vec![false; count],
        }
    }

    /// Where `flat`, at `at` in tree order, laid out as `boxed` at `scale`, stands, a scroll sliding what it holds as
    /// `slid` says; its children handed on where they stand within it.
    pub(super) fn stand(&mut self, flat: &Flat<'_>, at: usize, boxed: &Boxed, (slid, scale): (Slid, f32)) -> Stood {
        let shift = self.shifts[at];
        let moved = |rect: Rect2| Rect2 { min: rect.min - shift, max: rect.max - shift };
        let clip = self.clips[at];
        let content = moved(boxed.content);
        let rect = moved(boxed.rect);
        let laid = Laid { rect, content, clip, extent: boxed.extent, bar: slid.bar, scale };
        let holding = match flat.node.widget().arrange() {
            Arrange::Scroll(axis) => {
                Holding::Slides { slide: axis.vector(slid.offset), cut: clip.intersection(content) }
            }
            Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Leaf | Arrange::Popup(_) => Holding::Flows,
        };
        let lifted = self.lifted[at];
        let drawn = |rect: Rect2| Rect2 { min: rect.min - lifted, max: rect.max - lifted };
        let drawing = Laid { rect: drawn(boxed.rect), content: drawn(boxed.content), clip: self.cuts[at], ..laid };
        let (slide, cut, region) = match holding {
            Holding::Flows => (Vec2::ZERO, clip, self.cuts[at]),
            Holding::Slides { slide, cut } => (slide, cut, scrolled(&drawing)),
        };
        for child in flat.children {
            self.shifts[*child] = shift + slide;
            self.clips[*child] = cut;
            self.moved[*child] = self.moved[at] + slide;
            self.cuts[*child] = region;
            self.lifted[*child] = lifted;
        }

        Stood { laid, drawing, holding }
    }

    /// Hands on `content`, a piece of an open popup's content, placed `shift` from where it was laid out, in the screen's
    /// top layer: drawn in the screen's own coordinates, and cut to `screen` alone.
    pub(super) fn lift(&mut self, content: usize, shift: Vec2, screen: Rect2) {
        self.shifts[content] = shift;
        self.clips[content] = screen;
        self.moved[content] = Vec2::ZERO;
        self.cuts[content] = screen;
        self.lifted[content] = shift;
    }

    /// The shift what the scroll at `at` holds is listed within, the scroll sliding it by `slide` and cutting it to
    /// `cut`, in the screen's own coordinates: moved back by its offset, and cut to its content box, in the coordinates
    /// the scroll is listed in.
    pub(super) fn shift(&self, at: usize, slide: Vec2, cut: Rect2) -> Slot {
        let moved = self.moved[at];

        Slot::Shift { origin: -slide, clip: Rect2 { min: cut.min + moved, max: cut.max + moved } }
    }

    /// Whether a widget around the node at `at` blocks the pointer.
    pub(super) fn covered(&self, at: usize) -> bool {
        self.covered[at]
    }

    /// Hands on to the children of `node`, at `at` among `flat`, whether a widget around them blocks the pointer: one
    /// around `node` does, or `node` does, as it `blocks`; but for the content of a popup `open`, which stands in the top
    /// layer, out of what blocks around its anchor.
    pub(super) fn cover(&mut self, (node, at): (&Flat<'_>, usize), flat: &[Flat<'_>], blocks: bool, open: bool) {
        let covers = self.covered[at] || blocks;
        for child in node.children {
            let held = flat[*child].path.segments().last() == Some(&Segment::Place(0));
            let lifted = open && !held;
            self.covered[*child] = covers && !lifted;
        }
    }
}

/// What is within a scroll drawn at `drawing` is cut to, in its content's coordinates, where its offset is not counted:
/// all it may slide through, as far each way from its content box's top left as its base may move its window, and as
/// a screen could hold past that. The shift its content is listed in cuts its children to its content box, so this
/// cuts nothing that shows; it reads neither its extent nor where it stands, so a row stays drawn as it was while what
/// the scroll holds grows past it, as a log or a transcript does with each line added, and as the window moves past
/// other rows.
fn scrolled(drawing: &Laid) -> Rect2 {
    Rect2 { min: drawing.content.min - Vec2::splat(UNCUT), max: drawing.content.min + Vec2::splat(UNCUT) }
}

/// How far what a scroll holds may reach each way from its content box's top left, in pixels: past any window's base
/// and any screen, and a whole number an `f32` holds exactly.
const UNCUT: f32 = 16_777_216.0;
