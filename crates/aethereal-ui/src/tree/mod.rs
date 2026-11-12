//! The tree a screen's view builds (ADR-0056): each node a widget with its settings, its resolved style, and its
//! children in layout and draw order, a later child over an earlier one. A view builds it whole on every `Prepare`,
//! and it is compared with the last by value, so nothing marks what changed.
//!
//! Each node is told apart from its siblings by its [`Segment`]: its place among its parent's children in the markup,
//! which stays the same while a sibling before it is hidden, so a widget keeps its key, and its state, whatever else
//! shows. A widget's identity is its [`KeyPath`], the segments from the root to it.

#[cfg(test)]
mod tests;

use crate::style::Style;
use crate::widget::{Built, Widget};
use aethereal_data::Plain;
use core::fmt;

/// How deep a tree may be, the root at one: a view is written by hand, as markup, and stands far shallower.
pub const MOST_DEPTH: usize = 64;

/// One step of a key path: how a node is told apart from its siblings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Plain)]
pub enum Segment {
    /// The node at its place among its parent's children in the markup, counted from zero: its place in the source,
    /// not among the siblings shown, so a sibling hidden before it moves no key.
    Place(u32),
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
}

/// One node of a tree: a widget, its style, and its children, told apart from its siblings by its segment.
pub struct Node {
    segment: Segment,
    widget: Box<dyn Built>,
    style: Style,
    children: Vec<Self>,
}

impl Node {
    /// The widget `widget` at `segment` among its siblings, styled `style`, over `children` in layout and draw order.
    #[must_use]
    pub fn new<W: Widget>(segment: Segment, widget: W, style: Style, children: Vec<Self>) -> Self {
        Self { segment, widget: Box::new(widget), style, children }
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

        segment && style && widget && children
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
    /// children in the list: walked with a stack of its own, so a tree's depth costs no frames.
    ///
    /// # Errors
    ///
    /// [`TreeRefused::Deep`] for a node past [`MOST_DEPTH`], and [`TreeRefused::Twice`] for two siblings of one segment,
    /// which would share a key.
    pub(crate) fn flat(&self) -> Result<Vec<Flat<'_>>, TreeRefused> {
        let mut flat: Vec<Flat<'_>> = Vec::new();
        let root = KeyPath::of(&[self.root.segment]);
        let mut stack = vec![(&self.root, root, None::<usize>)];
        while let Some((node, path, parent)) = stack.pop() {
            let deep = path.segments.len() > MOST_DEPTH;
            if deep {
                return Err(TreeRefused::Deep { path });
            }
            let at = flat.len();
            if let Some(parent) = parent {
                flat[parent].children.push(at);
            }
            let mut segments: Vec<Segment> = node.children.iter().map(|child| child.segment).collect();
            segments.sort_unstable();
            let twice = segments.windows(2).find(|pair| pair[0] == pair[1]);
            if let Some(pair) = twice {
                return Err(TreeRefused::Twice { path: path.child(pair[0]) });
            }
            for child in node.children.iter().rev() {
                stack.push((child, path.child(child.segment), Some(at)));
            }
            flat.push(Flat { path, node, children: Vec::new() });
        }

        Ok(flat)
    }
}

/// One node of a tree as it is laid out and drawn: its path, the node, and the places of its children in the list.
pub struct Flat<'tree> {
    pub path: KeyPath,
    pub node: &'tree Node,
    pub children: Vec<usize>,
}

/// Why a tree was not laid out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeRefused {
    /// A node stands deeper than [`MOST_DEPTH`].
    Deep { path: KeyPath },
    /// Two siblings share the segment that ends `path`, so they would share a key.
    Twice { path: KeyPath },
}
