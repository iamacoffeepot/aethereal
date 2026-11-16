//! Laying a tree out (ADR-0056: Layout is Taffy): Taffy's flexbox and grid algorithms, run through its low-level
//! traits over the tree's nodes in tree order, never over Taffy's own tree. The root is the screen's size; every length
//! is the style's times the UI scale; and a leaf measures its content, a text with its font's metrics, as Taffy asks.
//!
//! A scroll is a flex container whose children are never shrunk, so they run past its box. When it built only a window
//! of a list's items, its lines are taken to be one length, the distance from its first built child to its second, or
//! its first's length and the gap with only one: each built child is placed at its item's line, and what it scrolls
//! through runs to the list's last line, so the content's length is known without building every item. Where it slides
//! them, and what it cuts them to, is the screen's, which keeps its offset.
//!
//! A popup lays out its anchor alone, as a `Stack` lays out its child. The content of each open popup is laid out after
//! the rest, each piece as its own root, out of flow in the screen's box: as wide and tall as it asks, within the
//! screen, from the screen's top left. Where it is placed against its anchor is the screen's, which knows where its
//! anchor stands once every scroll around it has slid it (ADR-0056: Popups draw in one top layer per screen).
//!
//! A screen keeps what each layout found for the next ([`Layouts`]): the last boxes, handed back while its tree lays out
//! as it did, and what Taffy keeps of each node, so a layout lays out again only the nodes whose layout changed and
//! those that hold them, as Taffy's own tree does when a node is marked dirty.

mod kept;
#[cfg(test)]
mod tests;

pub use kept::Layouts;

use crate::fonts::{Asker, Measuring};
use crate::style::{Edges, Parent, Space, Style, flow};
use crate::tree::Flat;
use crate::widget::{Arrange, Axis, Lines, Offer, Offered, anchor};
use aethereal_math::{Rect2, Vec2};
use core::cell::OnceCell;
use core::slice::Iter;
use kept::Kept;
use taffy::{
    AvailableSpace, CacheTree, Dimension, Layout, LayoutContainingBlock, LayoutFlexboxContainer, LayoutGridContainer,
    LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, Position, RunMode, Size, TraversePartialTree,
    compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_leaf_layout, compute_oof_layout,
};

/// Where layout put one node, in its screen's pixels from the screen's top left, as though no scroll had slid it: its
/// border box; its content box, the border box less its border, its room, and its padding; and how far its children
/// reach from its content box's top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boxed {
    pub rect: Rect2,
    pub content: Rect2,
    pub extent: Vec2,
}

/// How a scroll's lines stand along its axis, in pixels: where its first line starts past its content box's start,
/// and how far one line is from the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pitch {
    pub lead: f32,
    pub stride: f32,
}

/// What a layout found: each node's boxes, in tree order, and the pitch of each scroll with a child, by its place.
#[derive(Debug, Default, PartialEq)]
pub struct Laidout {
    pub boxes: Vec<Boxed>,
    pub pitches: Vec<(usize, Pitch)>,
}

