//! The widget library (ADR-0056): the runtime of a screen, which is an actor whose view builds a tree of widgets. It
//! is plain code inside the screen's actor and adds nothing to the actor framework: it subscribes the screen to
//! `Prepare`, lays its tree out with Taffy, and draws through the screen's own draw list, with the renderer's meshes and
//! texts (ADR-0020, ADR-0055, ADR-0052).
//!
//! ```text
//! Prepare ──> view: Tree ──> same as the last, nothing outside it changed? ──> nothing sent
//!                                     └─> otherwise: widget table ──> Taffy ──> draws, kept by key ──> what changed
//! ```
//!
//! - [`Tree`] and [`Node`]: what a view builds, each node a widget, its style, and its children, compared by value.
//! - [`Widget`]: a primitive. It arranges its children, measures itself as a leaf, and draws itself; its state is kept
//!   by key in the screen's [`Table`].
//! - [`Style`]: every property a sheet compiles into, resolved.
//! - [`Screen`]: what the library keeps in the screen's actor, and what it does in the screen's turns, as
//!   [`screen!`] writes the screen's handlers.
//!
//! A screen is written as its markup will compile it:
//!
//! ```
//! use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top};
//! use aethereal_data::Bound;
//! use aethereal_render::frame::Frame;
//! use aethereal_render::pass::Interface;
//! use aethereal_ui::prelude::*;
//!
//! #[derive(Bound)]
//! #[kind(name = "aether.example.hud")]
//! pub struct Hud {
//!     gold: u32,
//!     ui: Screen,
//! }
//!
//! impl Hud {
//!     /// What `hud.xml` compiles into.
//!     fn view(&self) -> Tree {
//!         let gold = Node::new(Segment::Place(0), Text::new(&format!("Gold: {}", self.gold)), Style::default(), vec![]);
//!
//!         Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![gold]))
//!     }
//! }
//! #
//! # impl ChildOf for Hud {
//! #     type Parent = Top;
//! # }
//!
//! impl Actor for Hud {
//!     type Config = Frame;
//!     type Key = ();
//!     type Handles = aethereal_ui::handles![];
//!     type BeginSends = (aethereal_actor::Asks<aethereal_actor::Path<aethereal_render::Renderer>, aethereal_render::frame::ShapeOf, Shaping>,);
//!     type EndSends = ();
//!
//!     fn begin(frame: Frame, turn: &mut Beginning<'_, Self, Self::BeginSends>) -> Result<Self, Fault> {
//!         let target = Target::of::<Interface>(frame);
//!         let ui = Screen::begin(target, Viewport::window(1, 1, 1.0), turn)?;
//!
//!         Ok(Self { gold: 0, ui })
//!     }
//!
//!     fn end(self, _: &mut Ending<'_, Self, ()>) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//!
//! aethereal_ui::screen!(Hud, ui, view);
//! ```

#![forbid(unsafe_code)]

mod draw;
mod fonts;
mod layout;
mod screen;
mod style;
mod table;
mod tree;
mod unkept;
mod widget;
pub mod widgets;

pub use fonts::{Measuring, Unmeasured};
pub use screen::{
    DrawKey, Instancing, Meshing, MetricsAsked, Orders, Outcome, Placing, Prepared, Programming, Screen, Sends,
    Shaping, Target, TextPlacing, Texting, Uniforming, Viewport, Whitening,
};
pub use style::{Alignment, Border, Distribute, Edges, Gap, Length, Placed, SelfAlign, Space, Style, Track};
pub use table::Table;
pub use tree::{KeyPath, MOST_DEPTH, Node, Segment, Tree, TreeRefused};
pub use widget::{Arrange, Axis, Draws, Laid, Offer, Offered, Quad, Sample, Slices, Stateless, TextDraw, Widget};

/// What a screen's code names: the tree, the stock widgets, the style, and the screen.
pub mod prelude {
    pub use crate::widgets::{Bar, Column, Fill, Grid, Image, Region, Row, Stack, Text, Texture};
    pub use crate::{
        Alignment, Border, Distribute, Edges, Gap, Length, Node, Placed, Screen, Segment, SelfAlign, Shaping, Space,
        Style, Target, Track, Tree, Viewport,
    };
}

