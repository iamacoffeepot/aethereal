//! `Button`: a box pressed to act (ADR-0056: Stock widgets). A press and a release of the main button both on it raise
//! its `click`, as Enter or Space do while it has focus. Its children are its label, stacked in its box; it shows the
//! pointer over it and a press held as a tint over its background, and focus as a ring, and the pointer over it shows
//! the hand.

use super::press::{Press, shade};
use crate::input::{Event, Hears, Input, Key, Pointing, Raised, Used};
use crate::style::Style;
use crate::widget::{Arrange, Draws, Laid, Widget};
use aethereal_render::draw_list::Pointer;
use aethereal_window::Cursor;

/// The keys that press a button with focus.
const PRESSES: &[&str] = &["Enter", "NumpadEnter", "Space"];

/// A box that raises `click` when pressed, its children its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Button<E> {
    pub click: E,
}

impl<E> Button<E> {
    /// A button that raises `click`.
    #[must_use]
    pub const fn new(click: E) -> Self {
        Self { click }
    }
}

impl<E: Event> Widget for Button<E> {
    type State = Press;

    fn arrange(&self) -> Arrange {
        Arrange::Stack
    }

    fn draw(&self, press: &Press, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        shade(*press, style, laid, draws);
    }

    fn pointer(&self, _: &Press) -> Pointer {
        Pointer::Blocks(Cursor::Hand)
    }

    fn focusable(&self) -> bool {
        true
    }

    fn hears(&self) -> Hears {
        Hears::Keys(PRESSES)
    }

    fn pointed(&self, press: &mut Press, pointing: Pointing, input: &mut Input<'_, '_>) {
        let clicked = press.pointed(pointing);
        if clicked {
            input.raise(Raised::Event(self.click));
        }
    }

    fn key(&self, _: &mut Press, key: &Key, input: &mut Input<'_, '_>) -> Used {
        let presses = PRESSES.contains(&key.code.as_str());
        if !presses {
            return Used::Passed;
        }
        if !key.repeat {
            input.raise(Raised::Event(self.click));
        }

        Used::Used
    }
}