/// Where each of `flat`, laid out as `layouts` with its children laid out in its box `flowing` and `tops` rooted on
/// their own, stands at `scale`: its boxes, from the screen's top left, and each scroll's pitch, the items a window
/// did not build counted in what it reaches.
fn boxed(flat: &[Flat<'_>], tops: &[usize], flowing: &[&[usize]], layouts: &[Layout], scale: f32) -> Laidout {
    let mut locals: Vec<Vec2> = layouts.iter().map(|layout| Vec2::new(layout.location.x, layout.location.y)).collect();
    let mut pitches = Vec::new();
    for (at, node) in flat.iter().enumerate() {
        let Arrange::Scroll(axis) = node.node.widget().arrange() else {
            continue;
        };
        let Some(pitch) = pitch(node, layouts, &locals, &layouts[at], axis, scale) else {
            continue;
        };
        if let Lines::Window { at: items, .. } = node.node.widget().lines() {
            lined(node, flat, items, &mut locals, axis, pitch);
        }
        pitches.push((at, pitch));
    }

    let mut boxes: Vec<Boxed> = Vec::with_capacity(flat.len());
    let mut origins = vec![Vec2::ZERO; flat.len()];
    for (at, layout) in layouts.iter().enumerate() {
        let rooted = tops.contains(&at);
        let origin = if rooted {
            locals[at]
        } else {
            origins[at] + locals[at]
        };
        for child in flat[at].children {
            origins[*child] = origin;
        }
        let rect = Rect2::from_origin_size(origin, size_of(layout));
        let (inset_min, inset_max) = insets(layout);
        let inner_min = rect.min + inset_min;
        let inner_max = rect.max - inset_max;
        let content =
            Rect2 { min: inner_min, max: Vec2::new(inner_max.x.max(inner_min.x), inner_max.y.max(inner_min.y)) };
        let mut extent = Vec2::ZERO;
        for child in flowing[at] {
            extent = extent.max(locals[*child] + size_of(&layouts[*child]) - inset_min);
        }
        let windowed = pitches.binary_search_by_key(&at, |(scroll, _)| *scroll).ok().map(|found| pitches[found].1);
        let (Some(pitch), Arrange::Scroll(axis), Lines::Window { count, .. }, Some(first)) =
            (windowed, flat[at].node.widget().arrange(), flat[at].node.widget().lines(), flat[at].children.first())
        else {
            boxes.push(Boxed { rect, content, extent });
            continue;
        };
        let last = pitch.stride.mul_add(lines(count.saturating_sub(1)), pitch.lead);
        let along = last + axis.along(size_of(&layouts[*first]));
        let extent = match axis {
            Axis::Row => Vec2::new(along, extent.y),
            Axis::Column => Vec2::new(extent.x, along),
        };
        boxes.push(Boxed { rect, content, extent });
    }

    Laidout { boxes, pitches }
}

/// The places in `flat` of the children of `node` laid out in its box: a popup's anchor alone, and every other node's
/// children.
fn in_flow<'flat>(node: &Flat<'flat>, flat: &[Flat<'_>]) -> &'flat [usize] {
    match node.node.widget().arrange() {
        Arrange::Popup(_) => {
            let anchor = anchor(node.path);
            let held = node.children.iter().position(|child| *flat[*child].path == anchor);
            held.map_or(&[], |at| &node.children[at..=at])
        }
        Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Scroll(_) | Arrange::Leaf => node.children,
    }
}

/// A node's border box's size, as Taffy laid it out.
const fn size_of(layout: &Layout) -> Vec2 {
    Vec2::new(layout.size.width, layout.size.height)
}

/// How far a node's border box is inset to its content box from its top left, and from its bottom right.
fn insets(layout: &Layout) -> (Vec2, Vec2) {
    let Layout { border, padding, .. } = *layout;
    let min = Vec2::new(border.left + padding.left, border.top + padding.top);
    let max = Vec2::new(border.right + padding.right, border.bottom + padding.bottom);

    (min, max)
}

/// The pitch of the lines of `node`, a scroll along `axis` laid out as `layout`, its children placed at `locals` as
/// Taffy put them: its first child's start past its content box's, and the distance from its first child to its
/// second, or with only one, the first's length and the gap. None for a scroll with no child.
fn pitch(
    node: &Flat<'_>,
    layouts: &[Layout],
    locals: &[Vec2],
    layout: &Layout,
    axis: Axis,
    scale: f32,
) -> Option<Pitch> {
    let first = *node.children.first()?;
    let (inset_min, inset_max) = insets(layout);
    let lead = axis.along(locals[first] - inset_min);
    let alone = || {
        let content = axis.along(size_of(layout) - inset_min - inset_max);
        axis.along(size_of(&layouts[first])) + gap(node.node.style(), axis, content, scale)
    };
    let stride = node.children.get(1).map_or_else(alone, |second| axis.along(locals[*second] - locals[first]));

    Some(Pitch { lead, stride })
}

/// Places each child of `node`, a scroll along `axis` whose children are the items `items` of a list, at its item's
/// line, `pitch` apart from the first line, which Taffy put its first child at.
fn lined(node: &Flat<'_>, flat: &[Flat<'_>], items: &[u32], locals: &mut [Vec2], axis: Axis, pitch: Pitch) {
    let Some(first) = node.children.first() else {
        return;
    };
    let start = axis.along(locals[*first]);
    for child in node.children {
        let Some(item) = items.get(flat[*child].born) else {
            continue;
        };
        let along = pitch.stride.mul_add(lines(*item), start);
        let local = locals[*child];
        locals[*child] = match axis {
            Axis::Row => Vec2::new(along, local.y),
            Axis::Column => Vec2::new(local.x, along),
        };
    }
}

/// `count` lines, as a length is counted.
fn lines(count: u32) -> f32 {
    // A list's lines are far fewer than an f32 counts exactly.
    #[allow(clippy::cast_precision_loss)]
    let lines = count as f32;

    lines
}

/// The gap a flex container styled `style` leaves between its children along `axis`, in pixels at `scale`: a share is
/// of its content box's length along the axis, `content`.
fn gap(style: &Style, axis: Axis, content: f32, scale: f32) -> f32 {
    let space = match axis {
        Axis::Row => style.gap.columns,
        Axis::Column => style.gap.rows,
    };
    match space {
        Space::Pixels(pixels) => pixels * scale,
        Space::Share(share) => share * content,
    }
}

/// One node as Taffy lays it out: how it arranges its children, the node, the places of the children it lays out in its
/// box, what its style is converted to Taffy's with, its style as Taffy reads it, converted as Taffy first reads it,
/// and what Taffy keeps of it.
struct Laying<'flat> {
    arrange: Arrange,
    flat: &'flat Flat<'flat>,
    children: &'flat [usize],
    converting: Converting,
    style: OnceCell<taffy::Style>,
    kept: Kept,
}