/// What [`screen!`] and [`handles!`] name, so a screen's crate need not.
#[doc(hidden)]
pub mod __private {
    pub use aethereal_actor::{Answer, Answered, Fault, Handles, On, Prepare, Turn};
    pub use aethereal_render::frame::Shaped;
    pub use aethereal_render::instances::Instanced;
    pub use aethereal_render::mesh::Meshed;
    pub use aethereal_render::placement::Placed;
    pub use aethereal_render::program::Programmed;
    pub use aethereal_render::text::{FontMetrics, TextMade, TextPlaced};
    pub use aethereal_render::texture::Textured;
    pub use aethereal_render::uniforms::Uniformed;
}

/// A screen's `Handles`: what the library handles, `Prepare` and the renderer's answer to each of its asks with the
/// ask's context, then each of `$own`, what the screen handles of its own.
#[macro_export]
macro_rules! handles {
    ($($own:ty),* $(,)?) => {
        (
            $crate::__private::On<$crate::__private::Prepare>,
            $crate::__private::Answer<$crate::__private::Shaped, $crate::Shaping>,
            $crate::__private::Answer<$crate::__private::Meshed, $crate::Meshing>,
            $crate::__private::Answer<$crate::__private::Programmed, $crate::Programming>,
            $crate::__private::Answer<$crate::__private::Uniformed, $crate::Uniforming>,
            $crate::__private::Answer<$crate::__private::Textured, $crate::Whitening>,
            $crate::__private::Answer<$crate::__private::FontMetrics, $crate::MetricsAsked>,
            $crate::__private::Answer<$crate::__private::Instanced, $crate::Instancing>,
            $crate::__private::Answer<$crate::__private::Placed, $crate::Placing>,
            $crate::__private::Answer<$crate::__private::TextMade, $crate::Texting>,
            $crate::__private::Answer<$crate::__private::TextPlaced, $crate::TextPlacing>,
            $($own,)*
        )
    };
}

/// Writes a screen's handlers of what is the library's: its `Prepare`, which builds the tree with the screen's `$view`
/// method and hands it to the [`Screen`] in its `$field`, and each answer the library's asks bring. A screen's markup
/// compiles into a call of it.
#[macro_export]
macro_rules! screen {
    ($screen:ty, $field:ident, $view:ident) => {
        impl $crate::__private::Handles<$crate::__private::On<$crate::__private::Prepare>> for $screen {
            type Sends = $crate::Sends;

            fn handle(
                &mut self,
                _: $crate::__private::Prepare,
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let tree = self.$view();
                self.$field.prepare(tree).orders.send(turn)
            }
        }

        impl $crate::__private::Handles<$crate::__private::Answer<$crate::__private::FontMetrics, $crate::MetricsAsked>>
            for $screen
        {
            type Sends = $crate::Sends;

            fn handle(
                &mut self,
                (answered, asked): ($crate::__private::Answered<$crate::__private::FontMetrics>, $crate::MetricsAsked),
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let orders = self.$field.metrics(answered, asked, |blob| turn.read(blob));
                orders.send(turn)
            }
        }

        $crate::screen!(@answer $screen, $field, Shaped, Shaping, shaped);
        $crate::screen!(@answer $screen, $field, Meshed, Meshing, meshed);
        $crate::screen!(@answer $screen, $field, Programmed, Programming, programmed);
        $crate::screen!(@answer $screen, $field, Uniformed, Uniforming, uniformed);
        $crate::screen!(@answer $screen, $field, Textured, Whitening, whitened);
        $crate::screen!(@answer $screen, $field, Instanced, Instancing, instanced);
        $crate::screen!(@answer $screen, $field, Placed, Placing, placed);
        $crate::screen!(@answer $screen, $field, TextMade, Texting, text_made);
        $crate::screen!(@answer $screen, $field, TextPlaced, TextPlacing, text_placed);
    };
    (@answer $screen:ty, $field:ident, $answer:ident, $context:ident, $method:ident) => {
        impl $crate::__private::Handles<$crate::__private::Answer<$crate::__private::$answer, $crate::$context>>
            for $screen
        {
            type Sends = $crate::Sends;

            fn handle(
                &mut self,
                (answered, context): ($crate::__private::Answered<$crate::__private::$answer>, $crate::$context),
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                self.$field.$method(answered, context).send(turn)
            }
        }
    };
}
