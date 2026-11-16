//! [`Grip`]: the seam between the band and the console, which a press and a drag move to set how tall the console is
//! (ADR-0057: The console is the chat). On its background, the well, it draws a divider's thickness of near-black
//! along its top, the shadow the console's sheet casts on the band it is laid over, and a short handle in its style's
//! colour centred in its box; the whole box takes the drag, not only the handle, so the seam is easy to take hold of.
//! While a press on it is held it raises its `change` with each move, the logical pixels the pointer moved down since
//! the last, as the window keeps sending moves to what a press reached (ADR-0055: capture).

use crate::chart::Fill;
use crate::layout::{DIVIDER_PIXELS, HANDLE_PIXELS, HANDLE_WIDTH_PIXELS};
use crate::widgets::paint;
use aethereal_data::Plain;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::draw_list::Pointer;
use aethereal_render::text::Colour;
use aethereal_ui::{Arrange, Draws, Event, Input, Laid, Pointing, Raised, Raising, Style, Widget};
use aethereal_window::{Button, Cursor};

/// The shadow along the grip's top: black at 63%, dark enough to read as the console's edge against the band's
/// graphs, and soft enough to read as a shadow and not a line drawn.
const SHADOW: Colour = Colour { red: 0.0, green: 0.0, blue: 0.0, alpha: 0.63 };

/// A seam a drag moves, raising `change` with how far down it moved.
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
        let rect = laid.rect;
        let thick = (DIVIDER_PIXELS * laid.scale).max(1.0);
        let shadow = Rect2 { min: rect.min, max: Vec2::new(rect.max.x, rect.min.y + thick) };

        let wide = (HANDLE_WIDTH_PIXELS * laid.scale).round();
        let tall = (HANDLE_PIXELS * laid.scale).max(1.0).round();
        let left = (rect.min.x + (rect.width() - wide) / 2.0).round();
        let top = (rect.min.y + (rect.height() - tall) / 2.0).round();
        let handle = Rect2 { min: Vec2::new(left, top), max: Vec2::new(left + wide, top + tall) };

        let fills = [Fill { area: shadow, colour: SHADOW }, Fill { area: handle, colour: style.colour }];
        paint(draws, fills, laid.clip);
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
