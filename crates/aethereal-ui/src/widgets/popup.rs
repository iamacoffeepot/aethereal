//! `Popup`: content shown over the rest of its screen, against the widget it is opened from (ADR-0056: Popups draw in
//! one top layer per screen). Its first child is its anchor, laid out in its box as a `Stack`'s child is; the rest are
//! its content, laid out and drawn in its screen's top layer while it is open, placed against its box on its `side`,
//! never cut by any widget around it.
//!
//! Whether it is open is its own state, as hover is a button's: it is the button of its anchor, drawn over it and
//! reached over it, so a click on its anchor opens it and a second shuts it, as Enter and Space do while it has focus.
//! Its screen shuts it when a press it hears lands outside it, when Escape is pressed while it is the topmost open, when
//! its anchor goes, and, for one whose `inside` closes, when a click within its content raises an event. A `modal`
//! popup blocks the pointer from the rest of its screen and holds Tab within its content while it is open.
//!
//! ```text
//! Popup { side: "below", inside: "closes",           Popup::new().closing()
//!     Row { style: field, Text { "{label}" } }        its anchor: what it is opened from
//!     Column { style: menu, … }                       its content, in the top layer while it is open
//! }
//! ```

use super::press::{PRESSES, Press, shade};
use crate::input::{Hears, Input, Key, Pointing, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Inside, Laid, Layer, Opens, Side, Widget};
use aethereal_data::Plain;
use aethereal_render::draw_list::Pointer;
use aethereal_window::Cursor;

/// Content shown over its screen while it is open, placed on `side` of its anchor: blocking the rest of its screen when
/// `modal`, and shut by a click within it as `inside` says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Popup {
    pub side: Side,
    pub modal: bool,
    pub inside: Inside,
}

/// Whether a popup is open, and its anchor's press as a button's is: the pointer over it and a press held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.popped")]
pub struct Popped {
    pub press: Press,
    pub open: Opened,
}

/// Whether a popup's content shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
pub enum Opened {
    #[default]
    Shut,
    Open,
}

impl Opened {
    /// The other.
    #[must_use]
    pub const fn toggled(self) -> Self {
        match self {
            Self::Shut => Self::Open,
            Self::Open => Self::Shut,
        }
    }
}

impl Popup {
    /// A popup whose content is placed below its anchor, blocks nothing beneath it, and stays open while it is clicked.
    #[must_use]
    pub const fn new() -> Self {
        Self { side: Side::Below, modal: false, inside: Inside::Stays }
    }

    /// It, its content placed on `side` of its anchor.
    #[must_use]
    pub const fn side(self, side: Side) -> Self {
        Self { side, ..self }
    }

    /// It, blocking the pointer from the rest of its screen and holding Tab within its content while it is open.
    #[must_use]
    pub const fn modal(self) -> Self {
        Self { modal: true, ..self }
    }

    /// It, shut by a click within its content that raises an event, once the event is raised.
    #[must_use]
    pub const fn closing(self) -> Self {
        Self { inside: Inside::Closes, ..self }
    }
}

impl Default for Popup {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Popup {
    type State = Popped;

    fn arrange(&self) -> Arrange {
        let Self { side, modal, inside } = *self;

        Arrange::Popup(Layer { side, opens: Opens::Pressed { modal, inside } })
    }

    fn open(&self, popped: &Popped) -> bool {
        popped.open == Opened::Open
    }

    fn shut(&self, popped: &mut Popped) {
        popped.open = Opened::Shut;
    }

    fn draw(&self, popped: &Popped, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        shade(popped.press, style, laid, draws);
    }

    fn pointer(&self, _: &Popped) -> Pointer {
        Pointer::Blocks(Cursor::Hand)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Keys(PRESSES)
    }

    fn pointed(&self, popped: &mut Popped, pointing: Pointing, _: &mut Input<'_, '_>) {
        let clicked = popped.press.pointed(pointing);
        if clicked {
            popped.open = popped.open.toggled();
        }
    }

    fn key(&self, popped: &mut Popped, key: &Key, _: &mut Input<'_, '_>) -> Used {
        let presses = PRESSES.contains(&key.code.as_str());
        if !presses {
            return Used::Passed;
        }
        if !key.repeat {
            popped.open = popped.open.toggled();
        }

        Used::Used
    }
}
