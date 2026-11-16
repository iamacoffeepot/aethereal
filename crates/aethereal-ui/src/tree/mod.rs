//! The tree a screen's view builds (ADR-0056): each node a widget with its settings, its resolved style, and its
//! children in layout and draw order, a later child over an earlier one. A view builds it whole on every `Prepare`,
//! and it is compared with the last by value, so nothing marks what changed.
//!
//! Each node is told apart from its siblings by its [`Segment`]: its place among its parent's children in the markup,
//! which stays the same while a sibling before it is hidden, so a widget keeps its key, and its state, whatever else
//! shows; or, for a child a widget's `items` repeats, the key of its item, so its state follows the item as the list
//! reorders and goes with it (ADR-0056: Lists and conditions are settings). A widget's identity is its [`KeyPath`], the
//! segments from the root to it.

mod compare;
mod element;
mod memo;
mod shape;
#[cfg(test)]
mod tests;

pub use compare::{Difference, Relaid};
pub use element::{Element, whole, windows};
pub use memo::{Composite, Look, same};
pub use memo::{Inputs, Memos};
pub use shape::{Reshaped, Shape, Was};

use crate::drag::{DragSource, DropTarget};
use crate::style::Style;
use crate::widget::{Built, Widget};
use crate::widgets::Stack;
use aethereal_data::{Bare, Boundary, Decode, DecodeError, Encode, EncodeError, Plain, Reader, Schema, Writer};
use core::borrow::Borrow;
use core::fmt;
use core::ops::Deref;
use std::sync::Arc;

/// How deep a tree may be, the root at one: a view is written by hand, as markup, and stands far shallower.
pub const MOST_DEPTH: usize = 64;

/// One step of a key path: how a node is told apart from its siblings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Plain)]
pub enum Segment {
    /// The node at its place among its parent's children in the markup, counted from zero: its place in the source,
    /// not among the siblings shown, so a sibling hidden before it moves no key.
    Place(u32),
    /// The node a widget's `items` built for the item of this key, wherever the item stands in its list.
    Item(ItemKey),
}

/// What tells one item of a list from the others, as `key item.id` names it: a field of the item, `Copy` and
/// compared by value, turned into the number its widget state is kept under. A type used as a key converts with
/// `From`, which the library gives every integer and a program writes for its own ids, and two items it tells apart
/// must convert to two keys: two items of one list with one key are one key to the library, and the later is left out
/// and logged, never merged with the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Plain)]
pub struct ItemKey {
    key: u64,
}

impl ItemKey {
    /// The key numbered `key`.
    #[must_use]
    pub const fn new(key: u64) -> Self {
        Self { key }
    }
}

/// An unsigned integer is its own key.
macro_rules! unsigned_keys {
    ($($integer:ty),*) => {
        $(impl From<$integer> for ItemKey {
            fn from(key: $integer) -> Self {
                Self::new(u64::from(key))
            }
        })*
    };
}

unsigned_keys!(u8, u16, u32, u64);

/// A signed integer is keyed by its bits, so every value is a key of its own.
macro_rules! signed_keys {
    ($($integer:ty => $unsigned:ty),*) => {
        $(impl From<$integer> for ItemKey {
            fn from(key: $integer) -> Self {
                Self::new(u64::from(<$unsigned>::from_ne_bytes(key.to_ne_bytes())))
            }
        })*
    };
}

signed_keys!(i8 => u8, i16 => u16, i32 => u32, i64 => u64);

impl From<usize> for ItemKey {
    fn from(key: usize) -> Self {
        // A `usize` is at most 64 bits on every target this builds for.
        Self::new(u64::try_from(key).unwrap_or(u64::MAX))
    }
}

/// Where a widget stands in its screen's tree: the segments from the root to it, the root's first. It is the
/// widget's identity: its state is kept under it (ADR-0056: Widget state is kept by key), and so are its draws.
///
/// Its segments are shared by every copy of it, so a path is copied without copying them: a screen copies a widget's
/// path into everything it keeps of the widget, and that costs a count, not an allocation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Plain)]
pub struct KeyPath {
    segments: Shared,
}

/// A key path's segments, shared by its copies, and written as the sequence of them they are.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
struct Shared(Arc<[Segment]>);

impl Deref for Shared {
    type Target = [Segment];

    fn deref(&self) -> &[Segment] {
        &self.0
    }
}

impl Schema for Shared {
    const SHAPE: aethereal_data::Shape = <Vec<Segment> as Schema>::SHAPE;
}

