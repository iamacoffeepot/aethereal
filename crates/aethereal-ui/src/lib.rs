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
//!   child a widget's `items` repeats is told apart by its [`ItemKey`], and a list in a widget that declares it windows
//!   its items ([`Widget::WINDOWS`]), a [`Scroll`](widgets::Scroll) or a program's own, builds only the items its
//!   [`Rows`] say can show, from the item it [stands](Stand) at.
//! - [`Widget`]: a primitive. It arranges its children, measures itself as a leaf, draws itself, and takes in input;
//!   its state is kept by key in the screen's [`Table`], with the screen's focus.
//! - [`Style`]: every property a style sets, resolved.
//! - [`View`] and [`Controls`]: what a screen's markup compiles into, its event enum, its view, and the call of each
//!   event's method; and what is written by hand beside those methods, its sends and its hotkeys.
//! - [`Element`] and [`Method`]: what markup's elements and events are built and called through: an element a
//!   primitive's own node or the tree a composite builds, and an event's method called with the arguments its view
//!   copied and, when it takes one, what its widget handed, by the method's own signature. An event a widget must
//!   raise is its event type, `E`, and one it may is a [`Raises<E>`](Raises); markup sets both through `Into`, the
//!   widget naming its event type through [`Raising`].
//! - [`Screen`]: what the library keeps in the screen's actor, and what it does in the screen's turns, as
//!   [`screen!`] writes the screen's handlers: among them, the content of each open widget that arranges its children
//!   as a popup, a [`Popup`](widgets::Popup) or a program's own, drawn in the screen's top layer, and the wait of each
//!   opened by the pointer resting, a [`Tooltip`](widgets::Tooltip), a `Sleep` job.
//! - [`Called`] and [`Node::call`]: a child actor the screen spawns and holds, placed in a slot of its tree, called
//!   there in the area [`CallArea`] names, and mailed its size, a [`Resize`], whenever it changes; a child that is a
//!   screen lays out to it.
//! - [`DragSource`] and [`DropTarget`]: a widget a press and a move past [`DRAG_THRESHOLD`] drag, carrying the
//!   [`Payload`] its view copied, and one that takes a payload of the kind its drop's method takes last. A drop on the
//!   screen's own widget raises that widget's drop; one on another actor mails it [`Dropped`], which a screen's library
//!   takes in the same way and answers with one [`DropAnswered`], its [`DropOutcome`] saying whether a target took it;
//!   a drop taken, wherever, raises the source's `drop_accepted`, and one nothing takes its `drop_outside`.
//!
//! A screen is written in markup (ADR-0056: Markup): its view in [`view!`], its styles in [`style!`], its own
//! composites in [`widget!`], and, on the impl of the methods its events call, [`events`](macro@events), which declares
//! the types of what those events copy, since the markup writes only their values:
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
//!     price: u32,
//!     ui: Screen,
//! }
//!
//! aethereal_ui::style! {
//!     pub panel { padding: 8, gap: 4, background: rgb(28, 24, 20) }
//!     pub label: panel { font_size: 20, colour: "#e8d9a0" }
//! }
//!
//! aethereal_ui::widget! {
//!     /// A button that spends `amount`, labelled with it.
//!     pub struct Coin { amount: u32 }
//!     raises click;
//!
//!     Button { click => click,
//!         Text { "Spend {amount}" }
//!     }
//! }
//!
//! aethereal_ui::view! { Hud, ui,
//!     Column { style: panel,
//!         Text { style: label, "Gold: {self.gold}" }
//!         Coin { amount: self.price, click => self.spend(self.price) }
//!     }
//! }
//!
//! /// What the HUD's methods send: none of their own.
//!
//! impl Controls for Hud {
//! }
//!
//! #[aethereal_ui::events]
//! impl Hud {
//!     fn spend(&mut self, _: &mut Turn<'_, Self>, amount: u32) -> Result<(), Fault> {
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
//!     type Depends = aethereal_ui::depends!();
//!
//!     fn begin(frame: Frame, turn: &mut Beginning<'_, Self>) -> Result<Self, Fault> {
//!         let target = Target::of::<Interface>(frame);
//!         let ui = Screen::begin(target, Viewport::window(1, 1, 1.0), turn)?;
//!
//!         Ok(Self { gold: 100, price: 5, ui })
//!     }
//!
//!     fn end(self, _: &mut Ending<'_, Self>) -> Result<(), Fault> {
//!         Ok(())
//!     }
//! }
//! ```

