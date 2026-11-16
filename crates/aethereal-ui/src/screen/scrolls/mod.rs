//! A screen's scrolls (ADR-0056: Lists and conditions are settings; Focus and keys): which items of a list in a scroll
//! the view builds, where each scroll stands among what it holds, the wheel, and focus moved by Tab brought into view.
//!
//! A scroll stands at an item, by its key, some pixels into it, or at its end ([`Stand`]): no item's length is kept,
//! summed, or guessed. A list in a scroll whose type windows its items ([`Widget::WINDOWS`](crate::Widget::WINDOWS)),
//! as `Scroll` does, builds only the items that can show. No item is shorter than the least length its last layout
//! found ([`found`]), so a view `view` pixels long shows at most `ceil(view / least) + 1` of them, whatever their
//! lengths. The view asks [`Screen::rows`] for the window of the list at a key path: from the item the scroll stands
//! at, found by its key near where it was, that many items on, and [`MARGIN`] more each way, so Tab finds the next
//! item built and a turn of the wheel shows no gap; or the last that many, standing at its end. The focused item is
//! built wherever it stands, so it keeps its focus and its state while it is scrolled out of view, and so are the items
//! the screen's selection's two ends stand in, so tree order places them; a view built to copy the selection builds
//! every item between them (ADR-0056: Copy and paste). A scroll never laid out builds the item it stands at, and the
//! next view builds the window its layout bounds.
//!
//! ```text
//! view ──> Screen::rows(list, scroll, count, key) ──> Rows: the window, from where it stands and its last layout
//!      ──> each item at Rows::places: push(at, node) ──> finish ──> Scroll { lines: Window { count, at } }
//! layout ──> the items built, one after another ──> Window: each one's place, key, start and length
//!        ──> the scroll settled: the item its view's start is in, and how far its children slide
//! ```
//!
//! A slide past the items built, as a fling of the wheel, goes as far as they reach in one pass, and keeps the rest in
//! how far into its item the scroll stands: the next view builds the window from there, and its layout settles it on.
//! The tree holds only what was built, by value, so an unchanged list at an unchanged stand is the same tree, and is
//! skipped.

mod found;
#[cfg(test)]
mod tests;
mod window;

pub(super) use found::windows;
pub(super) use window::{Window, Windowing};

use super::selection::Building;
use super::{Fresh, Last, Screen, Worked};
use crate::input::Used;
use crate::table::{Focus, Made, Mark, Selection};
use crate::tree::{Element, ItemKey, KeyPath, Node, Segment};
use crate::widget::{Arrange, Axis, Jump, Keeps, Lines, Stand};
use aethereal_math::{Rect2, Vec2};
use core::iter;
use window::items;

/// How many items past those that can show a list in a scroll builds each way.
pub const MARGIN: u32 = 2;

/// What a list in a scroll builds in this view: the places of the items in its window, and of the item that has its
/// screen's focus wherever it stands, in order; then each item built, beside its place in the list.
#[derive(Debug)]
pub struct Rows {
    count: u32,
    places: Vec<u32>,
    at: Vec<u32>,
    built: Vec<Node>,
}

