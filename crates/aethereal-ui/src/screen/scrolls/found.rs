//! What a layout finds of each scroll, kept for the next view and the wheel ([`Window`]), and the three conditions a
//! scroll's list windows by, checked at each layout of the items it built (ADR-0056: Lists and conditions are
//! settings):
//!
//! 1. **Each item has a least length along the axis**: its style's least or fixed length there, or one line of a text
//!    within it ([`Widget::least`](crate::Widget::least)). The least of those among the items built, with the gap
//!    after each, bounds how many items fill the view: `ceil(view / least) + 1`, however long each really is.
//! 2. **Each item stands where the items before it put it**: the scroll packs them at its start and does not wrap
//!    them. Its children are never shrunk.
//! 3. **Its length across its axis does not depend on what it holds**: its style sets it, in pixels, or as a share of,
//!    or stretched across, a parent whose length across is set so, up to the screen.
//!
//! A scroll whose list fails one builds every item, and its screen logs which, once, as it starts to; so does a scroll
//! one of whose plain children holds a list, which windows only when the scroll holds the list's items itself.

use super::window::{Failed, Lined, Window, Windowing};
use crate::fonts::{Asker, Measuring};
use crate::layout::Boxed;
use crate::style::{Alignment, Distribute, Length, SelfAlign, Space, Style};
use crate::tree::{Flat, Segment};
use crate::widget::{Arrange, Axis, Lines};

/// What the last layout found of each scroll in `flat`, laid out as `boxes`, by its place in tree order: each node's
/// subtree running to its place in `ends`, its parent's place in `parents`; its items' least lengths measured with
/// `measuring` at `scale`.
pub fn windows<'flat>(
    flat: &'flat [Flat<'flat>],
    boxes: &[Boxed],
    (ends, parents): (&[usize], &[usize]),
    measuring: &mut Measuring<'flat>,
    scale: f32,
) -> Vec<(usize, Window)> {
    let mut found = Vec::new();
    for (at, node) in flat.iter().enumerate() {
        let Arrange::Scroll(axis) = node.node.widget().arrange() else {
            continue;
        };
        let scroll = &boxes[at];
        let content = axis.along(scroll.content.min);
        let view = axis.along(scroll.content.size());
        let travel = (axis.along(scroll.extent) - view).max(0.0);
        let lines = node.node.widget().lines();
        let items = node
            .children
            .iter()
            .zip(0_u32..)
            .map(|(child, born)| {
                let laid = boxes[*child].rect;
                let place = match lines {
                    Lines::Whole => born,
                    Lines::Window { at: places, .. } => places.get(flat[*child].born).copied().unwrap_or(born),
                };
                let key = flat[*child].path.segments().last().copied().unwrap_or(Segment::Place(born));
                let start = axis.along(laid.min) - content;
                Lined { place, key, start, length: axis.along(laid.size()), placed: start }
            })
            .collect();
        let (count, windowing) = match lines {
            Lines::Whole => (u32::try_from(node.children.len()).unwrap_or(u32::MAX), wraps(node, flat)),
            Lines::Window { count, .. } => {
                let checking = Checking { flat, ends, parents, scale };
                (*count, checking.windowing(at, axis, view, measuring))
            }
        };
        let window = Window::new((node.path.clone(), axis), (view, travel), count, items, windowing);
        found.push((at, window));
    }

    found
}

/// The scroll `node`, among `flat`, holding children its markup writes: one that wraps a list when one of them holds a
/// list's items.
fn wraps(node: &Flat<'_>, flat: &[Flat<'_>]) -> Windowing {
    let holds_list = node.children.iter().any(|child| {
        let grandchildren = flat[*child].children;
        grandchildren
            .iter()
            .any(|grandchild| matches!(flat[*grandchild].path.segments().last(), Some(Segment::Item(_))))
    });
    if holds_list {
        return Windowing::Wraps;
    }

    Windowing::Whole
}

/// What a scroll's conditions are checked against: the tree's nodes, where each one's subtree ends and its parent, and
/// the UI scale.
struct Checking<'check, 'flat> {
    flat: &'check [Flat<'flat>],
    ends: &'check [usize],
    parents: &'check [usize],
    scale: f32,
}