#![forbid(unsafe_code)]

// The macros name the library from the root, `::aethereal_ui::…`, as a screen's crate does; its own tests are written
// in them too.
#[cfg(test)]
extern crate self as aethereal_ui;

mod child;
mod drag;
mod draw;
mod fonts;
mod input;
mod layout;
pub mod markdown;
pub mod pixels;
mod screen;
mod style;
mod table;
mod tree;
mod unkept;
mod view;
mod widget;
pub mod widgets;

pub use aethereal_ui_macros::{events, style, view, widget};
pub use child::{CallArea, Called, Resize, Resizes};
pub use drag::{
    Carried, DRAG_THRESHOLD, DragSource, DropAnswered, DropMethod, DropOutcome, DropTarget, Dropped, Drops, Payload,
    dropped,
};
pub use fonts::{Measuring, Unmeasured};
pub use input::{
    Clip, Erased, Event, First, Hears, Held, Input, Key, Pointing, Raised, Raises, Raising, Selecting, Used, raising,
};
pub use screen::{
    Clipping, Copying, DrawKey, Heard, ImeAsked, Instancing, Landing, Meshing, MetricsAsked, Ordering, Orders, Outcome,
    Pasting, Picturing, Placing, Prepared, Programming, Rows, Screen, Shaping, Target, TextPlacing, Texting, Texturing,
    Tipping, Uniforming, Viewport, Whitening,
};
pub use style::{Alignment, Border, Distribute, Edges, Gap, Length, Placed, SelfAlign, Space, Style, Track, rgb, rgba};
pub use table::{Dragging, Focus, Made, Mark, Selection, Table};
pub use tree::{Composite, Element, ItemKey, KeyPath, MOST_DEPTH, Node, Segment, Tree, TreeRefused, whole, windows};
#[doc(hidden)]
pub use tree::{Look, same};
pub use view::{Controls, Handed, Method, Took, View, take_in, unmatched};
pub use widget::{
    Anchor, Arrange, Axis, Draws, Inside, Jump, Keeps, Laid, Layer, Lines, Offer, Offered, Opens, Quad, Reads, Sample,
    Selected, Selects, Side, Slices, Span, Stand, Stateless, TextDraw, Tip, Widget,
};

/// What a screen's code names: the tree, the stock widgets, the style, the screen, and what its markup and its
/// controls are written as.
pub mod prelude {
    pub use crate::widgets::{
        Bar, Button, Column, Enter, Fill, Grid, Image, Keeps, Picture, Popup, Region, Row, Scroll, Selectable, Slider,
        Stack, Tab, Tabs, Text, TextInput, Texture, Toggle, Tooltip,
    };
    pub use crate::{
        Alignment, Border, CallArea, Called, Controls, Distribute, DragSource, DropTarget, Edges, Gap, Inside, ItemKey,
        Key, KeyPath, Length, Node, Payload, Placed, Raised, Raises, Rows, Screen, Segment, Selects, SelfAlign,
        Shaping, Side, Space, Style, Target, Track, Tree, View, Viewport, rgb, rgba,
    };
}

/// What [`screen!`], [`handles!`], and [`sends!`] name, so a screen's crate need not.
#[doc(hidden)]
pub mod __private {
    pub use crate::drag::Lent;
    pub use aethereal_actor::{Answer, Answered, Anyone, Fault, Handles, On, Path, Prepare, Receives, Slept, Turn};
    pub use aethereal_render::Renderer;
    pub use aethereal_render::frame::Shaped;
    pub use aethereal_render::instances::Instanced;
    pub use aethereal_render::mesh::Meshed;
    pub use aethereal_render::placement::Placed;
    pub use aethereal_render::program::Programmed;
    pub use aethereal_render::text::{FontMetrics, TextMade, TextPlaced};
    pub use aethereal_render::texture::Textured;
    pub use aethereal_render::uniforms::Uniformed;
    pub use aethereal_window::{AllowIme, Copied, ImeAllowed, Keyed, Pasted, Pointed, Windows};
}

