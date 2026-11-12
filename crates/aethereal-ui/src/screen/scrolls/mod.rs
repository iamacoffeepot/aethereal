//! A screen's scrolls (ADR-0056: Lists and conditions are settings; Focus and keys): which items of a list in a scroll
//! the view builds, the wheel, and focus moved by Tab brought into view.
//!
//! A list in a scroll builds only the items it shows. The view asks [`Screen::rows`] for the window of the list at a key
//! path: the lines its scroll's content box shows at its offset as the table keeps it, by the pitch its last layout
//! measured, and a margin of [`MARGIN`] lines each way, so Tab finds the next item built and a turn of the wheel shows
//! no gap. The focused item is built wherever it stands, so it keeps its focus and its state while it is scrolled out
//! of view. A scroll never laid out builds its first line, and the next `Prepare` builds the window its pitch shows.
//!
//! ```text
//! view ──> Screen::rows(list, count) ──> Rows: the window, by the offset now and the last layout's pitch
//!      ──> each item: builds? ──> push(at, node) ──> finish ──> Scroll { lines: Window { count, at } } over the rows
//! ```
//!
//! The window is worked out from the table as it stands, so a turn of the wheel builds the items it shows in the very
//! next view; and the tree holds only what was built, by value, so an unchanged list at an unchanged offset is the same
//! tree, and is skipped.

#[cfg(test)]
mod tests;

use super::{Fresh, Last, Screen, Worked};
use crate::input::Used;
use crate::table::Focus;
use crate::tree::{ItemKey, KeyPath, Node};
use crate::widget::{Arrange, Axis, Lines};
use aethereal_math::{Rect2, Vec2};
use core::iter;

/// How many lines past those it shows a list in a scroll builds each way.
pub const MARGIN: u32 = 2;

/// What a list in a scroll builds in this view: the lines of its window, and the item that has its screen's focus
/// wherever it stands; then each item built, beside its place in the list.
#[derive(Debug)]
pub struct Rows {
    count: u32,
    first: u32,
    end: u32,
    focused: Focused,
    at: Vec<u32>,
    built: Vec<Node>,
}

/// Whether an item of a list, by its key, has its screen's focus or holds the widget that has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focused {
    Elsewhere,
    On(ItemKey),
}

impl Rows {
    /// Whether the item at `at` in the list, keyed `key`, is built: it is in the window, or it has the focus.
    #[must_use]
    pub fn builds(&self, at: usize, key: ItemKey) -> bool {
        let at = place(at);
        let shown = at >= self.first && at < self.end;
        let focused = self.focused == Focused::On(key);

        shown || focused
    }

    /// Builds `row`, the node of the item at `at` in the list, after those built before it.
    pub fn push(&mut self, at: usize, row: Node) {
        self.at.push(place(at));
        self.built.push(row);
    }

    /// The scroll's lines, the list's count and each built item's place, and the rows built, in order.
    #[must_use]
    pub fn finish(self) -> (Lines, Vec<Node>) {
        (Lines::Window { count: self.count, at: self.at }, self.built)
    }
}

/// A place in a list, as a scroll's lines keep it: a list past four billion items keeps its last at the most it counts.
fn place(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}

/// A count of lines, as a length is counted.
fn length(lines: u32) -> f32 {
    // A list's lines are far fewer than an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let length = lines as f32;

    length
}

/// The line `at`, counted from the list's first, within a list of `count`: at least none and at most `count`.
fn line(at: f32, count: u32) -> u32 {
    let within = at.clamp(0.0, length(count));
    // Clamped to the list's lines, which a u32 counts.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let line = within as u32;

    line
}

impl Screen {
    /// What the list of `count` items in the scroll at `list` builds in this view: the lines its content box shows at
    /// its offset, by the pitch its last layout measured, with [`MARGIN`] lines each way; its first line before it was
    /// ever laid out; and, wherever it stands, the item that has the screen's focus or holds the widget that has.
    #[must_use]
    pub fn rows(&self, list: &KeyPath, count: usize) -> Rows {
        let count = place(count);
        let focused = match self.table.focus() {
            Focus::On(path) => path.item_of(list).map_or(Focused::Elsewhere, Focused::On),
            Focus::Nowhere => Focused::Elsewhere,
        };
        let (first, end) = self.window(list, count);

        Rows { count, first, end, focused, at: Vec::new(), built: Vec::new() }
    }

