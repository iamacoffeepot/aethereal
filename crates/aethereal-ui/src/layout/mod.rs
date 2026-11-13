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

#[cfg(test)]
mod tests;

use crate::fonts::{Asker, Measuring};
use crate::style::{Edges, Parent, Space, Style, flow};
use crate::tree::{Flat, KeyPath, Relaid, Segment};
use crate::widget::{Arrange, Axis, Lines, Offer, Offered, anchor};
use aethereal_math::{Rect2, Vec2};
use core::mem;
use core::slice::Iter;
use std::collections::BTreeMap;
use taffy::{
    AvailableSpace, Cache, CacheTree, Dimension, Layout, LayoutContainingBlock, LayoutFlexboxContainer,
    LayoutGridContainer, LayoutInput, LayoutOutput, LayoutPartialTree, NodeId, Position, RunMode, Size,
    TraversePartialTree, compute_cached_layout, compute_flexbox_layout, compute_grid_layout, compute_leaf_layout,
    compute_oof_layout, compute_root_layout,
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

/// What a screen keeps of its layouts from one to the next (*settled by the library's layout-cost stage*): what Taffy
/// kept of each node, by its key path, and the boxes of the last layout, with whether they hold.
///
/// A layout whose tree lays out as the last one did, at the same size, scale, and open popups, is the last one: its
/// boxes are handed back, and Taffy does not run. Otherwise Taffy runs, and a node whose layout is kept, whose own
/// settings and whose children's laid out as they did, answers from what it kept, so only what changed and what holds
/// it is laid out again. What tells it what changed is the comparison of each tree with the last ([`Relaid`]), and its
/// own: a node whose style, as Taffy reads it, is not what it kept, as its room or its parent's arrangement changed, is
/// laid out again, with all that holds it; and every node kept is let go at another UI scale, which every length and
/// measure reads, and after a layout that met a font whose metrics had not come.
#[derive(Debug, Default)]
pub struct Layouts {
    /// What Taffy kept of each node, by its key path, as the last layout left it.
    nodes: BTreeMap<KeyPath, Kept>,
    /// The UI scale the nodes kept were laid out at.
    scale: f32,
    /// The boxes the last layout gave.
    laidout: Laidout,
    holds: Holds,
}

/// Whether the boxes a screen's last layout gave hold for the tree it lays out next.
#[derive(Debug, Default)]
enum Holds {
    /// They do not: nothing was laid out, the tree laid out otherwise since, or a font's metrics had not come.
    #[default]
    Not,
    /// They do while the tree lays out as it did, at what this says.
    At(LaidAt),
}

/// What a layout was laid out at beside its tree: the screen's size and UI scale, and the places of the open popups'
/// content.
#[derive(Debug)]
struct LaidAt {
    size: Vec2,
    scale: f32,
    tops: Vec<usize>,
}

impl LaidAt {
    /// Whether it is `size`, `scale`, and `tops`.
    fn is(&self, size: Vec2, scale: f32, tops: &[usize]) -> bool {
        let sized = self.size == size;
        let scaled = self.scale.to_bits() == scale.to_bits();
        let topped = self.tops == tops;

        sized && scaled && topped
    }
}

/// What Taffy keeps of one node from one layout to the next: its place in tree order when it was kept, which Taffy
/// names it by; how it flowed; Taffy's cache of what it laid out; where its parent laid it; and the boxes out of flow
/// it holds, by their places.
#[derive(Debug)]
struct Kept {
    at: usize,
    flow: Flow,
    cache: Cache,
    layout: Layout,
    hoisted: Vec<NodeId>,
}

/// What a node's style as Taffy reads it takes beside its own style and its widget's arrangement, which its tree's
/// comparison tells of: what its parent is to it, the room it keeps, and where it is placed.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Flow {
    parent: Parent,
    room: Edges<f32>,
    placed: Placing,
}

/// Where a node is laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Placing {
    /// It is the root, the screen's size, here.
    Root(Vec2),
    /// It is the content of an open popup, out of flow in the screen's box.
    Top,
    /// In its parent's box, as its parent lays it out, or nowhere, in a shut popup's content.
    Held,
}

impl Kept {
    /// A node at `at` that flows as `flow` says, laid out nowhere yet.
    const fn new(at: usize, flow: Flow) -> Self {
        Self { at, flow, cache: Cache::new(), layout: Layout::with_order(0), hoisted: Vec::new() }
    }