impl Rows {
    /// The places in the list of the items to build, in order.
    #[must_use]
    pub fn places(&self) -> Vec<usize> {
        self.places.iter().map(|place| usize::try_from(*place).unwrap_or(usize::MAX)).collect()
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

/// Where a list's window is built from: on from the item at `item`, its view starting `into` pixels into it; or back
/// from the list's end.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Origin {
    Item { item: u32, into: f32 },
    End,
}

/// How many items a view `view` pixels long shows at most when none is shorter than `least` pixels: `ceil(view /
/// least) + 1`.
fn shown_most(view: f32, least: f32) -> u32 {
    let most = (view / least).ceil() + 1.0;
    // Bounded by the list's count where it is used, which a u32 counts.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let most = most.clamp(1.0, items(u32::MAX)) as u32;

    most
}

/// How many items a slide of `pixels` reaches past, when none is shorter than `least` pixels.
fn reached(pixels: f32, least: f32) -> u32 {
    let positive = pixels > 0.0;
    if !positive {
        return 0;
    }

    shown_most(pixels, least) - 1
}

/// The places a window of a list of `count` builds, from `from`, a view `view` pixels long, no item shorter than
/// `least`: from the item it stands at, the items that can show of the slide past it, and [`MARGIN`] more each way;
/// at most `ceil(view / least) + 1 + 2 * MARGIN` in all, a slide past that settling over passes.
fn bounded(from: Origin, count: u32, view: f32, least: f32) -> (u32, u32) {
    let shown = shown_most(view, least);
    let most = shown.saturating_add(2 * MARGIN);
    match from {
        Origin::End => (count.saturating_sub(shown + MARGIN), count),
        Origin::Item { item, into } => {
            let back = reached(-into, least).saturating_add(MARGIN).min(shown + MARGIN);
            let on = shown_most(into.max(0.0) + view, least).saturating_add(MARGIN).min(most - back).max(1);
            let end = item.saturating_add(on).min(count);
            let short = item.saturating_add(on) - end;
            let first = item.saturating_sub(back.saturating_add(short));

            (first, end)
        }
    }
}

impl Screen {
    /// What the list of `count` items, each keyed as `key` says by its place, in `scroll`, the scroll at `list`, builds
    /// in this view: the items that can show from where it stands, by its last layout, with [`MARGIN`] more each way;
    /// the item it stands at before it was ever laid out; every item while it fails a condition of windowing; wherever
    /// they stand, the item that has the screen's focus or holds the widget that has, and those the screen's selection's
    /// ends stand in; and, as the view is built to copy the selection, every item between its ends.
    #[must_use]
    pub fn rows<E: Element>(&self, list: &KeyPath, scroll: &E, count: usize, key: impl Fn(usize) -> ItemKey) -> Rows {
        let count = place(count);
        let keyed = |at: u32| Segment::Item(key(usize::try_from(at).unwrap_or(usize::MAX)));
        let found = self.found(list);
        let from = match self.stood(list) {
            Stand::At(anchor) => {
                let near = locate(anchor.key, anchor.item, count, &keyed);
                Origin::Item { item: near, into: anchor.into }
            }
            Stand::Item(item) => Origin::Item { item: item.min(count.saturating_sub(1)), into: 0.0 },
            Stand::Share(share) => {
                // A share of a list's count, which a u32 counts.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let item = (share.clamp(0.0, 1.0) * items(count)).floor() as u32;
                Origin::Item { item: item.min(count.saturating_sub(1)), into: 0.0 }
            }
            Stand::End { .. } => Origin::End,
            Stand::Unplaced => match scroll.keeps() {
                Keeps::End => Origin::End,
                Keeps::Offset => Origin::Item { item: 0, into: 0.0 },
            },
        };
        let windowing = found.map_or(Windowing::Unmeasured, Window::windowing);
        let (first, end) = match windowing {
            Windowing::Bounded { least } => {
                let view = found.map_or(0.0, Window::view);
                bounded(from, count, view, least)
            }
            Windowing::Unmeasured => match from {
                Origin::End => (count.saturating_sub(1), count),
                Origin::Item { item, .. } => (item, item.saturating_add(1).min(count)),
            },
            Windowing::Whole | Windowing::Wraps | Windowing::Every(_) => (0, count),
        };
        let mut places: Vec<u32> = (first..end).collect();
        let focused = match self.table.focus() {
            Focus::On(path) => path.item_of(list),
            Focus::Nowhere => None,
        };
        let marked: Vec<ItemKey> = match self.table.selection() {
            Selection::Made(made) => made.marked().filter_map(|path| path.item_of(list)).collect(),
            Selection::Nothing => Vec::new(),
        };
        for key in focused.into_iter().chain(marked) {
            if let Some(at) = held(key, found, count, &keyed) {
                let before = places.partition_point(|place| *place < at);
                let built = places.get(before) == Some(&at);
                if !built {
                    places.insert(before, at);
                }
            }
        }
        if let (Building::Selection, Selection::Made(made)) = (self.building, self.table.selection()) {
            let between = between(made, list, (found, count), &keyed);
            places.extend(between);
            places.sort_unstable();
            places.dedup();
        }

        Rows { count, places, at: Vec::new(), built: Vec::new() }
    }

    /// What the last layout found of the scroll at `list`, if it laid one out there.
    fn found(&self, list: &KeyPath) -> Option<&Window> {
        self.unkept.windows.iter().find(|window| window.path() == list)
    }

    /// Where the scroll at `path` in the last tree built stands, as its table keeps it: unplaced before it was built.
    fn stood(&self, path: &KeyPath) -> Stand {
        let Last::Built(tree) = &self.unkept.last else {
            return Stand::Unplaced;
        };
        let Some(node) = tree.find(path) else {
            return Stand::Unplaced;
        };
        let widget = node.widget();
        let state = self.table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);

        widget.stand(&state)
    }

    /// Sends the scroll at `path` to `to`: its end, or the start of the item at a place in what it holds, which the
    /// next view builds and its layout settles it at. Nothing when no scroll stands there.
    pub fn jump(&mut self, path: &KeyPath, to: Jump) {
        let Last::Built(tree) = &self.unkept.last else {
            return;
        };
        let Some(node) = tree.find(path) else {
            return;
        };
        let widget = node.widget();
        let Arrange::Scroll(_) = widget.arrange() else {
            return;
        };
        let mut state = self.table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
        let stand = match to {
            Jump::End => widget.stand(&state).ended(),
            Jump::Item(item) => Stand::Item(item),
        };
        widget.stand_at(&mut state, stand);
        self.table.write(path, widget.state_kind(), state);
        self.fresh = Fresh::Stale;
    }