impl<'flat> Checking<'_, 'flat> {
    /// Whether the scroll at `at`, along `axis`, its view `view` pixels long, windows the items it holds: the least
    /// length among those it built, measured with `measuring`, when it meets every condition.
    fn windowing(&self, at: usize, axis: Axis, view: f32, measuring: &mut Measuring<'flat>) -> Windowing {
        let style = self.flat[at].node.style();
        let packs = matches!(style.justify_content, Distribute::Normal | Distribute::Start);
        let placed_by_those_before = packs && !style.wrap;
        if !placed_by_those_before {
            return Windowing::Every(Failed::Distributed);
        }
        let set_across = self.set_across(at, axis);
        if !set_across {
            return Windowing::Every(Failed::Across);
        }
        let children = self.flat[at].children;
        if children.is_empty() {
            return Windowing::Unmeasured;
        }
        let mut least = f32::INFINITY;
        for child in children {
            least = least.min(self.least(*child, axis, measuring));
        }
        let bounded = least > 0.0;
        if !bounded {
            return Windowing::Every(Failed::Unbounded);
        }

        Windowing::Bounded { least: least + gap(style, axis, view, self.scale) }
    }

    /// The least length along `axis` of the item whose root is at `item`, whatever it holds: its style's fixed length
    /// there; or the longer of its style's least length and the longest line of a text within it.
    fn least(&self, item: usize, axis: Axis, measuring: &mut Measuring<'flat>) -> f32 {
        let style = self.flat[item].node.style();
        let (length, least) = match axis {
            Axis::Column => (style.height, style.min_height),
            Axis::Row => (style.width, style.min_width),
        };
        if let Length::Pixels(fixed) = length {
            return fixed * self.scale;
        }
        let mut longest = pixels(least, self.scale);
        for node in &self.flat[item..self.ends[item]] {
            let leaf = node.node.widget().arrange() == Arrange::Leaf;
            if leaf {
                measuring.at(node.path, Asker::Layout);
                let line = axis.along(node.node.widget().least(node.node.style(), measuring));
                longest = longest.max(line);
            }
        }

        longest
    }

    /// Whether the length across `axis` of the node at `at` is set by styles alone, not by what it holds: its style
    /// sets it in pixels; or as a share of its parent's, or stretched across its parent, whose length across is set
    /// so; up to the screen's root, which is the screen's size.
    fn set_across(&self, at: usize, axis: Axis) -> bool {
        let mut node = at;
        loop {
            let style = self.flat[node].node.style();
            let across = match axis {
                Axis::Column => style.width,
                Axis::Row => style.height,
            };
            let fixed = matches!(across, Length::Pixels(_));
            if fixed {
                return true;
            }
            let root = node == 0;
            if root {
                return true;
            }
            let parent = &self.flat[self.parents[node]];
            let shared = matches!(across, Length::Share(_));
            let stretched = stretches(parent.node.style(), parent.node.widget().arrange(), style, axis);
            let carried = shared || stretched;
            if !carried {
                return false;
            }
            node = self.parents[node];
        }
    }
}

/// Whether a parent styled `parent`, arranging its children as `arrange`, stretches a child styled `child` across
/// `axis`: a flex container along `axis` stretches a child that sets itself as it is told, or stretches itself.
fn stretches(parent: &Style, arrange: Arrange, child: &Style, axis: Axis) -> bool {
    let flexes = matches!(arrange, Arrange::Flex(along) | Arrange::Scroll(along) if along == axis);
    let aligned = match child.align_self {
        SelfAlign::Auto => parent.align_items,
        SelfAlign::Own(alignment) => alignment,
    };
    let stretched = matches!(aligned, Alignment::Normal | Alignment::Stretch);

    flexes && stretched
}

/// `length` in pixels at `scale`, when it is in pixels: none for any other.
fn pixels(length: Length, scale: f32) -> f32 {
    match length {
        Length::Pixels(pixels) => pixels * scale,
        Length::Auto | Length::Share(_) => 0.0,
    }
}

/// The gap a scroll styled `style` leaves between its children along `axis`, in pixels at `scale`: a share is of its
/// content box's length along the axis, `view`.
fn gap(style: &Style, axis: Axis, view: f32, scale: f32) -> f32 {
    let space = match axis {
        Axis::Row => style.gap.columns,
        Axis::Column => style.gap.rows,
    };
    match space {
        Space::Pixels(pixels) => pixels * scale,
        Space::Share(share) => share * view,
    }
}
