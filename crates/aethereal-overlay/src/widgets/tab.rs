//! [`TabButton`]: a session tab of the console, pressed to open it (ADR-0057: The console is the chat). Its children
//! are its label, stacked in its box over its style's background, which is the open tab's surface while it is open.
//! The library's button shows the pointer over it as a fixed light tint, too bright for the tabs' dark strip; a tab
//! shows it as a soft shade of the palette's instead, a step between the strip and the open tab, and only while it is
//! closed, as the open tab is already lit. The open tab's line along its bottom edge is drawn here too, inset from
//! either side, so two tabs side by side never read as joined. It takes no focus: the console's line keeps the keys.

use crate::chart::Fill;
use crate::colour::{Ink, Palette, Surface};
use crate::layout::{TAB_ACCENT_INSET_PIXELS, TAB_ACCENT_PIXELS};
use crate::widgets::paint;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_ui::widgets::Press;
use aethereal_ui::{Arrange, Draws, Event, Input, Laid, Pointing, Raised, Raising, Style, Widget};
use aethereal_window::Cursor;

/// A tab that raises `click` when pressed, open or not, drawn in `palette`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabButton<E> {
    pub click: E,
    pub open: bool,
    pub palette: Palette,
}

impl<E: Event> Raising for TabButton<E> {
    type Event = E;
}

impl<E: Event> Widget for TabButton<E> {
    type State = Press;

    fn arrange(&self) -> Arrange {
        Arrange::Stack
    }

    fn draw(&self, press: &Press, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let rect = laid.rect;
        if self.open {
            let inset = (TAB_ACCENT_INSET_PIXELS * laid.scale).round();
            let thick = (TAB_ACCENT_PIXELS * laid.scale).round().max(1.0);
            let accent = Rect2 {
                min: Vec2::new(rect.min.x + inset, rect.max.y - thick),
                max: Vec2::new(rect.max.x - inset, rect.max.y),
            };
            paint(draws, [Fill { area: accent, colour: self.palette.ink(Ink::Lichen) }], laid.clip);
        } else if press.hovered {
            paint(draws, [Fill { area: rect, colour: self.palette.surface(Surface::Hover) }], laid.clip);
        }
    }

    fn pointer(&self, _: &Press) -> Pointer {
        Pointer::Blocks(Cursor::Hand)
    }

    fn pointed(&self, press: &mut Press, pointing: Pointing, input: &mut Input<'_, '_>) {
        let clicked = press.pointed(pointing);
        if clicked {
            input.raise(Raised::Event(self.click));
        }
    }
}