    /// Slides the scroll at `path` by `by` pixels along its axis, from where its last layout settled it, within what it
    /// holds: how far it moved, none when it is not a scroll or is at its end that way. A slide past the items it built
    /// goes on from them at the next view. A move lays the screen out at the next `Prepare`.
    pub(super) fn slide(&mut self, path: &KeyPath, by: f32) -> f32 {
        let Worked { last: Last::Built(tree), windows, .. } = &*self.unkept else {
            return 0.0;
        };
        let found = windows.iter().find(|window| window.path() == path);
        let (Some(node), Some(window)) = (tree.find(path), found) else {
            return 0.0;
        };
        let widget = node.widget();
        let mut state = self.table.bytes(path).map_or_else(|| widget.first_state(), <[u8]>::to_vec);
        let (stand, moved) = window.slide(widget.stand(&state), widget.keeps(), by);
        let still = moved == 0.0;
        if still {
            return 0.0;
        }
        widget.stand_at(&mut state, stand);
        self.table.write(path, widget.state_kind(), state);
        self.fresh = Fresh::Stale;

        moved
    }

    /// The axis of the scroll at `path` in the last tree built, if a scroll stands there.
    pub(super) fn scrolls(&self, path: &KeyPath) -> Option<Axis> {
        let Last::Built(tree) = &self.unkept.last else {
            return None;
        };
        match tree.find(path).map(|node| node.widget().arrange()) {
            Some(Arrange::Scroll(axis)) => Some(axis),
            _ => None,
        }
    }

    /// Turns the wheel by `delta` over the widget at `path`: the innermost scroll at or around it that can still move
    /// that way along its axis moves, and one at its end lets it pass outward, as far as the popup whose content it is
    /// in: a popup's content does not slide the scroll its anchor stands in.
    pub(super) fn wheel(&mut self, path: &KeyPath, delta: Vec2) -> Used {
        let around: Vec<KeyPath> = iter::once(path.clone()).chain(self.surfaced(path)).collect();
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

    /// Brings the widget at `path` into view: each scroll around it, innermost first and up to the popup whose content
    /// it is in, slides as little as it can to show the widget's box within its content box, as far as it can scroll.
    pub(super) fn reveal(&mut self, path: &KeyPath) {
        let Some(mut rect) = self.laid(path).map(|laid| laid.rect) else {
            return;
        };
        for at in self.surfaced(path) {
            let Some(axis) = self.scrolls(&at) else {
                continue;
            };
            let Some(view) = self.laid(&at).map(|laid| laid.content) else {
                continue;
            };
            let by = into_view(rect, view, axis);
            let moved = self.slide(&at, by);
            rect = Rect2 { min: rect.min - axis.vector(moved), max: rect.max - axis.vector(moved) };
        }
    }
}

/// The places of every item of the list at `list`, of `count`, each keyed as `keyed` says, between the ends of
/// `made`, when it runs within the scroll there: from the first item, or the one its start stands in, to the last,
/// or the one its end stands in. None when it runs within another.
fn between(
    made: &Made,
    list: &KeyPath,
    (found, count): (Option<&Window>, u32),
    keyed: &impl Fn(u32) -> Segment,
) -> Vec<u32> {
    let ours = made.scope == *list;
    let empty = count == 0;
    if !ours || empty {
        return Vec::new();
    }
    let place = |mark: &Mark, missing: u32| match mark {
        Mark::Start => 0,
        Mark::End => count - 1,
        Mark::At { path, .. } => path.item_of(list).and_then(|key| held(key, found, count, keyed)).unwrap_or(missing),
    };
    let (anchor, caret) = (place(&made.anchor, 0), place(&made.caret, count - 1));

    (anchor.min(caret)..=anchor.max(caret)).collect()
}

/// The place in a list of `count`, each keyed as `keyed` says, of the item keyed `key`, looked for outward from where the
/// last layout found it, as `found` says, or the list's start: none when no item is keyed so.
fn held(key: ItemKey, found: Option<&Window>, count: u32, keyed: &impl Fn(u32) -> Segment) -> Option<u32> {
    if count == 0 {
        return None;
    }
    let near = found.and_then(|window| window.place_of(Segment::Item(key))).unwrap_or(0);
    let at = locate(Segment::Item(key), near, count, keyed);
    let keyed_so = keyed(at) == Segment::Item(key);

    keyed_so.then_some(at)
}

/// The place in a list of `count` of the item keyed `key`, looked for from `near` outward: `near`, within the list,
/// when no item is keyed so.
fn locate(key: Segment, near: u32, count: u32, keyed: &impl Fn(u32) -> Segment) -> u32 {
    let last = count.saturating_sub(1);
    let near = near.min(last);
    let empty = count == 0;
    if empty {
        return 0;
    }
    for distance in 0..count {
        let on = near.checked_add(distance).filter(|on| *on <= last);
        let back = near.checked_sub(distance).filter(|_| distance > 0);
        let gone = on.is_none() && back.is_none();
        if gone {
            break;
        }
        for at in on.into_iter().chain(back) {
            let found = keyed(at) == key;
            if found {
                return at;
            }
        }
    }

    near
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
