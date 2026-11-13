//! Where each widget of a screen's tree stands as a layout lists it, handed on from its parent in tree order: where the
//! scrolls around it slid it and what they cut it to, in the screen's own coordinates, as input and popups find it;
//! where it is drawn and what it is cut to in the coordinates it is listed in; and whether a widget around it blocks
//! the pointer (ADR-0056: Lists and conditions are settings; *settled by the library's draw-cost stage*).
//!
//! What a scroll holds is listed within a shift ([`Slot::Shift`]), in the scroll's content's coordinates, where its
//! offset is not counted: the shift moves it back by the offset and cuts it to the scroll's content box, so a row that
//! stays in view is drawn where it was drawn, and only the shift moves as the scroll slides.
//!
//! ```text
//! node ──> stand: laid (screen) ── less the shifts around it ──> drawing (listed) ──> its children handed on:
//!          a scroll: each slid by its offset in the screen, drawn as it was laid in its shift, cut to all it slides through
//!          else:     each where it stands
//! ```

use super::Slot;
use crate::layout::Boxed;
use crate::table::Table;
use crate::tree::{Flat, KeyPath, Segment};
use crate::widget::{Arrange, Axis, Built, Laid};
use aethereal_math::{Rect2, Vec2};

/// Where each node of a tree being listed stands, as its parent hands it on in tree order: how far the scrolls around
/// it slid it, and what they cut it to, in the screen's own coordinates; how far the coordinates it is listed in are
/// from the screen's, and what it is cut to in them; and whether a widget around it blocks the pointer.
pub(super) struct Around {
    shifts: Vec<Vec2>,
    clips: Vec<Rect2>,
    moved: Vec<Vec2>,
    cuts: Vec<Rect2>,
    covered: Vec<bool>,
}

/// Where a node stands: laid in the screen's own coordinates, as input and popups find it, and as it is drawn in the
/// coordinates it is listed in; and what it does to what is within it.
pub(super) struct Stood {
    pub laid: Laid,
    pub drawing: Laid,
    pub holding: Holding,
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
            covered: vec![false; count],
        }
    }

    /// Where `flat`, at `at` in tree order, laid out as `boxed` at `scale`, stands, a scroll's offset settled and
    /// clamped in `table`; its children handed on where they stand within it.
    pub(super) fn stand(&mut self, table: &mut Table, flat: &Flat<'_>, at: usize, boxed: &Boxed, scale: f32) -> Stood {
        let shift = self.shifts[at];
        let moved = |rect: Rect2| Rect2 { min: rect.min - shift, max: rect.max - shift };
        let clip = self.clips[at];
        let laid = Laid { rect: moved(boxed.rect), content: moved(boxed.content), clip, extent: boxed.extent, scale };
        let widget = flat.node.widget();
        let holding = match widget.arrange() {
            Arrange::Scroll(axis) => {
                let slide = fit(table, &flat.path, widget, &laid, axis);
                Holding::Slides { slide, cut: clip.intersection(laid.content) }
            }
            Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Leaf | Arrange::Popup(_) => Holding::Flows,
        };
        let drawing = Laid { clip: self.cuts[at], ..laid.relative_to(-self.moved[at]) };
        let (slide, cut, region) = match holding {
            Holding::Flows => (Vec2::ZERO, clip, self.cuts[at]),
            Holding::Slides { slide, cut } => (slide, cut, scrolled(&drawing)),
        };
        for child in &flat.children {
            self.shifts[*child] = shift + slide;
            self.clips[*child] = cut;
            self.moved[*child] = self.moved[at] + slide;
            self.cuts[*child] = region;
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
        for child in &node.children {
            let held = flat[*child].path.segments().last() == Some(&Segment::Place(0));
            let lifted = open && !held;
            self.covered[*child] = covers && !lifted;
        }
    }
}

/// What is within a scroll drawn at `drawing` is cut to, in its content's coordinates, where its offset is not counted:
/// all it slides through, from its content box's top left as far as its children reach, or its content box where they
/// reach less far. Its offset within what it can slide shows its content box within this, so its children are cut to
/// its content box alone, by the shift its content is listed in.
fn scrolled(drawing: &Laid) -> Rect2 {
    let content = drawing.content;
    let reach = drawing.extent.max(content.size());

    Rect2::from_origin_size(content.min, reach)
}

/// Settles `widget`, a scroll along `axis` at `path` laid at `laid`, at what it can now scroll, and clamps its offset to
/// it, keeping what changed of its state in `table`, and answers how far it slides what is within it.
fn fit(table: &mut Table, path: &KeyPath, widget: &dyn Built, laid: &Laid, axis: Axis) -> Vec2 {
    let kept = table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
    let mut state = kept.clone();
    let travel = laid.travel(axis);
    widget.settle(&mut state, travel);
    let offset = widget.offset(&state);
    let fitted = offset.clamp(0.0, travel);
    let clamped = fitted.to_bits() != offset.to_bits();
    if clamped {
        widget.scroll_to(&mut state, fitted);
    }
    let changed = state != kept;
    if changed {
        table.write(path, widget.state_kind(), state);
    }

    axis.vector(fitted)
}
