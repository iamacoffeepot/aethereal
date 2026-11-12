//! What a widget pressed like a button keeps and shows: whether the pointer is over it and whether a press on it is
//! held, a click as a press and a release of the main button both on it, and a tint over its box for each and a ring
//! for focus, and for a drag it takes over it. `Button`, `Toggle`, and `Popup` are pressed so.

use crate::input::Pointing;
use crate::style::Style;
use crate::widget::{Draws, Laid, Quad, Sample};
use aethereal_data::Plain;
use aethereal_render::text::Colour;
use aethereal_window::Button;

/// The keys that press a widget pressed like a button while it has focus.
pub(super) const PRESSES: &[&str] = &["Enter", "NumpadEnter", "Space"];

/// The tint over a widget the pointer is over: a little lighter.
pub(super) const HOVERED: Colour = Colour { red: 1.0, green: 1.0, blue: 1.0, alpha: 0.12 };

/// The tint over a widget pressed: a little darker.
const PRESSED: Colour = Colour { red: 0.0, green: 0.0, blue: 0.0, alpha: 0.2 };

/// The ring inside the edge of a widget with focus.
pub(super) const RING: Colour = Colour { red: 1.0, green: 1.0, blue: 1.0, alpha: 0.7 };

/// How wide the focus ring is, in logical pixels.
pub(super) const RING_WIDTH: f32 = 2.0;

/// Whether the pointer is over a widget pressed like a button, and whether a press of the main button on it is held,
/// wherever the pointer has gone since.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Plain)]
#[kind(name = "aether.ui.press")]
pub struct Press {
    pub hovered: bool,
    pub pressed: bool,
}

impl Press {
    /// Takes in `pointing`, and answers whether it ended a click: a release of the main button over the widget after a
    /// press on it. A release elsewhere ends the press without one.
    pub fn pointed(&mut self, pointing: Pointing) -> bool {
        match pointing {
            Pointing::Entered => self.hovered = true,
            Pointing::Left => self.hovered = false,
            Pointing::Down { button: Button::Main, .. } => self.pressed = true,
            Pointing::Up { button: Button::Main, over, .. } => {
                let clicked = self.pressed && over;
                self.pressed = false;
                return clicked;
            }
            Pointing::Moved { .. } | Pointing::Down { .. } | Pointing::Up { .. } => {}
        }

        false
    }

    /// The tint over the widget as it stands: darker while pressed, lighter while the pointer is over it, and none
    /// otherwise.
    const fn tint(self) -> Colour {
        match (self.pressed, self.hovered) {
            (true, _) => PRESSED,
            (false, true) => HOVERED,
            (false, false) => Colour::CLEAR,
        }
    }
}

/// Draws over a widget `laid` and styled `style`, as `press` stands and its focus is: the tint, and the focus ring, which
/// a drag it takes over it shows as well.
pub fn shade(press: Press, style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
    let radius = style.radius * laid.scale;
    let tint = press.tint();
    let tinted = tint.alpha > 0.0;
    if tinted {
        draws.quad(Quad {
            rect: laid.rect,
            clip: laid.clip,
            colour: tint,
            border_width: 0.0,
            border_colour: tint,
            radius,
            sample: Sample::Solid,
        });
    }
    ring(style, laid, draws);
}

/// Draws the focus ring inside the edge of a widget `laid` and styled `style`, while it has focus, or while a drag of its
/// screen's carrying a payload it takes is over it, so a target lights up where a drop would land.
pub fn ring(style: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
    let focused = draws.focused();
    let lit = draws.drop_over();
    let shows = focused || lit;
    if !shows {
        return;
    }
    draws.quad(Quad {
        rect: laid.rect,
        clip: laid.clip,
        colour: Colour::CLEAR,
        border_width: RING_WIDTH * laid.scale,
        border_colour: RING,
        radius: style.radius * laid.scale,
        sample: Sample::Solid,
    });
}
