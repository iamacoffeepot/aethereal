//! What an element of markup builds where it stands (ADR-0056: Your own widgets): a primitive, its own node; a
//! composite, the tree its `widget!` writes from its params. A screen's `view!` writes every element through it, so it
//! never needs to know which kind a type is, and rewriting a composite as a primitive changes no screen that places it.
//!
//! An element given `items` asks its type whether it windows them, builds only the rows its screen says it shows, as
//! [`Widget::WINDOWS`] declares, so a program's own scrolling primitive is windowed as `Scroll` is:
//!
//! ```text
//! Scroll { .., items: self.wares as ware key ware.id, … }
//!     ──> let widget = Scroll { ..Default::default() };
//!         if windows(&widget) { (widget.window(lines), the rows the screen says it shows) }   // folded: Scroll windows
//!         else { (widget, every item) }
//! ```

use super::{Node, Segment};
use crate::style::Style;
use crate::widget::{Lines, Widget};

/// An element of a view's markup: a widget, or a composite that builds its own tree.
pub trait Element {
    /// Whether it windows the items markup's `items` gives it, building only those its screen says it shows: a
    /// primitive's [`Widget::WINDOWS`]. A composite never does, as it builds every item it is given.
    const WINDOWS: bool = false;

    /// Its node at `segment`, styled as its own markup styles it, the default style for a primitive, over `children`.
    fn node(self, segment: Segment, children: Vec<Node>) -> Node;

    /// Its node at `segment`, styled `style`, over `children`: a composite's root styled where it is placed.
    fn styled(self, segment: Segment, style: Style, children: Vec<Node>) -> Node;

    /// It, its children the items of a list at `lines`, as its screen answered the window: a primitive's
    /// [`Widget::window`]. A composite, which never windows, is unchanged.
    #[must_use]
    fn window(self, lines: Lines) -> Self
    where
        Self: Sized,
    {
        let _ = lines;

        self
    }
}

/// A primitive is its own node, and windows its items as it declares.
impl<W: Widget> Element for W {
    const WINDOWS: bool = <W as Widget>::WINDOWS;

    fn node(self, segment: Segment, children: Vec<Node>) -> Node {
        Node::new(segment, self, Style::default(), children)
    }

    fn styled(self, segment: Segment, style: Style, children: Vec<Node>) -> Node {
        Node::new(segment, self, style, children)
    }

    fn window(self, lines: Lines) -> Self {
        Widget::window(self, lines)
    }
}

/// Whether `element`'s type windows the items markup gives it: what a screen's `view!` asks of an element with
/// `items`, by its value, as markup names a generic widget's type without its parameters. It is the type's constant, so
/// the compiler folds the branch it chooses.
#[must_use]
pub const fn windows<T: Element>(_: &T) -> bool {
    T::WINDOWS
}

/// `element`, whose items a composite's markup builds whole: refused as the composite is built for a screen when its
/// type windows its items, as a composite has no screen to ask which rows it shows.
#[must_use]
pub const fn whole<T: Element>(element: T) -> T {
    const {
        assert!(
            !T::WINDOWS,
            "a composite's list cannot be windowed: it has no screen to ask which rows it shows; place the list in \
             the screen's view, or build it whole in a `Column`"
        );
    }

    element
}