/// What a node's style is converted to Taffy's with beside the style itself: how it arranges its children, how its
/// parent arranges it, the room it keeps, the UI scale, and where it stands.
#[derive(Debug, Clone, Copy)]
struct Converting {
    arrange: Arrange,
    parent: Parent,
    room: Edges<f32>,
    scale: f32,
    stands: Stands,
}

/// Where a node stands in the layout beside its style: the root, the screen's size; the root of an open popup's
/// content, out of flow; or within its parent's flow.
#[derive(Debug, Clone, Copy)]
enum Stands {
    Root(Vec2),
    Top,
    Flowing,
}

impl Laying<'_> {
    /// Its style as Taffy reads it, converted the first time it is read.
    fn style(&self) -> &taffy::Style {
        self.style.get_or_init(|| {
            let Converting { arrange, parent, room, scale, stands } = self.converting;
            let mut style = flow(self.flat.node.style(), arrange, parent, room, scale);
            match stands {
                Stands::Root(size) => {
                    style.size = Size { width: Dimension::length(size.x), height: Dimension::length(size.y) };
                }
                Stands::Top => style.position = Position::Absolute,
                Stands::Flowing => {}
            }
            style
        })
    }
}

/// A tree's nodes as Taffy walks them, by their places in tree order, with the fonts its leaves measure by.
struct Tree<'flat, 'measuring> {
    nodes: Vec<Laying<'flat>>,
    measuring: &'measuring mut Measuring<'flat>,
}

impl<'flat> Tree<'flat, '_> {
    /// The node Taffy names `id`.
    fn node(&self, id: NodeId) -> &Laying<'flat> {
        &self.nodes[usize::from(id)]
    }

    /// What Taffy keeps of the node it names `id`, to write.
    fn kept(&mut self, id: NodeId) -> &mut Kept {
        &mut self.nodes[usize::from(id)].kept
    }
}

/// The children of a node, as Taffy names them.
struct Children<'a>(Iter<'a, usize>);

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        self.0.next().map(|at| NodeId::from(*at))
    }
}

impl TraversePartialTree for Tree<'_, '_> {
    type ChildIter<'a>
        = Children<'a>
    where
        Self: 'a;

    fn child_ids(&self, parent: NodeId) -> Children<'_> {
        Children(self.node(parent).children.iter())
    }

    fn child_count(&self, parent: NodeId) -> usize {
        self.node(parent).children.len()
    }

    fn get_child_id(&self, parent: NodeId, index: usize) -> NodeId {
        NodeId::from(self.node(parent).children[index])
    }
}