/// A screen's `Handles`: what the library handles, `Prepare`, the pointer and the keys its list reaches, the answer to
/// each of its asks with the ask's context, the end of a tooltip's sleep with the tooltip and its hover, the clipboard's
/// answers to a paste and a copy with the widget that asked, the size a
/// screen that places it as a child mails it, a drag another actor dropped on it, and the answer, taken or refused, to
/// one it dropped on another, one kind with its outcome; then each of `$own`, what the screen handles of its own.
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
            $crate::__private::Answer<$crate::__private::Textured, $crate::Texturing>,
            $crate::__private::Answer<$crate::__private::FontMetrics, $crate::MetricsAsked>,
            $crate::__private::Answer<$crate::__private::Instanced, $crate::Instancing>,
            $crate::__private::Answer<$crate::__private::Placed, $crate::Placing>,
            $crate::__private::Answer<$crate::__private::TextMade, $crate::Texting>,
            $crate::__private::Answer<$crate::__private::TextPlaced, $crate::TextPlacing>,
            $crate::__private::Answer<$crate::__private::ImeAllowed, $crate::ImeAsked>,
            $crate::__private::Answer<$crate::__private::Slept, $crate::Tipping>,
            $crate::__private::Answer<$crate::__private::Pasted, $crate::Pasting>,
            $crate::__private::Answer<$crate::__private::Copied, $crate::Copying>,
            $crate::Resize,
            $crate::Dropped,
            $crate::DropAnswered,
            $($own,)*
        )
    };
}

/// A screen's `Depends`: the roots the library reaches by typed path, the renderer, which makes and draws what the
/// screen shows, and the windows, whose input method it stops; then each of `$own`, the types the screen's event
/// methods and its `key` reach by typed path of their own, none of the renderer's or the windows' trees (ADR-0059).
#[macro_export]
macro_rules! depends {
    ($($own:ty),* $(,)?) => {
        ($crate::__private::Renderer, $crate::__private::Windows, $($own,)*)
    };
}

