//! What a layout finds of each scroll, kept for the next view and the wheel ([`Window`]), and the three conditions a
//! scroll's list windows by, checked at each layout of the items it built (ADR-0056: Lists and conditions are
//! settings):
//!
//! 1. **Each item has a least length along the axis**: what every item is sure to take there, its margins with it,
//!    from what styles set in pixels and one line of each text within it ([`Widget::least`](crate::Widget::least)).
//!    The least of those among the items built, with the gap after each, bounds how many items fill the view:
//!    `ceil(view / least) + 1`, however long each really is.
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
        let mut sure = Vec::new();
        for child in children {
            least = least.min(self.least(*child, axis, measuring, &mut sure));
        }
        let bounded = least > 0.0;
        if !bounded {
            return Windowing::Every(Failed::Unbounded);
        }

        Windowing::Bounded { least: least + gap(style, axis, view, self.scale) }
    }

    /// The least length along `axis` of the item whose root is at `item`, its margins with it, whatever it holds: each
    /// node of it, its children before it, is sure to take what [`sure_of`] says, the least each of its leaves holds
    /// asked of its widget, `sure` keeping each node's, from the item's root on.
    fn least(&self, item: usize, axis: Axis, measuring: &mut Measuring<'flat>, sure: &mut Vec<f32>) -> f32 {
        let end = self.ends[item];
        sure.clear();
        sure.resize(end - item, 0.0);
        for at in (item..end).rev() {
            let node = &self.flat[at];
            let style = node.node.style();
            let children = node.children.iter().map(|child| sure[child - item]);
            let held = match node.node.widget().arrange() {
                Arrange::Leaf => {
                    measuring.at(node.path, Asker::Layout);
                    axis.along(node.node.widget().least(style, measuring))
                }
                Arrange::Flex(along) if along == axis && !style.wrap => {
                    let gaps =
                        items(node.children.len().saturating_sub(1)) * spaced(gap_along(style, axis), self.scale);
                    children.sum::<f32>() + gaps
                }
                Arrange::Flex(_) => children.fold(0.0, f32::max),
                Arrange::Grid | Arrange::Stack => {
                    let tracks = match axis {
                        Axis::Column => &style.rows,
                        Axis::Row => &style.columns,
                    };
                    let auto = tracks.is_empty();
                    if auto {
                        children.fold(0.0, f32::max)
                    } else {
                        0.0
                    }
                }
                Arrange::Popup(_) => {
                    let anchor = node.children.iter().find(|child| anchors(&self.flat[**child]));
                    anchor.map_or(0.0, |child| sure[child - item])
                }
                Arrange::Scroll(_) => 0.0,
            };
            let parent = self.flat[self.parents[at]].node.widget().arrange();
            let based = matches!(parent, Arrange::Flex(along) | Arrange::Scroll(along) if along == axis);
            sure[at - item] = sure_of(style, axis, (held.max(0.0), based), self.scale);
        }

        sure[0]
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

/// Whether `child` is the anchor of the popup it stands in: its first child.
fn anchors(child: &Flat<'_>) -> bool {
    matches!(child.path.segments().last(), Some(Segment::Place(0)))
}

/// `count` lengths, as a length is counted.
fn items(count: usize) -> f32 {
    // A node's children are far fewer than an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let items = count as f32;

    items
}

/// The least length along `axis` of the margin box of a node styled `style`, at `scale`, holding at least `held` pixels
/// along it, `based` when its parent runs along the axis and sizes it from its flex basis: each length there as it is
/// sure to be, whatever its parent and what it holds, a share or what layout works out counting none.
///
/// - A length in pixels is its border box's, its padding and border within it; a share is none.
/// - Otherwise its border box holds what it holds, and its padding and border along the axis, each side.
/// - Its flex basis, where its parent sizes it by one, sets its main length: none is sure past its padding and border.
/// - Its most in pixels bounds that, and its least in pixels, and then its padding and border, raise it: a most that is
///   a share bounds it at its padding and border.
/// - Its margins along the axis, before and after, add to how far it stands from the next node, as a flex or grid
///   container lays them out: a margin in pixels counts, one that is a share or is set by layout counts none.
fn sure_of(style: &Style, axis: Axis, (held, based): (f32, bool), scale: f32) -> f32 {
    let (length, least, most) = match axis {
        Axis::Column => (style.height, style.min_height, style.max_height),
        Axis::Row => (style.width, style.min_width, style.max_width),
    };
    let (padding, margin) = match axis {
        Axis::Column => ((style.padding.top, style.padding.bottom), (style.margin.top, style.margin.bottom)),
        Axis::Row => ((style.padding.left, style.padding.right), (style.margin.left, style.margin.right)),
    };
    let border = style.border.width * scale;
    let rim = spaced(padding.0, scale) + spaced(padding.1, scale) + border + border;
    let by_basis = based && style.basis != Length::Auto;
    let sized = match length {
        _ if by_basis => rim,
        Length::Pixels(pixels) => pixels * scale,
        Length::Share(_) => rim,
        Length::Auto => held + rim,
    };
    let bounded = match most {
        Length::Pixels(pixels) => sized.min(pixels * scale),
        Length::Share(_) => rim,
        Length::Auto => sized,
    };
    let boxed = bounded.max(pixels(least, scale)).max(rim);

    boxed + pixels(margin.0, scale) + pixels(margin.1, scale)
}

/// `space` in pixels at `scale`, when it is in pixels: none for a share.
fn spaced(space: Space, scale: f32) -> f32 {
    match space {
        Space::Pixels(pixels) => pixels * scale,
        Space::Share(_) => 0.0,
    }
}

/// The gap a container styled `style` leaves between its children along `axis`.
const fn gap_along(style: &Style, axis: Axis) -> Space {
    match axis {
        Axis::Row => style.gap.columns,
        Axis::Column => style.gap.rows,
    }
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
    match gap_along(style, axis) {
        Space::Pixels(pixels) => pixels * scale,
        Space::Share(share) => share * view,
    }
}