impl LayoutPartialTree for Tree<'_, '_> {
    type CoreContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type CustomIdent = String;

    fn get_core_container_style(&self, node: NodeId) -> &taffy::Style {
        self.node(node).style()
    }

    fn set_unrounded_layout(&mut self, node: NodeId, layout: &Layout) {
        self.kept(node).layout = *layout;
    }

    fn compute_child_layout(&mut self, node: NodeId, inputs: LayoutInput) -> LayoutOutput {
        compute_cached_layout(self, node, inputs, |tree, node, inputs| {
            let arrange = tree.node(node).arrange;
            let mut output = match arrange {
                Arrange::Flex(_) | Arrange::Scroll(_) => compute_flexbox_layout(tree, node, inputs),
                Arrange::Grid | Arrange::Stack | Arrange::Popup(_) => compute_grid_layout(tree, node, inputs),
                Arrange::Leaf => {
                    let Tree { nodes, measuring } = tree;
                    let laying = &nodes[usize::from(node)];
                    measuring.at(laying.flat.path, Asker::Layout);
                    let measured = |known: Size<Option<f32>>, available: Size<AvailableSpace>| {
                        let offer = Offer {
                            width: offered(known.width, available.width),
                            height: offered(known.height, available.height),
                        };
                        let content = laying.flat.node.widget().measure(laying.flat.node.style(), measuring, offer);

                        Size { width: content.x, height: content.y }
                    };
                    compute_leaf_layout(inputs, laying.style(), |_, _| 0.0, measured)
                }
            };
            let performs = inputs.run_mode == RunMode::PerformLayout;
            if performs {
                compute_oof_layout(tree, node, &mut output);
            }

            output
        })
    }
}

/// The space offered along one axis: exactly its known size, or as Taffy offers it.
fn offered(known: Option<f32>, available: AvailableSpace) -> Offered {
    match (known, available) {
        (Some(exactly), _) => Offered::Exactly(exactly),
        (None, AvailableSpace::Definite(most)) => Offered::AtMost(most),
        (None, AvailableSpace::MinContent) => Offered::MinContent,
        (None, AvailableSpace::MaxContent) => Offered::MaxContent,
    }
}

impl LayoutContainingBlock for Tree<'_, '_> {
    type OofItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_oof_item_style(&self, node: NodeId) -> &taffy::Style {
        self.node(node).style()
    }

    fn clear_hoisted_children(&mut self, node: NodeId) {
        self.kept(node).hoisted.clear();
    }

    fn add_hoisted_children(&mut self, node: NodeId, hoisted: &[NodeId]) {
        self.kept(node).hoisted.extend_from_slice(hoisted);
    }
}

impl CacheTree for Tree<'_, '_> {
    fn cache_get(&mut self, node: NodeId, inputs: &LayoutInput) -> Option<LayoutOutput> {
        self.kept(node).cache.get(inputs)
    }

    fn cache_store(&mut self, node: NodeId, inputs: &LayoutInput, output: LayoutOutput) {
        self.kept(node).cache.store(inputs, output);
    }

    fn cache_clear(&mut self, node: NodeId) {
        self.kept(node).cache.clear();
    }
}

impl LayoutFlexboxContainer for Tree<'_, '_> {
    type FlexboxContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type FlexboxItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_flexbox_container_style(&self, node: NodeId) -> &taffy::Style {
        self.node(node).style()
    }

    fn get_flexbox_child_style(&self, child: NodeId) -> &taffy::Style {
        self.node(child).style()
    }
}

impl LayoutGridContainer for Tree<'_, '_> {
    type GridContainerStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    type GridItemStyle<'a>
        = &'a taffy::Style
    where
        Self: 'a;

    fn get_grid_container_style(&self, node: NodeId) -> &taffy::Style {
        self.node(node).style()
    }

    fn get_grid_child_style(&self, child: NodeId) -> &taffy::Style {
        self.node(child).style()
    }
}