impl Plain for Shared where Vec<Segment>: Encode<Bare> + Decode<Bare> {}

impl<B: Boundary> Encode<B> for Shared
where
    Vec<Segment>: Encode<B>,
{
    fn encode(&self, out: &mut Writer<'_, B>) -> Result<(), EncodeError> {
        self.0.to_vec().encode(out)
    }
}

impl<B: Boundary> Decode<B> for Shared
where
    Vec<Segment>: Decode<B>,
{
    fn decode(input: &mut Reader<'_, B>) -> Result<Self, DecodeError> {
        Vec::<Segment>::decode(input).map(|segments| Self(segments.into()))
    }
}

impl KeyPath {
    /// The path of `segments`, the root's first.
    #[must_use]
    pub fn of(segments: &[Segment]) -> Self {
        Self { segments: Shared(segments.into()) }
    }

    /// The path of a child of this one, told apart from its siblings by `segment`.
    #[must_use]
    pub fn child(&self, segment: Segment) -> Self {
        let segments: Arc<[Segment]> = self.segments.iter().copied().chain([segment]).collect();

        Self { segments: Shared(segments) }
    }

    /// Its segments, the root's first.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Whether `other` stands within the widget at this path, as one of its descendants.
    #[must_use]
    pub fn holds(&self, other: &Self) -> bool {
        let deeper = other.segments.len() > self.segments.len();

        deeper && other.segments.starts_with(&self.segments)
    }

    /// The paths of the widgets this one stands within, its parent's first and the root's last.
    pub fn ancestors(&self) -> impl Iterator<Item = Self> + '_ {
        (1..self.segments.len()).rev().map(|depth| Self::of(&self.segments[..depth]))
    }

    /// The key of the item of the list at `list` that this path stands at or within, if it is one of that list's items
    /// or within one.
    #[must_use]
    pub fn item_of(&self, list: &Self) -> Option<ItemKey> {
        let within = list.holds(self);
        if !within {
            return None;
        }
        match self.segments[list.segments.len()] {
            Segment::Item(key) => Some(key),
            Segment::Place(_) => None,
        }
    }
}

/// A path is its segments, so what is kept by path is found by a run of segments without making a path of them.
impl Borrow<[Segment]> for KeyPath {
    fn borrow(&self) -> &[Segment] {
        &self.segments.0
    }
}

/// One node of a tree: a widget, its style, and its children, told apart from its siblings by its segment; and what
/// its markup's `drag` and `drop` make of it, a widget dragged and one dropped on (ADR-0056: Drag and drop is one path).
pub struct Node {
    segment: Segment,
    widget: Box<dyn Built>,
    style: Style,
    children: Vec<Self>,
    /// What a press and a move drag from it, fixed when the view builds it: none for a widget the markup gives no
    /// `drag`.
    drag: Option<DragSource>,
    /// What it takes dropped on it, fixed when the view builds it: none for a widget the markup gives no `drop`.
    drop: Option<DropTarget>,
    memo: Memo,
    stamps: Stamps,
}

/// The numbers of the builds of its screen's tree at which a node last laid out otherwise than the node before it at
/// its key path, and last drew otherwise, each the build it entered the tree at when it has not changed since: what a
/// screen kept of the node from a layout of an earlier build holds while these are no later than that build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamps {
    pub laid: u64,
    pub drawn: u64,
}

impl Stamps {
    /// A node no merge has taken in: later than every build, so nothing kept of an earlier one holds for it.
    const UNMERGED: Self = Self { laid: u64::MAX, drawn: u64::MAX };

    /// A node that changed at `build`, or entered the tree then.
    pub(crate) const fn at(build: u64) -> Self {
        Self { laid: build, drawn: build }
    }
}

/// Where a node stands with what its screen's view keeps of the composites marked `memo`.
#[derive(Debug)]
enum Memo {
    /// It is built at every view.
    Plain,
    /// It is the root of a composite marked `memo`, built at this view from these inputs.
    Built(Inputs),
    /// It is the root of a composite marked `memo`, its inputs kept by its screen.
    Root,
    /// It stands in place of a composite marked `memo` whose inputs stood: the last tree's node at its path is taken
    /// whole as the tree is merged.
    Kept,
}

impl Node {
    /// The widget `widget` at `segment` among its siblings, styled `style`, over `children` in layout and draw order.
    #[must_use]
    pub fn new<W: Widget>(segment: Segment, widget: W, style: Style, children: Vec<Self>) -> Self {
        let stamps = Stamps::UNMERGED;

        Self { segment, widget: Box::new(widget), style, children, drag: None, drop: None, memo: Memo::Plain, stamps }
    }