/// Writes a screen's handlers of what is the library's, for the screen `$screen`, whose [`Screen`] is in its `$field`:
/// its `Prepare`, which builds the tree with its [`View`] and hands it to the screen with its [`Controls::HOTKEYS`],
/// once it has said whether it takes every key ([`Controls::takes_every_key`]); the pointer and the keys its list
/// reaches, whose raised events call its methods through [`View::on`] and whose keys left over call [`Controls::key`],
/// after which its view is built again for the next input to reach; each answer the
/// library's asks bring; the end of each tooltip's sleep; the clipboard's answers, which paste into the widget that
/// asked, or cut its selection, and call its methods with what that raises; the size a screen that places it as a child
/// mails it, which it
/// lays out to as a root screen lays out to its window's; and a drag dropped on it by another actor, which raises its
/// target's drop as a drop of its own does and is answered once, taken or refused, and the answer to one it dropped,
/// whose outcome raises its source's `drop_accepted` or `drop_outside`. A screen's markup compiles into a call of it.
#[macro_export]
macro_rules! screen {
    ($screen:ty, $field:ident) => {
        impl $crate::__private::Receives<$crate::__private::On<$crate::__private::Prepare>> for $screen {
            fn handle(
                &mut self,
                _: $crate::__private::Prepare,
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let every = <Self as $crate::Controls>::takes_every_key(self);
                self.$field.take_keys(every);
                let tree = <Self as $crate::View>::view(self);
                let hotkeys = <Self as $crate::Controls>::HOTKEYS;
                self.$field.prepare(tree, hotkeys).orders.send(turn)
            }
        }

        impl $crate::__private::Handles<$crate::__private::Pointed> for $screen {
            type From = $crate::__private::Anyone;

            fn handle(
                &mut self,
                pointed: $crate::__private::Pointed,
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let heard = self.$field.pointed(pointed, turn.me().id(), turn.now());
                $crate::screen!(@take $field, self, heard, turn)
            }
        }

        impl $crate::__private::Handles<$crate::__private::Keyed> for $screen {
            type From = $crate::__private::Anyone;

            fn handle(
                &mut self,
                keyed: $crate::__private::Keyed,
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let heard = self.$field.keyed(keyed, <Self as $crate::Controls>::FIRST);
                $crate::screen!(@take $field, self, heard, turn)
            }
        }

        impl $crate::__private::Receives<$crate::__private::Answer<$crate::__private::FontMetrics, $crate::MetricsAsked>>
            for $screen
        {
            fn handle(
                &mut self,
                (answered, asked): ($crate::__private::Answered<$crate::__private::FontMetrics>, $crate::MetricsAsked),
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let orders = self.$field.metrics(answered, asked, |blob| turn.read(blob));
                orders.send(turn)
            }
        }

        $crate::screen!(@answer $screen, $field, Shaped, Shaping, shaped);
        $crate::screen!(@answer $screen, $field, Meshed, Meshing, meshed);
        $crate::screen!(@answer $screen, $field, Programmed, Programming, programmed);
        $crate::screen!(@answer $screen, $field, Uniformed, Uniforming, uniformed);
        $crate::screen!(@answer $screen, $field, Textured, Texturing, textured);
        $crate::screen!(@answer $screen, $field, Instanced, Instancing, instanced);
        $crate::screen!(@answer $screen, $field, Placed, Placing, placed);
        $crate::screen!(@answer $screen, $field, TextMade, Texting, text_made);
        $crate::screen!(@answer $screen, $field, TextPlaced, TextPlacing, text_placed);
        $crate::screen!(@answer $screen, $field, ImeAllowed, ImeAsked, ime_answered);
        $crate::screen!(@answer $screen, $field, Slept, Tipping, slept);

        impl $crate::__private::Receives<$crate::__private::Answer<$crate::__private::Pasted, $crate::Pasting>>
            for $screen
        {
            fn handle(
                &mut self,
                (answered, pasting): ($crate::__private::Answered<$crate::__private::Pasted>, $crate::Pasting),
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let heard = self.$field.pasted(answered, pasting, |blob| turn.read(blob));
                $crate::screen!(@take $field, self, heard, turn)
            }
        }

        impl $crate::__private::Receives<$crate::__private::Answer<$crate::__private::Copied, $crate::Copying>>
            for $screen
        {
            fn handle(
                &mut self,
                (answered, copying): ($crate::__private::Answered<$crate::__private::Copied>, $crate::Copying),
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let heard = self.$field.copied(answered, copying);
                $crate::screen!(@take $field, self, heard, turn)
            }
        }

        impl $crate::__private::Handles<$crate::Resize> for $screen {
            type From = $crate::__private::Anyone;

            fn handle(
                &mut self,
                resize: $crate::Resize,
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                self.$field.resize(resize.viewport).send(turn)
            }
        }

        impl $crate::__private::Handles<$crate::Dropped> for $screen {
            type From = $crate::__private::Anyone;

            fn handle(
                &mut self,
                dropped: $crate::Dropped,
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let payload = dropped.payload.payload(|blob| turn.read(blob));
                match self.$field.dropped(dropped.entry, &payload) {
                    $crate::Landing::Taken(heard) => {
                        turn.reply(&dropped.answer($crate::DropOutcome::Accepted))?;
                        $crate::screen!(@take $field, self, heard, turn)
                    }
                    $crate::Landing::Refused => Ok(turn.reply(&dropped.answer($crate::DropOutcome::Refused))?),
                }
            }
        }

        impl $crate::__private::Handles<$crate::DropAnswered> for $screen {
            type From = $crate::__private::Anyone;

            fn handle(
                &mut self,
                answered: $crate::DropAnswered,
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                let payload = answered.payload.payload(|blob| turn.read(blob));
                let heard = self.$field.answered(&answered.source, payload, answered.outcome);
                $crate::screen!(@take $field, self, heard, turn)
            }
        }
    };
    (@take $field:ident, $this:ident, $heard:ident, $turn:ident) => {{
        let mut $heard = $heard;
        if $heard.copies_selection() {
            $this.$field.building_selection();
            let built = <Self as $crate::View>::view($this);
            $this.$field.copy_selection(&built, &mut $heard);
        }
        let took = $crate::take_in($this, $heard, $turn)?;
        if took == $crate::Took::Called {
            let tree = <Self as $crate::View>::view($this);
            $this.$field.rebuilt(tree);
        }

        Ok(())
    }};
    (@answer $screen:ty, $field:ident, $answer:ident, $context:ident, $method:ident) => {
        impl $crate::__private::Receives<$crate::__private::Answer<$crate::__private::$answer, $crate::$context>>
            for $screen
        {
            fn handle(
                &mut self,
                (answered, context): ($crate::__private::Answered<$crate::__private::$answer>, $crate::$context),
                turn: &mut $crate::__private::Turn<'_, Self>,
            ) -> ::core::result::Result<(), $crate::__private::Fault> {
                self.$field.$method(answered, context).send(turn)
            }
        }
    };
}
