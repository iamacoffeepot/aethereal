//! What an element of markup builds where it stands (ADR-0056: Your own widgets): a primitive, its own node; a
//! composite, the tree its `widget!` writes from its params. A screen's `view!` writes every element through it, so it
//! never needs to know which kind a type is, and rewriting a composite as a primitive changes no screen that places it.

use super::{Node, Segment};
use crate::style::Style;
use crate::widget::Widget;

/// An element of a view's markup: a widget, or a composite that builds its own tree.
pub trait Element {
    /// Its node at `segment`, styled as its own markup styles it, the default style for a primitive, over `children`.
    fn node(self, segment: Segment, children: Vec<Node>) -> Node;

    /// Its node at `segment`, styled `style`, over `children`: a composite's root styled where it is placed.
    fn styled(self, segment: Segment, style: Style, children: Vec<Node>) -> Node;
}

/// A primitive is its own node.
impl<W: Widget> Element for W {
    fn node(self, segment: Segment, children: Vec<Node>) -> Node {
        Node::new(segment, self, Style::default(), children)
    }

    fn styled(self, segment: Segment, style: Style, children: Vec<Node>) -> Node {
        Node::new(segment, self, style, children)
    }
}
