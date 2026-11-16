//! [`StatusIcon`]: the small shape before a session's tab's title that marks its status (ADR-0057: The console is the
//! chat). The engine has no font with such symbols, so each is drawn as quads, rounded to circles where it is round,
//! centred in its box, which its style makes a fixed slot so the title beside it never moves as the shape changes.

use crate::chart::Fill;
use crate::widgets::paint;
use aethereal_math::{Rect2, Vec2};
use aethereal_render::text::Colour;
use aethereal_ui::{Arrange, Draws, Laid, Quad, Sample, Stateless, Style, Widget};

/// How wide a ring's line is, in logical pixels.
const RING_PIXELS: f32 = 1.5;

/// How wide a dash is, and a square's side, in logical pixels: short of the slot, so each reads as small beside the
/// round shapes that fill it.
const SHORT_PIXELS: f32 = 6.0;

/// How tall a dash is, in logical pixels.
const DASH_PIXELS: f32 = 1.5;

/// How far a square's corners are rounded, in logical pixels: a little, so it reads as a mark and not a hole.
const SQUARE_RADIUS_PIXELS: f32 = 1.0;

/// A shape a status is drawn as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// A circle's outline, the slot's width across.
    Ring,
    /// A filled circle, the slot's width across.
    Dot,
    /// A ring with its lower half filled.
    HalfRing,
    /// A short line across the slot's middle.
    Dash,
    /// A small square, slightly rounded.
    Square,
    /// Nothing: the console's own tab, which has no status.
    Blank,
}

/// A status's shape, in its colour, at the opacity it is drawn at now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatusIcon {
    pub shape: Shape,
    pub colour: Colour,
}

impl Widget for StatusIcon {
    type State = Stateless;

    fn arrange(&self) -> Arrange {
        Arrange::Leaf
    }

    fn draw(&self, _: &Stateless, _: &Style, laid: &Laid, draws: &mut Draws<'_, '_>) {
        let scale = laid.scale;
        let rect = laid.rect;
        let across = rect.width().min(rect.height()).round();
        let circle = centred(rect, Vec2::new(across, across));
        let round = |border: f32, colour: Colour, clip: Rect2| Quad {
            rect: circle,
            clip,
            colour,
            border_width: border,
            border_colour: self.colour,
            radius: across / 2.0,
            sample: Sample::Solid,
        };
        let ring = (RING_PIXELS * scale).max(1.0);

        match self.shape {
            Shape::Blank => {}
            Shape::Ring => draws.quad(round(ring, Colour::CLEAR, laid.clip)),
            Shape::Dot => draws.quad(round(0.0, self.colour, laid.clip)),
            Shape::HalfRing => {
                let lower = Rect2 { min: Vec2::new(circle.min.x, circle.center().y), max: circle.max };
                draws.quad(round(0.0, self.colour, laid.clip.intersection(lower)));
                draws.quad(round(ring, Colour::CLEAR, laid.clip));
            }
            Shape::Dash => {
                let dash = Vec2::new((SHORT_PIXELS * scale).round(), (DASH_PIXELS * scale).round().max(1.0));
                paint(draws, [Fill { area: centred(rect, dash), colour: self.colour }], laid.clip);
            }
            Shape::Square => {
                let square = (SHORT_PIXELS * scale).round();
                draws.quad(Quad {
                    rect: centred(rect, Vec2::new(square, square)),
                    clip: laid.clip,
                    colour: self.colour,
                    border_width: 0.0,
                    border_colour: self.colour,
                    radius: SQUARE_RADIUS_PIXELS * scale,
                    sample: Sample::Solid,
                });
            }
        }
    }
}

/// A box `size` across and down, centred in `rect` on whole pixels, so its edges fall crisply.
fn centred(rect: Rect2, size: Vec2) -> Rect2 {
    let min = Vec2::new(
        (rect.min.x + (rect.width() - size.x) / 2.0).round(),
        (rect.min.y + (rect.height() - size.y) / 2.0).round(),
    );

    Rect2 { min, max: Vec2::new(min.x + size.x, min.y + size.y) }
}