    /// The builds at which it last laid out otherwise and drew otherwise.
    pub(crate) const fn stamps(&self) -> Stamps {
        self.stamps
    }

    /// The marker of a composite at `segment` whose inputs stood: it draws nothing, and the last tree's node at its
    /// path takes its place as the tree is merged.
    fn kept(segment: Segment) -> Self {
        Self { memo: Memo::Kept, ..Self::new(segment, Stack, Style::default(), Vec::new()) }
    }

    /// Whether it is the marker of a kept composite.
    #[cfg(test)]
    pub(crate) const fn is_kept(&self) -> bool {
        matches!(self.memo, Memo::Kept)
    }

    /// It, the root of a composite marked `memo`, built from `inputs`.
    fn memoed(self, inputs: Inputs) -> Self {
        Self { memo: Memo::Built(inputs), ..self }
    }

    /// It, dragged as `source` says: what the markup's `drag`, and its `drop_outside`, compile into.
    #[must_use]
    pub fn with_drag(self, source: DragSource) -> Self {
        Self { drag: Some(source), ..self }
    }

    /// It, taking drops as `target` says: what the markup's `drop` compiles into.
    #[must_use]
    pub fn with_drop(self, target: DropTarget) -> Self {
        Self { drop: Some(target), ..self }
    }

    /// What a press and a move drag from it, if it is a source.
    pub(crate) const fn drag(&self) -> Option<&DragSource> {
        self.drag.as_ref()
    }

    /// What it takes dropped on it, if it is a target.
    pub(crate) const fn drop_target(&self) -> Option<&DropTarget> {
        self.drop.as_ref()
    }

    /// The widget, as built.
    pub(crate) fn widget(&self) -> &dyn Built {
        &*self.widget
    }

    /// Its style, resolved from the style its markup names.
    pub(crate) const fn style(&self) -> &Style {
        &self.style
    }

    /// Its child told apart by `segment`, if it has one.
    fn child(&self, segment: Segment) -> Option<&Self> {
        self.children.iter().find(|child| child.segment == segment)
    }
}

impl PartialEq for Node {
    fn eq(&self, other: &Self) -> bool {
        let segment = self.segment == other.segment;
        let style = self.style == other.style;
        let widget = self.widget.same(&*other.widget);
        let children = self.children == other.children;
        let drag = self.drag == other.drag;
        let drop = self.drop == other.drop;

        segment && style && widget && children && drag && drop
    }
}

impl fmt::Debug for Node {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Node")
            .field("segment", &self.segment)
            .field("widget", &self.widget)
            .field("style", &self.style)
            .field("children", &self.children)
            .field("drag", &self.drag)
            .field("drop", &self.drop)
            .finish_non_exhaustive()
    }
}

/// A screen's tree, from its root.
#[derive(Debug, PartialEq)]
pub struct Tree {
    root: Node,
}

impl Tree {
    /// The tree whose root is `root`.
    #[must_use]
    pub const fn new(root: Node) -> Self {
        Self { root }
    }

    /// The node at `path`, if the tree has one there: found a segment at a time, without a frame for each.
    pub(crate) fn find(&self, path: &KeyPath) -> Option<&Node> {
        let (first, rest) = path.segments.split_first()?;
        let rooted = *first == self.root.segment;
        if !rooted {
            return None;
        }

        rest.iter().try_fold(&self.root, |node, segment| node.child(*segment))
    }
}

/// A tree's nodes in tree order, and the items left out because a sibling before each had its key; where each node
/// stood in the last flattening, whether any stands otherwise, and the number of this flattening.
pub struct Flattened<'tree> {
    pub nodes: Vec<Flat<'tree>>,
    pub twice: Vec<KeyPath>,
    pub was: Vec<Was>,
    pub reshaped: Reshaped,
    pub generation: u64,
}

/// One node of a tree as it is laid out and drawn: its path, the node, its place among its parent's children as the
/// view built them, and the places of its children in the list.
pub struct Flat<'tree> {
    pub path: &'tree KeyPath,
    pub node: &'tree Node,
    pub born: usize,
    pub children: &'tree [usize],
}

/// Why a tree was not laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRefused {
    /// A node stands deeper than [`MOST_DEPTH`].
    Deep { path: KeyPath },
    /// Two siblings stand at the place in the markup that ends `path`, so they would share a key.
    Twice { path: KeyPath },
}