    /// It, at `at` in tree order: what it hoisted moves with it, as its subtree stands unchanged.
    fn moved(mut self, at: usize) -> Self {
        let moved = at != self.at;
        if moved {
            for hoisted in &mut self.hoisted {
                *hoisted = NodeId::from(usize::from(*hoisted) + at - self.at);
            }
            self.at = at;
        }

        self
    }
}

impl Layouts {
    /// Lays `flat`, a tree's nodes in tree order, out in a screen `size` pixels across and down at `scale` pixels to
    /// the logical pixel, measuring text with `measuring`, and then each node at a place among `tops` as a root of its
    /// own, out of flow in the screen's box, the content of an open popup: each node's boxes, in the same order, and
    /// each scroll's pitch. A node within a popup's content whose root is not among `tops` is laid out nowhere.
    ///
    /// The last layout's boxes when the tree lays out as it did, at the same size, scale, and tops, which it told
    /// ([`Relaid`]).
    pub fn lay_out<'flat>(
        &mut self,
        flat: &'flat [Flat<'flat>],
        tops: &[usize],
        measuring: &mut Measuring<'flat>,
        size: Vec2,
        scale: f32,
    ) -> &Laidout {
        let holds = match &self.holds {
            Holds::At(at) => at.is(size, scale, tops),
            Holds::Not => false,
        };
        if holds {
            return &self.laidout;
        }
        self.laidout = self.laid(flat, tops, measuring, size, scale);
        let waited = measuring.waited();
        self.holds = if waited {
            self.nodes.clear();
            Holds::Not
        } else {
            Holds::At(LaidAt { size, scale, tops: tops.to_vec() })
        };

        &self.laidout
    }

    /// Lays `flat` out with Taffy, as [`Self::lay_out`] says, from what was kept of each node that lays out as it did,
    /// and keeps what Taffy kept of each.
    fn laid<'flat>(
        &mut self,
        flat: &'flat [Flat<'flat>],
        tops: &[usize],
        measuring: &mut Measuring<'flat>,
        size: Vec2,
        scale: f32,
    ) -> Laidout {
        let rescaled = self.scale.to_bits() != scale.to_bits();
        if rescaled {
            self.nodes.clear();
            self.scale = scale;
        }
        let mut parents = vec![Parent::Other; flat.len()];
        for flat in flat {
            let parent = Parent::of(flat.node.widget().arrange());
            for child in &flat.children {
                parents[*child] = parent;
            }
        }
        let flowing: Vec<Vec<usize>> = flat.iter().map(|node| in_flow(node, flat)).collect();
        let mut was = mem::take(&mut self.nodes);
        let mut keys: Vec<KeyPath> = Vec::with_capacity(flat.len());
        let mut anew = vec![false; flat.len()];
        let mut nodes: Vec<Laying<'_>> = Vec::with_capacity(flat.len());
        for (at, ((node, parent), children)) in flat.iter().zip(parents).zip(&flowing).enumerate() {
            let widget = node.node.widget();
            let arrange = widget.arrange();
            measuring.at(&node.path, Asker::Layout);
            let room = widget.room(node.node.style(), measuring);
            let mut style = flow(node.node.style(), arrange, parent, room, scale);
            let placed = match at {
                0 => Placing::Root(size),
                _ if tops.contains(&at) => Placing::Top,
                _ => Placing::Held,
            };
            match placed {
                Placing::Root(size) => {
                    style.size = Size { width: Dimension::length(size.x), height: Dimension::length(size.y) };
                }
                Placing::Top => style.position = Position::Absolute,
                Placing::Held => {}
            }
            let flowed = Flow { parent, room, placed };
            let (key, kept, changed) = match was.remove_entry(&node.path) {
                Some((key, kept)) => {
                    let reflowed = kept.flow != flowed;
                    let emptied = kept.cache.is_empty();
                    (key, Kept { flow: flowed, ..kept.moved(at) }, reflowed || emptied)
                }
                None => (node.path.clone(), Kept::new(at, flowed), true),
            };
            anew[at] = changed;
            keys.push(key);
            nodes.push(Laying { arrange, flat: node, children: children.clone(), style, kept });
        }
        let reached = reached(&flowing, tops);
        for (at, reached) in reached.iter().enumerate().rev() {
            let holds = flowing[at].iter().any(|child| anew[*child]);
            let unreached = !*reached;
            let relaid = anew[at] || holds || unreached;
            if relaid {
                anew[at] = true;
                let laying = &mut nodes[at].kept;
                let _ = laying.cache.clear();
                laying.layout = Layout::with_order(0);
            }
        }

        let mut tree = Tree { nodes, measuring };
        let space = Size { width: AvailableSpace::Definite(size.x), height: AvailableSpace::Definite(size.y) };
        compute_root_layout(&mut tree, NodeId::from(0_usize), space);
        for top in tops {
            compute_root_layout(&mut tree, NodeId::from(*top), space);
        }

        let layouts: Vec<Layout> = tree.nodes.iter().map(|laying| laying.kept.layout).collect();
        self.nodes = keys.into_iter().zip(tree.nodes).map(|(key, laying)| (key, laying.kept)).collect();

        boxed(flat, tops, &flowing, &layouts, scale)
    }
}

