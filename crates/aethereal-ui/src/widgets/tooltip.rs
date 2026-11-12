//! `Tooltip`: a popup that opens itself (ADR-0056: Popups draw in one top layer per screen). Its first child is its
//! anchor and the rest its content, as a `Popup`'s are; once the pointer has rested on its anchor for its `delay`, its
//! content shows in its screen's top layer, placed against its anchor on its `side`, and it shuts as the pointer leaves
//! its anchor or a press is made.
//!
//! The wait is its own state: its screen waits for it with a `Sleep` job (ADR-0018), numbered by the hover it began
//! for, and a sleep that ends for a hover that has ended, or for one before the hover now, opens nothing.
//!
//! It never takes focus and never blocks the pointer: it is reached over its anchor with a hit that passes the pointer
//! on to what is beneath, which tells its screen the pointer is there over an anchor that is reached by nothing else, and
//! its content lists no hit and nothing Tab reaches.
//!
//! ```text
//! <Tooltip>                                    Tooltip::new()
//!     <ItemSlot item="{item}"/>                its anchor
//!     <Text class="tip">{item.name}</Text>     its content, shown once the pointer has rested on its anchor
//! </Tooltip>
//! ```

use super::popup::Side;
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Widget};
use aethereal_data::Plain;
use aethereal_render::draw_list::Pointer;
use aethereal_window::Cursor;
use core::time::Duration;

/// How long the pointer rests on a tooltip's anchor before it opens, unless it says otherwise: long enough that a
/// pointer passing over a row of slots opens none of them.
pub const DELAY: Duration = Duration::from_millis(500);

/// Content shown on `side` of its anchor once the pointer has rested on the anchor for `delay`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tooltip {
    pub side: Side,
    pub delay: Duration,
}

/// Where a tooltip stands with the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.tip")]
pub enum Tip {
    /// The pointer is not on its anchor.
    #[default]
    Away,
    /// The pointer came onto its anchor in the hover numbered `hover`, and it waits for that hover's sleep to end.
    Waiting { hover: u64 },
    /// Its content shows.
    Open,
    /// A press was made while the pointer was on its anchor: it stays shut until the pointer leaves.
    Dismissed,
}

impl Tooltip {
    /// A tooltip shown below its anchor after [`DELAY`].
    #[must_use]
    pub const fn new() -> Self {
        Self { side: Side::Below, delay: DELAY }
    }

    /// It, shown on `side` of its anchor.
    #[must_use]
    pub const fn side(self, side: Side) -> Self {
        Self { side, ..self }
    }

    /// It, shown once the pointer has rested on its anchor for `delay`.
    #[must_use]
    pub const fn delay(self, delay: Duration) -> Self {
        Self { delay, ..self }
    }
}

impl Default for Tooltip {
    fn default() -> Self {
        Self::new()
    }
}

impl Widget for Tooltip {
    type State = Tip;

    fn arrange(&self) -> Arrange {
        Arrange::Popup
    }

    fn draw(&self, _: &Tip, _: &Style, _: &Laid, _: &mut Draws<'_, '_>) {}

    fn pointer(&self, _: &Tip) -> Pointer {
        Pointer::Passes(Cursor::Default)
    }
}