    /// The lines of the list of `count` items in the scroll at `list` its window holds: from the first it shows less
    /// the margin to past the last it shows and the margin, or its first line before it was laid out.
    fn window(&self, list: &KeyPath, count: u32) -> (u32, u32) {
        let unlaid = (0, count.min(1));
        let Worked { last: Last::Built(tree), placed, pitches, .. } = &*self.unkept else {
            return unlaid;
        };
        let pitch = pitches.iter().find(|(path, _)| path == list).map(|(_, pitch)| *pitch);
        let laid = placed.iter().find(|(path, _)| path == list).map(|(_, laid)| *laid);
        let widget = tree.find(list).map(Node::widget);
        let (Some(pitch), Some(laid), Some(widget)) = (pitch, laid, widget) else {
            return unlaid;
        };
        let Arrange::Scroll(axis) = widget.arrange() else {
            return unlaid;
        };
        let state = self.table.bytes(list).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
        let stride = pitch.stride.max(1.0);
        let view = axis.along(laid.content.size());
        let most = stride.mul_add(length(count), pitch.lead - view).max(0.0);
        let offset = widget.offset(&state).clamp(0.0, most);
        let first = line(((offset - pitch.lead) / stride).floor(), count).saturating_sub(MARGIN);
        let end = line(((offset + view - pitch.lead) / stride).ceil(), count).saturating_add(MARGIN).min(count);

        (first, end)
    }

    /// Slides the scroll at `path` by `by` pixels along its axis, within what it can scroll, as its last layout
    /// measured it: how far it moved, none when it is not a scroll or is at its end that way. A move lays the screen
    /// out at the next `Prepare`.
    fn slide(&mut self, path: &KeyPath, by: f32) -> f32 {
        let Worked { last: Last::Built(tree), placed, .. } = &*self.unkept else {
            return 0.0;
        };
        let laid = placed.iter().find(|(placed, _)| placed == path).map(|(_, laid)| *laid);
        let (Some(node), Some(laid)) = (tree.find(path), laid) else {
            return 0.0;
        };
        let widget = node.widget();
        let Arrange::Scroll(axis) = widget.arrange() else {
            return 0.0;
        };
        let mut state = self.table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
        let offset = widget.offset(&state);
        let to = (offset + by).clamp(0.0, laid.travel(axis));
        let moved = to - offset;
        let still = moved == 0.0;
        if still {
            return 0.0;
        }
        widget.scroll_to(&mut state, to);
        self.table.write(path, widget.state_kind(), state);
        self.fresh = Fresh::Stale;

        moved
    }

    /// The axis of the scroll at `path` in the last tree built, if a scroll stands there.
    fn scrolls(&self, path: &KeyPath) -> Option<Axis> {
        let Last::Built(tree) = &self.unkept.last else {
            return None;
        };
        match tree.find(path).map(|node| node.widget().arrange()) {
            Some(Arrange::Scroll(axis)) => Some(axis),
            _ => None,
        }
    }

    /// Turns the wheel by `delta` over the widget at `path`: the innermost scroll at or around it that can still move
    /// that way along its axis moves, and one at its end lets it pass outward.
    pub(super) fn wheel(&mut self, path: &KeyPath, delta: Vec2) -> Used {
        let around: Vec<KeyPath> = iter::once(path.clone()).chain(path.ancestors()).collect();
        for at in around {
            let Some(axis) = self.scrolls(&at) else {
                continue;
            };
            let moved = self.slide(&at, axis.along(delta));
            let took = moved != 0.0;
            if took {
                return Used::Used;
            }
        }

        Used::Passed
    }

    /// Brings the widget at `path` into view: each scroll around it, innermost first, slides as little as it can to
    /// show the widget's box within its content box, as far as it can scroll.
    pub(super) fn reveal(&mut self, path: &KeyPath) {
        let found = self.unkept.placed.iter().find(|(placed, _)| placed == path).map(|(_, laid)| laid.rect);
        let Some(mut rect) = found else {
            return;
        };
        for at in path.ancestors() {
            let Some(axis) = self.scrolls(&at) else {
                continue;
            };
            let viewed = self.unkept.placed.iter().find(|(placed, _)| *placed == at).map(|(_, laid)| laid.content);
            let Some(view) = viewed else {
                continue;
            };
            let by = into_view(rect, view, axis);
            let moved = self.slide(&at, by);
            rect = Rect2 { min: rect.min - axis.vector(moved), max: rect.max - axis.vector(moved) };
        }
    }
}

/// How far a scroll along `axis` whose content box is `view` slides to show `rect` within it: none when it shows
/// already; back to its start when it is before, or too long to show whole; on to its end when it is past.
fn into_view(rect: Rect2, view: Rect2, axis: Axis) -> f32 {
    let before = axis.along(rect.min) - axis.along(view.min);
    let past = axis.along(rect.max) - axis.along(view.max);
    let early = before < 0.0;
    let late = past > 0.0;
    match (early, late) {
        (true, _) => before,
        (false, true) => past.min(before),
        (false, false) => 0.0,
    }
}
