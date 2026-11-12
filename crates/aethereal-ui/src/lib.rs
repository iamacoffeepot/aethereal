//! The widget library (ADR-0056): the runtime of a screen, which is an actor whose view builds a tree of widgets. It
//! is plain code inside the screen's actor and adds nothing to the actor framework: it subscribes the screen to
//! `Prepare`, lays its tree out with Taffy, draws through the screen's own draw list, with the renderer's meshes and
//! texts, and takes in the input that list reaches (ADR-0020, ADR-0055, ADR-0052).
//!
//! ```text
//! Prepare ──> view: Tree ──> same as the last, nothing outside it changed? ──> nothing sent
//!                                     └─> otherwise: widget table ──> Taffy ──> draws, hits, keys ──> what changed
//! Pointed / Keyed ──> widget by entry ──> its state, the focus ──> raised events ──> View::on ──> the screen's method
//!                                                              └─> keys left over ──> Controls::key
//! ```
//!
//! - [`Tree`] and [`Node`]: what a view builds, each node a widget, its style, and its children, compared by value; a
//!   child a widget's `items` repeats is told apart by its [`ItemKey`], and a list in a
//!   [`Scroll`](widgets::Scroll) builds only the items its [`Rows`] say it shows.
//! - [`Widget`]: a primitive. It arranges its children, measures itself as a leaf, draws itself, and takes in input;
//!   its state is kept by key in the screen's [`Table`], with the screen's focus.
//! - [`Style`]: every property a sheet compiles into, resolved.
//! - [`View`] and [`Controls`]: what a screen's markup compiles into, its event enum, its view, and the call of each
//!   event's method; and what is written by hand beside those methods, its sends and its hotkeys.
//! - [`Screen`]: what the library keeps in the screen's actor, and what it does in the screen's turns, as
//!   [`screen!`] writes the screen's handlers: among them, the content of each open widget that arranges its children
//!   as a popup, a [`Popup`](widgets::Popup) or a program's own, drawn in the screen's top layer, and the wait of each
//!   opened by the pointer resting, a [`Tooltip`](widgets::Tooltip), a `Sleep` job.
//! - [`Called`] and [`Node::call`]: a child actor the screen spawns and holds, placed in a slot of its tree, called
//!   there in the area [`CallArea`] names, and mailed its size, a [`Resize`], whenever it changes; a child that is a
//!   screen lays out to it.
//!
//! A screen is written as its markup will compile it:
//!
//! ```
//! use aethereal_actor::{Actor, Beginning, ChildOf, Ending, Fault, Top, Turn};
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
//! /// What `hud.xml` compiles into: `<Button click="{self.spend(5)}">`.
//! #[derive(Debug, Clone, Copy, PartialEq)]
//! pub enum HudEvent {
//!     Spend(u32),
//! }
//!
//! impl View for Hud {
//!     type Event = HudEvent;
//!
//!     fn view(&self) -> Tree {
//!         let gold = Node::new(Segment::Place(0), Text::new(&format!("Gold: {}", self.gold)), Style::default(), vec![]);
//!         let spend = Node::new(Segment::Place(1), Button::new(HudEvent::Spend(5)), Style::default(), vec![]);
//!
//!         Tree::new(Node::new(Segment::Place(0), Column, Style::default(), vec![gold, spend]))
//!     }
//!
//!     fn on(&mut self, turn: &mut Turn<'_, Self, HudSends>, raised: Raised<HudEvent>) -> Result<(), Fault> {
//!         match raised {
//!             Raised::Event(HudEvent::Spend(amount)) => self.spend(turn, amount),
//!             unmatched => aethereal_ui::unmatched(turn, &unmatched),
//!         }
//!     }
//! }
//!
//! /// What the HUD's methods send: none of their own.
//! type HudSends = aethereal_ui::sends![];
//!
//! impl Controls for Hud {
//!     type Sends = HudSends;
//! }
//!
//! impl Hud {
//!     fn spend(&mut self, _: &mut Turn<'_, Self, HudSends>, amount: u32) -> Result<(), Fault> {
//!         self.gold = self.gold.saturating_sub(amount);
//!         Ok(())
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
//! aethereal_ui::screen!(Hud, ui);
//! ```

#![forbid(unsafe_code)]

mod child;
mod draw;
mod fonts;
mod input;
mod layout;
mod screen;
mod style;
mod table;
mod tree;
mod unkept;
mod view;
mod widget;
pub mod widgets;

pub use child::{CallArea, Called, Resize, Resizes};
pub use fonts::{Measuring, Unmeasured};
pub use input::{Erased, Event, Hears, Held, Input, Key, Pointing, Raised, Used};
pub use screen::{
    DrawKey, Heard, ImeAsked, Instancing, Meshing, MetricsAsked, Orders, Outcome, Placing, Prepared, Programming, Rows,
    Screen, Sends, Shaping, Target, TextPlacing, Texting, Tipping, Uniforming, Viewport, Whitening,
};
pub use style::{Alignment, Border, Distribute, Edges, Gap, Length, Placed, SelfAlign, Space, Style, Track};
pub use table::{Focus, Table};
pub use tree::{ItemKey, KeyPath, MOST_DEPTH, Node, Segment, Tree, TreeRefused};
pub use view::{Controls, Took, View, take_in, unmatched};
pub use widget::{
    Arrange, Axis, Draws, Inside, Laid, Layer, Lines, Offer, Offered, Opens, Quad, Sample, Side, Slices, Stateless,
    TextDraw, Tip, Widget,
};

