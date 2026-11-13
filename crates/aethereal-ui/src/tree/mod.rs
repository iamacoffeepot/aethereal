//! The tree a screen's view builds (ADR-0056): each node a widget with its settings, its resolved style, and its
//! children in layout and draw order, a later child over an earlier one. A view builds it whole on every `Prepare`,
//! and it is compared with the last by value, so nothing marks what changed.
//!
//! Each node is told apart from its siblings by its [`Segment`]: its place among its parent's children in the markup,
//! which stays the same while a sibling before it is hidden, so a widget keeps its key, and its state, whatever else
//! shows; or, for a child a widget's `items` repeats, the key of its item, so its state follows the item as the list
//! reorders and goes with it (ADR-0056: Lists and conditions are settings). A widget's identity is its [`KeyPath`], the
//! segments from the root to it.

#[cfg(test)]
mod tests;

use crate::drag::{DragSource, DropTarget};
use crate::style::Style;
use crate::widget::{Built, Widget};
use aethereal_data::Plain;
use core::fmt;
use std::collections::BTreeSet;

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

/// What tells one item of a list from the others, as `key="{item.id}"` names it: a field of the item, `Copy` and
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Plain)]
pub struct KeyPath {
    segments: Vec<Segment>,
}

impl KeyPath {
    /// The path of `segments`, the root's first.
    #[must_use]
    pub fn of(segments: &[Segment]) -> Self {
        Self { segments: segments.to_vec() }
    }

    /// The path of a child of this one, told apart from its siblings by `segment`.
    #[must_use]
    pub fn child(&self, segment: Segment) -> Self {
        let mut segments = self.segments.clone();
        segments.push(segment);

        Self { segments }
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
}

impl Node {
    /// The widget `widget` at `segment` among its siblings, styled `style`, over `children` in layout and draw order.
    #[must_use]
    pub fn new<W: Widget>(segment: Segment, widget: W, style: Style, children: Vec<Self>) -> Self {
        Self { segment, widget: Box::new(widget), style, children, drag: None, drop: None }
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

    /// Its style, resolved from the screen's sheets.
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
            .finish()
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

    /// Its nodes in tree order, the order they lay out and draw in, each beside its path and the places of its
    /// children in the list: walked with a stack of its own, so a tree's depth costs no frames. An item whose key a
    /// sibling before it has is left out, with its descendants, and named in [`Flattened::twice`].
    ///
    /// # Errors
    ///
    /// [`TreeRefused::Deep`] for a node past [`MOST_DEPTH`], and [`TreeRefused::Twice`] for two siblings at one place in
    /// the markup, which only a tree built by hand can hold.
    pub(crate) fn flat(&self) -> Result<Flattened<'_>, TreeRefused> {
        let mut nodes: Vec<Flat<'_>> = Vec::new();
        let mut twice = Vec::new();
        let root = KeyPath::of(&[self.root.segment]);
        let mut stack = vec![(&self.root, root, Parent::Root)];
        while let Some((node, path, parent)) = stack.pop() {
            let deep = path.segments.len() > MOST_DEPTH;
            if deep {
                return Err(TreeRefused::Deep { path });
            }
            let at = nodes.len();
            let born = match parent {
                Parent::Root => 0,
                Parent::At { at: parent, born } => {
                    nodes[parent].children.push(at);
                    born
                }
            };
            let mut seen = BTreeSet::new();
            let mut kept = Vec::with_capacity(node.children.len());
            for (born, child) in node.children.iter().enumerate() {
                let fresh = seen.insert(child.segment);
                match child.segment {
                    _ if fresh => kept.push((born, child)),
                    Segment::Place(_) => return Err(TreeRefused::Twice { path: path.child(child.segment) }),
                    Segment::Item(_) => twice.push(path.child(child.segment)),
                }
            }
            for (born, child) in kept.into_iter().rev() {
                stack.push((child, path.child(child.segment), Parent::At { at, born }));
            }
            nodes.push(Flat { path, node, born, children: Vec::new() });
        }

        Ok(Flattened { nodes, twice })
    }
}

/// Where a node being walked was met: as the root, or as a child of the node at `at` in the list, `born` its place
/// among that node's children.
#[derive(Debug, Clone, Copy)]
enum Parent {
    Root,
    At { at: usize, born: usize },
}

/// A tree's nodes in tree order, and the items left out because a sibling before each had its key.
pub struct Flattened<'tree> {
    pub nodes: Vec<Flat<'tree>>,
    pub twice: Vec<KeyPath>,
}

/// One node of a tree as it is laid out and drawn: its path, the node, its place among its parent's children as the
/// view built them, and the places of its children in the list.
pub struct Flat<'tree> {
    pub path: KeyPath,
    pub node: &'tree Node,
    pub born: usize,
    pub children: Vec<usize>,
}

/// Why a tree was not laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRefused {
    /// A node stands deeper than [`MOST_DEPTH`].
    Deep { path: KeyPath },
    /// Two siblings stand at the place in the markup that ends `path`, so they would share a key.
    Twice { path: KeyPath },
}
