//! A child actor a screen places (ADR-0056: Child actors): a panel with a life of its own, its own data source, rate of
//! change, or code, which the screen spawns and holds in Rust, and which its markup places in a slot of its tree. The
//! slot takes its place in layout as a widget does, sized by its style, and the library lists a call of the child at
//! the slot's content box, at the slot's place in draw order, in the area the markup names. Whenever the size of the
//! slot's content box, or the screen's frame or UI scale, changes, the library mails the child a [`Resize`], and a
//! child that is a screen lays out to it; moving the slot sends nothing.
//!
//! ```text
//! child { actor: self.minimap, area: "inherit", style: minimap_slot }
//!     ──> Node::call(Segment::Place(2), self.minimap.clone(), CallArea::Inherit, minimap_slot())
//!     ──> layout: a leaf, sized by its style ──> Entry::Call at its content box, in draw order
//!                                           └──> a size not told before? ──> Resize, mailed to the child
//! ```

use crate::screen::Viewport;
use crate::style::Style;
use crate::tree::{Node, Segment};
use crate::widget::{Arrange, Built, Draws, Laid, Stateless, Widget};
use aethereal_actor::{ActorRef, Child, ChildOf, Conforms, ProtocolRef};
use aethereal_data::{Bound, Plain};
use aethereal_math::Rect2;
use aethereal_render::draw_list::{Area, Callee, Clip};

/// What a screen mails each child it places when the child's size changes: the child's viewport, its own size the
/// slot's content box in pixels, and its frame and UI scale the screen's. A child that is a screen lays out to it, as
/// [`screen!`](crate::screen!) writes its handler, the way a root screen lays out to the window's size.
#[derive(Debug, Clone, Copy, PartialEq, Plain)]
#[kind(name = "aether.ui.resize")]
pub struct Resize {
    pub viewport: Viewport,
}

/// What a child a screen places is reached by: an actor that takes [`Resize`].
pub type Resizes = (Resize,);

/// A child actor a screen places in a slot of its tree, as its markup's `actor: self.minimap` reads it: where the
/// renderer finds the child beneath the screen, which a call names, and the reference its size is mailed through. The
/// screen makes it once, from the child it spawned, and keeps it in its state; the view copies it into the slot each
/// time it builds, so a tree whose children stand as they stood compares equal and is skipped.
///
/// ```
/// use aethereal_actor::{Actor, Beginning, ChildOf, Fault, Top, child};
/// use aethereal_data::{Bound, Plain};
/// use aethereal_render::frame::Frame;
/// use aethereal_render::pass::Interface;
/// use aethereal_ui::prelude::*;
/// # use aethereal_actor::{Asks, Ending, Path, Turn};
/// # use aethereal_render::Renderer;
/// # use aethereal_render::frame::ShapeOf;
/// #
/// # /// What each screen's `begin` sends: the ask for the shape of its pass.
/// # type Begins = (Asks<Path<Renderer>, ShapeOf, Shaping>,);
///
/// /// A minimap, a screen of its own: it lays out to the size the HUD mails it.
/// #[derive(Bound)]
/// #[kind(name = "aether.example.minimap")]
/// pub struct Minimap {
///     ui: Screen,
/// }
///
/// impl ChildOf for Minimap {
///     type Parent = Hud;
/// }
///
/// /// A HUD that places the minimap in a slot of its tree.
/// #[derive(Bound)]
/// #[kind(name = "aether.example.hud")]
/// pub struct Hud {
///     minimap: Called,
///     ui: Screen,
/// }
///
/// /// The HUD's minimap was not spawned, so the HUD does not begin.
/// #[derive(Plain)]
/// #[kind(name = "aether.example.no_minimap")]
/// pub struct NoMinimap;
///
/// impl View for Hud {
///     type Event = ();
///
///     /// What `child { actor: self.minimap, area: "inherit" }` compiles into, in a column.
///     fn view(&self) -> Tree {
///         let slot = Style { width: Length::Pixels(120.0), height: Length::Pixels(120.0), ..Style::default() };
///         let minimap = Node::call(Segment::Place(0), self.minimap.clone(), CallArea::Inherit, slot);
///
///         Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![minimap]))
///     }
/// #
/// #     fn on(&mut self, turn: &mut Turn<'_, Self, Self::Sends>, raised: Raised<()>) -> Result<(), Fault> {
/// #         aethereal_ui::unmatched(turn, &raised)
/// #     }
/// }
///
/// impl Actor for Hud {
///     type Config = Frame;
///     type Key = ();
///     type Handles = aethereal_ui::handles![];
///     type BeginSends = Begins;
///     type EndSends = ();
///
///     fn begin(frame: Frame, turn: &mut Beginning<'_, Self, Begins>) -> Result<Self, Fault> {
///         let Ok(spawned) = turn.spawn::<Minimap>(&(), frame) else {
///             return Err(Fault::new(&NoMinimap)?);
///         };
///         let minimap = Called::new(child::<Minimap>(()), spawned);
///         let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;
///
///         Ok(Self { minimap, ui })
///     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// }
/// #
/// # impl Actor for Minimap {
/// #     type Config = Frame;
/// #     type Key = ();
/// #     type Handles = aethereal_ui::handles![];
/// #     type BeginSends = Begins;
/// #     type EndSends = ();
/// #
/// #     fn begin(frame: Frame, turn: &mut Beginning<'_, Self, Begins>) -> Result<Self, Fault> {
/// #         let ui = Screen::begin(Target::of::<Interface>(frame), Viewport::window(1, 1, 1.0), turn)?;
/// #         Ok(Self { ui })
/// #     }
/// #
/// #     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
/// #         Ok(())
/// #     }
/// # }
/// #
/// # impl View for Minimap {
/// #     type Event = ();
/// #
/// #     fn view(&self) -> Tree {
/// #         Tree::new(Node::new(Segment::Place(0), Stack, Style::default(), vec![]))
/// #     }
/// #
/// #     fn on(&mut self, turn: &mut Turn<'_, Self, Self::Sends>, raised: Raised<()>) -> Result<(), Fault> {
/// #         aethereal_ui::unmatched(turn, &raised)
/// #     }
/// # }
/// #
/// # impl Controls for Hud {
/// #     type Sends = aethereal_ui::sends![];
/// # }
/// #
/// # impl Controls for Minimap {
/// #     type Sends = aethereal_ui::sends![];
/// # }
/// #
/// # impl ChildOf for Hud {
/// #     type Parent = Top;
/// # }
///
/// aethereal_ui::screen!(Hud, ui);
/// aethereal_ui::screen!(Minimap, ui);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Bound)]
pub struct Called {
    callee: Callee,
    sized: ProtocolRef<Resizes>,
}

