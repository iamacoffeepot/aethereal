//! The top layer as a screen lists it: what it lists beneath the top layer, then each open popup's layer in tree order,
//! the markup's; a popup's own draws and hit listed over its anchor's; and where a popup's content is placed.

use super::super::Slot;
use super::{Popper, Stands};
use crate::layout::Boxed;
use crate::tree::Flat;
use crate::widget::Axis;
use crate::widgets::{Side, anchor};
use aethereal_math::{Rect2, Vec2};

/// What a screen lists of its widgets as a layout walks them: beneath its top layer, then the layer of each open popup,
/// in the order the walk opened them, which is tree order.
#[derive(Debug, Default)]
pub struct Layers {
    base: Vec<Slot>,
    tops: Vec<(usize, Vec<Slot>)>,
}

impl Layers {
    /// Opens the layer of the popup at `popup` in the list, over every layer opened before it.
    pub fn open(&mut self, popup: usize) {
        self.tops.push((popup, Vec::new()));
    }

    /// Lists `slots`, of a widget that stands as `stands` says, after what was listed there before.
    pub fn list(&mut self, stands: Stands, slots: Vec<Slot>) {
        match stands {
            Stands::Base => self.base.extend(slots),
            Stands::Top { popup } => {
                if let Some((_, layer)) = self.tops.iter_mut().find(|(opened, _)| *opened == popup) {
                    layer.extend(slots);
                }
            }
            Stands::Hidden => {}
        }
    }

    /// Lists each of `pending` that waits for no widget at or past `at` in the list: what a popup lists over its anchor,
    /// once its anchor's are listed.
    pub fn settle(&mut self, pending: &mut Vec<Pending>, at: usize) {
        while let Some(last) = pending.last() {
            let due = last.until <= at;
            if !due {
                break;
            }
            let Some(Pending { stands, slots, .. }) = pending.pop() else {
                break;
            };
            self.list(stands, slots);
        }
    }

    /// Everything listed, beneath the top layer first and then each popup's layer in turn.
    pub fn order(self) -> Vec<Slot> {
        let mut order = self.base;
        for (_, layer) in self.tops {
            order.extend(layer);
        }

        order
    }
}

/// What a popup lists over its anchor: its own draws and hit, listed where it stands once the walk reaches `until`, the
/// first place past its anchor's descendants.
#[derive(Debug)]
pub struct Pending {
    pub until: usize,
    pub stands: Stands,
    pub slots: Vec<Slot>,
}

/// The first place in `flat`, a tree's nodes in tree order, past each node's descendants.
pub fn spans(flat: &[Flat<'_>]) -> Vec<usize> {
    let mut ends: Vec<usize> = (1..=flat.len()).collect();
    for (at, node) in flat.iter().enumerate().rev() {
        if let Some(last) = node.children.iter().map(|child| ends[*child]).max() {
            ends[at] = last;
        }
    }

    ends
}

/// Where content `extent` big is placed against an anchor at `anchor`, its top left: on `side` of the anchor, or on the
/// other side when it does not fit on that one and the other has more room; then shifted as little as it can to stay
/// within `bounds`, its start at the bounds' start when it is longer than they are. Content below or above the anchor
/// starts at the anchor's left edge, and content right or left of it at its top edge.
pub fn place(anchor: Rect2, extent: Vec2, side: Side, bounds: Rect2) -> Vec2 {
    let (axis, across, forward) = match side {
        Side::Below => (Axis::Column, Axis::Row, true),
        Side::Above => (Axis::Column, Axis::Row, false),
        Side::Right => (Axis::Row, Axis::Column, true),
        Side::Left => (Axis::Row, Axis::Column, false),
    };

    let length = axis.along(extent);
    let after = axis.along(bounds.max) - axis.along(anchor.max);
    let before = axis.along(anchor.min) - axis.along(bounds.min);
    let (ahead, behind) = if forward {
        (after, before)
    } else {
        (before, after)
    };
    let fits = length <= ahead;
    let roomier = behind > ahead;
    let flips = !fits && roomier;
    let onward = forward != flips;
    let start = if onward {
        axis.along(anchor.max)
    } else {
        axis.along(anchor.min) - length
    };
    let main = shifted(start, length, axis.along(bounds.min), axis.along(bounds.max));
    let cross =
        shifted(across.along(anchor.min), across.along(extent), across.along(bounds.min), across.along(bounds.max));

    axis.vector(main) + across.vector(cross)
}

/// Opens the layer of `popper`, the open popup at `at` in `flat` whose box is `rect`, in `layers`, behind a backdrop
/// over `screen` when it is modal; and answers each piece of its content by its place, with how far it slides from
/// where it was laid out, `boxes`, to where it is placed against the popup within `screen`.
pub fn lift(
    layers: &mut Layers,
    flat: &[Flat<'_>],
    boxes: &[Boxed],
    at: usize,
    popper: Popper<'_>,
    rect: Rect2,
    screen: Rect2,
) -> Vec<(usize, Vec2)> {
    let node = &flat[at];
    layers.open(at);
    if popper.modal() {
        let backdrop = Slot::Backdrop { path: node.path.clone(), rect: screen };
        layers.list(Stands::Top { popup: at }, vec![backdrop]);
    }
    let first = anchor(&node.path);
    let content = node.children.iter().filter(|child| flat[**child].path != first);

    content
        .map(|child| {
            let laid = boxes[*child].rect;
            let placed = place(rect, laid.size(), popper.side(), screen);
            (*child, laid.min - placed)
        })
        .collect()
}

/// The first place in `flat` past the anchor of the popup `node` at `at`, `ends` the first place past each node's
/// descendants: where what the popup lists over its anchor is listed. Past the popup itself when it has no anchor.
pub fn past_anchor(node: &Flat<'_>, flat: &[Flat<'_>], ends: &[usize], at: usize) -> usize {
    let first = anchor(&node.path);
    let held = node.children.iter().find(|child| flat[**child].path == first);

    held.map_or(at + 1, |held| ends[*held])
}

/// `start`, of something `length` long, moved as little as it can to lie within `low` to `high`: to `low` when it is
/// longer than they are apart.
fn shifted(start: f32, length: f32, low: f32, high: f32) -> f32 {
    start.min(high - length).max(low)
}