/// A node whose layout changed lets go of what Taffy kept of it and of every node holding it, and the last boxes no
/// longer hold; a node new to the tree takes nothing kept at or within its path, which may be left from a tree before
/// the last, when a tree between was never laid out; and what Taffy kept of a node that left goes at the next layout,
/// which keeps only what its tree holds.
impl Relaid for Layouts {
    fn changed(&mut self, path: &[Segment]) {
        self.holds = Holds::Not;
        for depth in 1..=path.len() {
            if let Some(kept) = self.nodes.get_mut(&path[..depth]) {
                let _ = kept.cache.clear();
            }
        }
    }

    fn entered(&mut self, path: &[Segment]) {
        self.holds = Holds::Not;
        self.nodes.retain(|kept, _| !kept.segments().starts_with(path));
    }

    fn left(&mut self, _: &[Segment]) {
        self.holds = Holds::Not;
    }
}

/// Which of the nodes whose children laid out in their boxes are `flowing` are laid out: the root and each of `tops`,
/// and every node within one laid out in its box.
fn reached(flowing: &[Vec<usize>], tops: &[usize]) -> Vec<bool> {
    let mut reached = vec![false; flowing.len()];
    let mut stack: Vec<usize> = tops.to_vec();
    stack.push(0);
    while let Some(at) = stack.pop() {
        let Some(node) = reached.get_mut(at) else {
            continue;
        };
        *node = true;
        stack.extend_from_slice(&flowing[at]);
    }

    reached
}

/// Where each of `flat`, laid out as `layouts` with its children laid out in its box `flowing` and `tops` rooted on
/// their own, stands at `scale`: its boxes, from the screen's top left, and each scroll's pitch, the items a window
/// did not build counted in what it reaches.
fn boxed(flat: &[Flat<'_>], tops: &[usize], flowing: &[Vec<usize>], layouts: &[Layout], scale: f32) -> Laidout {
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
        for child in &flat[at].children {
            origins[*child] = origin;
        }
        let rect = Rect2::from_origin_size(origin, size_of(layout));
        let (inset_min, inset_max) = insets(layout);
        let inner_min = rect.min + inset_min;
        let inner_max = rect.max - inset_max;
        let content =
            Rect2 { min: inner_min, max: Vec2::new(inner_max.x.max(inner_min.x), inner_max.y.max(inner_min.y)) };
        let mut extent = Vec2::ZERO;
        for child in &flowing[at] {
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
fn in_flow(node: &Flat<'_>, flat: &[Flat<'_>]) -> Vec<usize> {
    match node.node.widget().arrange() {
        Arrange::Popup(_) => {
            let anchor = anchor(&node.path);
            node.children.iter().copied().filter(|child| flat[*child].path == anchor).collect()
        }
        Arrange::Flex(_) | Arrange::Grid | Arrange::Stack | Arrange::Scroll(_) | Arrange::Leaf => node.children.clone(),
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
    for child in &node.children {
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
/// box, its style as Taffy reads it, and what Taffy keeps of it.
struct Laying<'flat> {
    arrange: Arrange,
    flat: &'flat Flat<'flat>,
    children: Vec<usize>,
    style: taffy::Style,
    kept: Kept,
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
        &self.node(node).style
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
                    measuring.at(&laying.flat.path, Asker::Layout);
                    let measured = |known: Size<Option<f32>>, available: Size<AvailableSpace>| {
                        let offer = Offer {
                            width: offered(known.width, available.width),
                            height: offered(known.height, available.height),
                        };
                        let content = laying.flat.node.widget().measure(laying.flat.node.style(), measuring, offer);

                        Size { width: content.x, height: content.y }
                    };
                    compute_leaf_layout(inputs, &laying.style, |_, _| 0.0, measured)
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
        &self.node(node).style
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
        let _ = self.kept(node).cache.clear();
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
        &self.node(node).style
    }

    fn get_flexbox_child_style(&self, child: NodeId) -> &taffy::Style {
        &self.node(child).style
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
        &self.node(node).style
    }

    fn get_grid_child_style(&self, child: NodeId) -> &taffy::Style {
        &self.node(child).style
    }
}
