//! What input a primitive takes in, and what it raises (ADR-0056: Events call the screen's methods; Focus and keys):
//! the pointer at it, in its own coordinates; a key, and typed text, while it has its screen's focus or a widget within
//! it has; and the events it raises, each a value of its screen's event type, with the value a widget hands with it.
//! The wheel is not a widget's: it slides the innermost scroll under the pointer that can still move that way.
//!
//! An event is a value, never a closure, so a tree still compares by value: the markup compiles each event
//! into a variant of one enum of the screen's, holding the arguments the view copied as it was built, and the screen's
//! [`View::on`](crate::View::on) matches each variant to the method it names.

mod raised;
mod raises;

pub use raised::{Erased, Event, Raised};
pub use raises::{Raises, Raising, raising};

use crate::fonts::Measuring;
use crate::style::Style;
use crate::widget::Laid;
use aethereal_data::Plain;
use aethereal_math::Vec2;
use aethereal_window::Button;

/// What the pointer did at a widget, each place in the widget's own coordinates: from its border box's top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pointing {
    /// The pointer now reaches it.
    Entered,
    /// The pointer no longer reaches it.
    Left,
    /// The pointer moved, to `at`: over it, or anywhere while a press on it is held.
    Moved { at: Vec2 },
    /// A button went down over it, at `at`: every move and the release come to it until every button is up.
    Down { at: Vec2, button: Button },
    /// A button came up, at `at`, after a press on it; `over` when what the pointer reaches at the release is this
    /// widget, as a click needs.
    Up { at: Vec2, button: Button, over: bool },
}

/// A key pressed, or repeated as it is held, as the window names it (ADR-0024): its place on the keyboard, `code`,
/// whatever the layout (`"KeyW"`, `"ArrowLeft"`); what it means under the layout, `key` (`"w"`, `"Enter"`); whether it
/// is the keyboard's repeat; and the modifiers held as it went down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub code: String,
    pub key: String,
    pub repeat: bool,
    pub held: Held,
}

impl Key {
    /// Whether it types text when pressed as it was: a key whose meaning is one character, with neither Control nor
    /// Meta held, as the window sends its text after it (ADR-0051).
    #[must_use]
    pub fn types(&self) -> bool {
        let mut characters = self.key.chars();
        let one = characters.next().is_some() && characters.next().is_none();

        one && !self.held.command
    }
}

/// The modifiers held, as the screen saw their keys go down and come up: Shift, and Control or Meta, the key a
/// command is chorded with on either kind of keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
pub struct Held {
    pub shift: bool,
    pub command: bool,
}

/// What a widget hears while it has its screen's focus: the keys at these places on the keyboard, declared with the
/// screen's own, or every key, blocking, with typed text and the window's input method, as a text field does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hears {
    Keys(&'static [&'static str]),
    Typing,
}

impl Hears {
    /// Whether it names the key at `code` among the keys it hears: a widget that types names none, as it hears every
    /// key only while it has focus itself.
    #[must_use]
    pub fn holds(self, code: &str) -> bool {
        match self {
            Self::Keys(keys) => keys.contains(&code),
            Self::Typing => false,
        }
    }
}

/// Whether a widget used a key: a key it passes goes on to its screen's focus keys, then to the screen's own `key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Used {
    Used,
    Passed,
}

/// What a widget is given as input reaches it: its style; where layout put it, in its own coordinates, its border box
/// from `0, 0`; the fonts its text measures by; and where the events it raises go.
pub struct Input<'input, 'fonts> {
    style: &'input Style,
    laid: Laid,
    measuring: &'input mut Measuring<'fonts>,
    raised: &'input mut Vec<Raised<Erased>>,
}

impl<'input, 'fonts> Input<'input, 'fonts> {
    /// Input to a widget styled `style`, laid at `laid` in its screen's pixels, measuring with `measuring`, raising into
    /// `raised`.
    pub(crate) fn new(
        style: &'input Style,
        laid: Laid,
        measuring: &'input mut Measuring<'fonts>,
        raised: &'input mut Vec<Raised<Erased>>,
    ) -> Self {
        Self { style, laid: laid.relative_to(laid.rect.min), measuring, raised }
    }

    /// Its style, resolved from the style its markup names.
    #[must_use]
    pub const fn style(&self) -> &Style {
        self.style
    }

    /// Where layout put it, in its own coordinates.
    #[must_use]
    pub const fn laid(&self) -> &Laid {
        &self.laid
    }

    /// The fonts its text measures by, as layout measured them.
    pub const fn measuring(&mut self) -> &mut Measuring<'fonts> {
        self.measuring
    }

    /// Raises `raised`, which calls its screen's method once the input is taken in.
    pub fn raise<E: Event>(&mut self, raised: Raised<E>) {
        self.raised.push(raised.erase());
    }
}