impl Called {
    /// The child `reference` refers to, which stands at `child` beneath the screen, as [`child`](aethereal_actor::child)
    /// names it with the key the screen spawned it under: it builds only for a child of the screen's own that takes
    /// [`Resize`].
    #[must_use]
    pub fn new<C, Is>(child: Child<C>, reference: ActorRef<C>) -> Self
    where
        C: ChildOf + Conforms<Resizes, Is>,
    {
        Self { callee: Callee::of(child), sized: reference.protocol() }
    }
}

/// The coordinates a child placed in a slot draws in, as the markup's `area` names them (ADR-0055).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CallArea {
    /// The screen's, moved to the slot's content box, its `0, 0` at the box's top left: the child moves with the screen
    /// and every scroll around the slot, and a move sends it nothing.
    #[default]
    Inherit,
    /// The window's pixels, for a child that draws where it is in the window, as a scene does: the slot's box is taken
    /// to be in the window's pixels, as a screen's are when it is a root or is itself called in the window's pixels, and
    /// the child is told its size, never where the slot is.
    Absolute,
}

impl Node {
    /// The slot at `segment` among its siblings, styled `style`, where the child `called` is placed in the area `area`
    /// names: what the markup's `child` compiles into. It has no children, and is sized by its style alone.
    #[must_use]
    pub fn call(segment: Segment, called: Called, area: CallArea, style: Style) -> Self {
        Self::new(segment, Calling { called, area }, style, Vec::new())
    }
}

/// The widget of a child's slot: the child, and the area it is placed in. It draws nothing of its own, and is reached
/// by the pointer only where its style fills its box, as a panel is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Calling {
    called: Called,
    area: CallArea,
}

impl Widget for Calling {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}
}

impl Calling {
    /// The child it places.
    pub const fn callee(&self) -> &Callee {
        &self.called.callee
    }

    /// The reference its child is told its size through.
    pub const fn sized(&self) -> ProtocolRef<Resizes> {
        self.called.sized
    }

    /// Where its child is placed, the slot laid at `laid` in the screen's pixels: at its content box, cut to the box
    /// within what the slot is cut to, so the child is cut by every scroll around the slot.
    pub fn area(&self, laid: &Laid) -> Area {
        let clip = Clip::To(cut(laid));
        match self.area {
            CallArea::Inherit => Area::Inherit { origin: laid.content.min, clip },
            CallArea::Absolute => Area::Absolute { clip },
        }
    }
}

/// What the child placed in a slot laid at `laid` is cut to, in the screen's pixels: the slot's content box, within
/// what the slot is cut to.
pub fn cut(laid: &Laid) -> Rect2 {
    laid.content.intersection(laid.clip)
}

/// `widget` as a child's slot, if it is one.
pub fn calling(widget: &dyn Built) -> Option<&Calling> {
    widget.as_any().downcast_ref::<Calling>()
}
