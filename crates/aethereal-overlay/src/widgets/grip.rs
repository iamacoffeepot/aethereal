//! [`Grip`]: the line between the band and the console, which a press and a drag move to set how tall the console is
//! (ADR-0057: The console is the chat). It draws its style's colour across the middle of its box, a seam twice a
//! divider's thickness, on its background; the box is taller than the line, so the line is easy to take hold of. While
//! a press on it is held it raises its `change` with each move, the logical pixels the pointer moved down since the
//! last, as the window keeps sending moves to what a press reached (ADR-0055: capture).

use crate::chart::Fill;
use crate::layout::SEAM_PIXELS;
use crate::widgets::paint;
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_ui::{Arrange, Draws, Event, Input, Laid, Pointing, Raised, Raising, Style, Widget};
use aethereal_window::{Button, Cursor};

/// A line a drag moves, raising `change` with how far down it moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grip<E> {
    pub change: E,
}

/// Whether a press on a grip is held, and the pointer's height, in the window's pixels, as it last raised its change.
#[derive(Debug, Clone, Copy, PartialEq, Default, Plain)]
#[kind(name = "aether.overlay.gripping")]
pub enum Gripping {
    #[default]
    Loose,
    Held {
        at: f32,
    },
}

impl<E: Event> Raising for Grip<E> {
    type Event = E;
}

impl<E: Event> Widget for Grip<E> {
    type State = Gripping;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Gripping, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let thick = (SEAM_PIXELS * laid.scale).max(1.0);
        let middle = laid.rect.min.y + laid.rect.height() / 2.0;
        let area = Rect2 {
            min: Vec2::new(laid.rect.min.x, (middle - thick / 2.0).round()),
            max: Vec2::new(laid.rect.max.x, (middle - thick / 2.0).round() + thick),
        };
        paint(draws, [Fill { area, colour: style.colour }], laid.clip);
    }

    fn pointer(&self, gripping: &Gripping) -> Pointer {
        let cursor = match gripping {
            Gripping::Loose => Cursor::Grab,
            Gripping::Held { .. } => Cursor::Grabbing,
        };

        Pointer::Blocks(cursor)
    }

    fn pointed(&self, gripping: &mut Gripping, pointing: Pointing, input: &mut Input<'_, '_>) {
        match (pointing, *gripping) {
            (Pointing::Down { at, button: Button::Main, .. }, _) => *gripping = Gripping::Held { at: at.y },
            (Pointing::Moved { at }, Gripping::Held { at: was }) => {
                let scale = input.laid().scale;
                input.raise(Raised::Number(self.change, (at.y - was) / scale));
                *gripping = Gripping::Held { at: at.y };
            }
            (Pointing::Up { button: Button::Main, .. }, _) => *gripping = Gripping::Loose,
            _ => {}
        }
    }
}