/// What a screen's code names: the tree, the stock widgets, the style, the screen, and what its markup and its
/// controls are written as.
pub mod prelude {
    pub use crate::widgets::{
        Bar, Button, Column, Fill, Grid, Image, Popup, Region, Row, Scroll, Slider, Stack, Tab, Tabs, Text, TextInput,
        Texture, Toggle, Tooltip,
    };
    pub use crate::{
        Alignment, Border, CallArea, Called, Controls, Distribute, Edges, Gap, Inside, ItemKey, Key, KeyPath, Length,
        Node, Placed, Raised, Rows, Screen, Segment, SelfAlign, Shaping, Side, Space, Style, Target, Track, Tree, View,
        Viewport,
    };
}

/// What [`screen!`], [`handles!`], and [`sends!`] name, so a screen's crate need not.
#[doc(hidden)]
pub mod __private {
    pub use aethereal_actor::{Answer, Answered, Asks, Fault, Handles, On, Path, Prepare, Slept, Turn};
    pub use aethereal_render::frame::Shaped;
    pub use aethereal_render::instances::Instanced;
    pub use aethereal_render::mesh::Meshed;
    pub use aethereal_render::placement::Placed;
    pub use aethereal_render::program::Programmed;
    pub use aethereal_render::text::{FontMetrics, TextMade, TextPlaced};
    pub use aethereal_render::texture::Textured;
    pub use aethereal_render::uniforms::Uniformed;
    pub use aethereal_window::{AllowIme, ImeAllowed, Keyed, Pointed, Windows};
}

/// A screen's `Handles`: what the library handles, `Prepare`, the pointer and the keys its list reaches, the answer to
/// each of its asks with the ask's context, the end of a tooltip's sleep with the tooltip and its hover, and the size a
/// screen that places it as a child mails it; then each of `$own`, what the screen handles of its own.
#[macro_export]
macro_rules! handles {
    ($($own:ty),* $(,)?) => {
        (
            $crate::__private::On<$crate::__private::Prepare>,
            $crate::__private::Pointed,
            $crate::__private::Keyed,
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
            $crate::__private::Answer<$crate::__private::ImeAllowed, $crate::ImeAsked>,
            $crate::__private::Answer<$crate::__private::Slept, $crate::Tipping>,
            $crate::Resize,
            $($own,)*
        )
    };
}

/// A screen's [`Controls::Sends`]: the library's ask of the window's input method, which input may make, then each of
/// `$own`, what the screen's event methods and its `key` send of their own. A handler's sends are at most 24, so a
/// screen's own are at most 23.
#[macro_export]
macro_rules! sends {
    ($($own:ty),* $(,)?) => {
        (
            $crate::__private::Asks<
                $crate::__private::Path<$crate::__private::Windows>,
                $crate::__private::AllowIme,
                $crate::ImeAsked,
            >,
            $($own,)*
        )
    };
}

/// Writes a screen's handlers of what is the library's, for the screen `$screen`, whose [`Screen`] is in its `$field`:
/// its `Prepare`, which builds the tree with its [`View`] and hands it to the screen with its [`Controls::HOTKEYS`]; the
/// pointer and the keys its list reaches, whose raised events call its methods through [`View::on`] and whose keys
/// left over call [`Controls::key`], after which its view is built again for the next input to reach; each answer the
/// library's asks bring; the end of each tooltip's sleep; and the size a screen that places it as a child mails it,
/// which it lays out to as a root screen lays out to its window's. A screen's markup compiles into a call of it.
#[macro_export]
macro_rules! screen {
    ($screen:ty, $field:ident) => {
        impl $crate::__private::Handles<$crate::__private::On<$crate::__private::Prepare>> for $screen {
            type Sends = $crate::Sends;

            fn handle(
                &mut self,
                _: $crate::__private::Prepare,
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let tree = <Self as $crate::View>::view(self);
                let hotkeys = <Self as $crate::Controls>::HOTKEYS;
                self.$field.prepare(tree, hotkeys).orders.send(turn)
            }
        }

        impl $crate::__private::Handles<$crate::__private::Pointed> for $screen {
            type Sends = <Self as $crate::Controls>::Sends;

            fn handle(
                &mut self,
                pointed: $crate::__private::Pointed,
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let heard = self.$field.pointed(pointed, turn.me().id());
                $crate::screen!(@take $field, self, heard, turn)
            }
        }

        impl $crate::__private::Handles<$crate::__private::Keyed> for $screen {
            type Sends = <Self as $crate::Controls>::Sends;

            fn handle(
                &mut self,
                keyed: $crate::__private::Keyed,
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let heard = self.$field.keyed(keyed);
                $crate::screen!(@take $field, self, heard, turn)
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
        $crate::screen!(@answer $screen, $field, ImeAllowed, ImeAsked, ime_answered);
        $crate::screen!(@answer $screen, $field, Slept, Tipping, slept);

        impl $crate::__private::Handles<$crate::Resize> for $screen {
            type Sends = $crate::Sends;

            fn handle(
                &mut self,
                resize: $crate::Resize,
                turn: &mut $crate::__private::Turn<'_, Self, Self::Sends>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                self.$field.resize(resize.viewport).send(turn)
            }
        }
    };
    (@take $field:ident, $this:ident, $heard:ident, $turn:ident) => {{
        let took = $crate::take_in($this, $heard, $turn)?;
        if took == $crate::Took::Called {
            let tree = <Self as $crate::View>::view($this);
            $this.$field.rebuilt(tree);
        }

        Ok(())
    }};
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
